//! Check for updates on the Mac. A background thread asks GitHub for the latest release with the
//! Mac's own curl, downloads SHA256SUMS.txt and its signature, checks the signature with the release
//! key, then downloads the app's zip, checks its sha256, unzips it with ditto and checks it is Juice
//! Bar signed as this copy is. Once Settings is closed the app asks first; on Install a small shell
//! script waits for this copy to quit, swaps the two apps and opens the new one. Nothing about the
//! user is sent: only the app's name, as the User-Agent.

use juicebar_shared::update::{
    asset_url, expected_sha, is_allowed_download_url, is_newer, is_release_signed, is_sha_match, latest_release_url, mac_asset_name, release_tag, signature_url, user_agent,
    UpdateStatus,
};
use objc2_foundation::{NSDate, NSDateFormatter, NSLocale, NSString};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// The program's name in the release files, the app bundle and the User-Agent.
const PROGRAM_NAME: &str = "JuiceBar";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const SUMS_NAME: &str = "SHA256SUMS.txt";
const RELEASE_ACCEPT: &str = "application/vnd.github+json";
const FILE_ACCEPT: &str = "application/octet-stream";
const CURL: &str = "/usr/bin/curl";
const DITTO: &str = "/usr/bin/ditto";
const PLIST_BUDDY: &str = "/usr/libexec/PlistBuddy";
const SHELL: &str = "/bin/sh";
const CODESIGN: &str = "/usr/bin/codesign";
/// How `codesign -d -r-` starts the line with an app's designated requirement.
const REQUIREMENT_PREFIX: &str = "designated => ";
const TIMEOUT_SECONDS: &str = "30";
// Far above the app's size, so a broken answer cannot fill the disk.
const MAX_DOWNLOAD_BYTES: &str = "67108864";
/// The folders under the user's own Library that hold the download until it goes in, only the user can open.
const CACHES_FOLDER: &str = "Library/Caches/JuiceBar";
const WORK_FOLDER_NAME: &str = "update";
const PRIVATE_FOLDER_MODE: u32 = 0o700;
const UNPACKED_FOLDER_NAME: &str = "unpacked";
/// $0 of the inline install script, as `sh -c` names it.
const SCRIPT_NAME: &str = "juicebar-install";
const PROBE_NAME: &str = ".JuiceBar-update-probe";
// "28 Nov 15:20" in the Mac's own time zone, with English month names whatever the Mac's language.
const TIME_FORMAT: &str = "d MMM HH:mm";
const TIME_LOCALE: &str = "en_US_POSIX";

/**
 * Swaps the apps once this copy has quit. $1 is this copy's process id, $2 the installed app, $3 the
 * new app, $4 where the installed app waits aside, $5 the update folder. The old app goes back if the
 * new one cannot go in, and whichever app is in place opens again.
 */
const INSTALL_SCRIPT: &str = r#"
while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
if mv "$2" "$4"; then
    if mv "$3" "$2"; then
        rm -rf "$4"
    else
        mv "$4" "$2"
    fi
fi
open "$2"
rm -rf "$5"
"#;

/// What a finished check hands the main thread.
pub struct Outcome {
    /// When GitHub answered, to save as the last check; None when it could not be reached.
    pub checked_at: Option<u64>,
    /// The checked new app, unzipped in the update folder, with its version.
    pub new_copy: Option<(String, PathBuf)>,
}

static STATUS: Mutex<UpdateStatus> = Mutex::new(UpdateStatus::Idle);
static OUTCOME: Mutex<Option<Outcome>> = Mutex::new(None);
static IS_BUSY: AtomicBool = AtomicBool::new(false);
/// The last checked download and its tag, kept when the user says Later so the next check need not fetch it again.
static READY: Mutex<Option<(String, PathBuf)>> = Mutex::new(None);

/// Returns where the Check now row stands.
pub fn status() -> UpdateStatus {
    STATUS.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Sets the row's state and hands the step to the main thread, which saves it and redraws Settings.
fn set_status(status: UpdateStatus) {
    *STATUS.lock().unwrap_or_else(PoisonError::into_inner) = status;
    dispatch2::DispatchQueue::main().exec_async(crate::app::on_update);
}

/// Takes the last finished check, once.
pub fn take_outcome() -> Option<Outcome> {
    OUTCOME.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Returns the time now in seconds since 1970.
pub fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs())
}

/// Starts a check on a background thread, unless one is running. Each step reaches `app::on_update`.
pub fn check_now() {
    if IS_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        set_status(UpdateStatus::Checking);
        let (status, outcome) = run_check();
        *OUTCOME.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        IS_BUSY.store(false, Ordering::SeqCst);
        set_status(status);
    });
}

/// Asks GitHub for the latest release and, when it is newer, downloads, checks and unzips it.
fn run_check() -> (UpdateStatus, Outcome) {
    let agent = user_agent(PROGRAM_NAME);
    let unreachable = (UpdateStatus::Failed, Outcome { checked_at: None, new_copy: None });
    let Some(json) = latest_release_url(REPOSITORY).and_then(|url| fetch(&url, &agent, RELEASE_ACCEPT)).and_then(|body| String::from_utf8(body).ok()) else {
        return unreachable;
    };
    let checked = Outcome { checked_at: Some(now_seconds()), new_copy: None };
    let Some(tag) = release_tag(&json) else { return (UpdateStatus::Failed, checked) };
    if !is_newer(VERSION, &tag) {
        return (UpdateStatus::UpToDate, checked);
    }
    let kept = READY.lock().unwrap_or_else(PoisonError::into_inner).clone().filter(|(ready_tag, path)| *ready_tag == tag && path.exists());
    if let Some(kept) = kept {
        return (UpdateStatus::Ready(tag), Outcome { new_copy: Some(kept), ..checked });
    }
    set_status(UpdateStatus::Downloading(tag.clone()));
    match download_checked(&json, &tag, &agent).and_then(|zip| unpack(&zip)) {
        Some(new_copy) => {
            let ready = (tag.clone(), new_copy);
            *READY.lock().unwrap_or_else(PoisonError::into_inner) = Some(ready.clone());
            (UpdateStatus::Ready(tag), Outcome { new_copy: Some(ready), ..checked })
        }
        None => (UpdateStatus::Failed, checked),
    }
}

/**
 * Downloads SHA256SUMS.txt and its signature, and only when the release key signed those exact bytes
 * downloads the Mac zip, kept only when its sha256 matches. A missing or bad signature is no update;
 * the next check tries again.
 */
fn download_checked(json: &str, tag: &str, agent: &str) -> Option<Vec<u8>> {
    let asset = mac_asset_name(PROGRAM_NAME);
    let sums = download(&asset_url(json, SUMS_NAME)?, agent)?;
    let signature = download(&signature_url(REPOSITORY, tag)?, agent).and_then(|body| String::from_utf8(body).ok())?;
    if !is_release_signed(&sums, &signature) {
        return None;
    }
    let expected = expected_sha(std::str::from_utf8(&sums).ok()?, &asset)?;
    let zip = download(&asset_url(json, &asset)?, agent)?;
    is_sha_match(&zip, &expected).then_some(zip)
}

/// Downloads an update file, but only from the owner's GitHub or GitHub's file stores.
fn download(url: &str, agent: &str) -> Option<Vec<u8>> {
    is_allowed_download_url(url).then(|| fetch(url, agent, FILE_ACCEPT)).flatten()
}

/// Returns ~/Library/Caches/JuiceBar/update, the folder that holds the download until it goes in,
/// made only the user can open. None without a home folder.
fn work_folder() -> Option<PathBuf> {
    let caches = PathBuf::from(std::env::var_os("HOME").filter(|home| !home.is_empty())?).join(CACHES_FOLDER);
    for folder in [&caches, &caches.join(WORK_FOLDER_NAME)] {
        match std::fs::DirBuilder::new().mode(PRIVATE_FOLDER_MODE).create(folder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
        // A folder that was already there is made private too, and must be a real folder, not a link.
        if !std::fs::symlink_metadata(folder).is_ok_and(|meta| meta.is_dir()) {
            return None;
        }
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(PRIVATE_FOLDER_MODE)).ok()?;
    }
    Some(caches.join(WORK_FOLDER_NAME))
}

/**
 * Unzips the checked zip into a fresh update folder and returns the new app, but only when its bundle
 * id is the running app's and its code signature is valid and satisfies the running app's own
 * designated requirement, so nothing else can be swapped in under Juice Bar's name.
 */
fn unpack(zip: &[u8]) -> Option<PathBuf> {
    let folder = work_folder()?;
    let _ = std::fs::remove_dir_all(&folder);
    let folder = work_folder()?;
    let archive = folder.join(mac_asset_name(PROGRAM_NAME));
    let unpacked = folder.join(UNPACKED_FOLDER_NAME);
    let is_unzipped = std::fs::write(&archive, zip).is_ok()
        && Command::new(DITTO).args(["-x", "-k"]).arg(&archive).arg(&unpacked).stdin(Stdio::null()).status().is_ok_and(|status| status.success());
    let _ = std::fs::remove_file(&archive);
    let new_copy = unpacked.join(format!("{PROGRAM_NAME}.app"));
    let running = running_bundle();
    let running_id = running.as_deref().and_then(bundle_identifier);
    let is_ours = is_unzipped && running_id.is_some() && bundle_identifier(&new_copy) == running_id && running.as_deref().is_some_and(|running| is_signed_like(&new_copy, running));
    if !is_ours {
        let _ = std::fs::remove_dir_all(&folder);
        return None;
    }
    Some(new_copy)
}

/// Returns the app bundle the running program sits in: …/JuiceBar.app/Contents/MacOS/JuiceBar gives …/JuiceBar.app.
fn running_bundle() -> Option<PathBuf> {
    let program = std::env::current_exe().ok()?;
    let bundle = program.parent()?.parent()?.parent()?;
    bundle.extension().is_some_and(|extension| extension == "app").then(|| bundle.to_path_buf())
}

/// Reads an app's CFBundleIdentifier from its Info.plist, or None when it has none.
fn bundle_identifier(app: &Path) -> Option<String> {
    let plist = app.join("Contents").join("Info.plist");
    let output = Command::new(PLIST_BUDDY).args(["-c", "Print CFBundleIdentifier"]).arg(&plist).stdin(Stdio::null()).output().ok()?;
    let identifier = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !identifier.is_empty()).then_some(identifier)
}

/// Returns an app's designated requirement as `codesign -d -r-` prints it, or None when it has none.
fn designated_requirement(app: &Path) -> Option<String> {
    let output = Command::new(CODESIGN).args(["-d", "-r-"]).arg(app).stdin(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    // Older macOS prints it on stderr, newer on stdout.
    let text = [output.stdout, output.stderr].concat();
    String::from_utf8_lossy(&text).lines().find_map(|line| line.strip_prefix(REQUIREMENT_PREFIX).map(|rule| rule.trim().to_string())).filter(|rule| !rule.is_empty())
}

/**
 * Returns true when the new app's signature is valid, deep and strict, its designated requirement is
 * the running app's, and it satisfies that requirement (same bundle id and, when signed with the
 * owner's certificate, the same certificate).
 */
fn is_signed_like(new_copy: &Path, running: &Path) -> bool {
    let Some(requirement) = designated_requirement(running) else { return false };
    if designated_requirement(new_copy).as_deref() != Some(requirement.as_str()) {
        return false;
    }
    Command::new(CODESIGN)
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={requirement}"))
        .arg(new_copy)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Returns true when a new folder can be made in `folder`, so the installed app can be moved there.
fn is_writable(folder: &Path) -> bool {
    let probe = folder.join(PROBE_NAME);
    let is_made = std::fs::create_dir(&probe).is_ok();
    let _ = std::fs::remove_dir(&probe);
    is_made
}

/**
 * Starts the install script, detached, and returns true; the caller quits right after. The new app
 * takes the installed app's exact path, so Open at login and the menu bar place stay as they are.
 * Returns false, with nothing changed, when this copy is not in an app bundle or its folder cannot
 * be written.
 */
pub fn install(new_copy: &Path) -> bool {
    let Some(installed) = running_bundle() else { return false };
    let Some(parent) = installed.parent() else { return false };
    if !is_writable(parent) {
        return false;
    }
    let Some(folder) = work_folder() else { return false };
    let name = installed.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let set_aside = installed.with_file_name(format!("{name}.old"));
    let _ = std::fs::remove_dir_all(&set_aside);
    // Inline, so no script file sits anywhere another user could change it. Its own process group,
    // so the script keeps running after this copy quits.
    Command::new(SHELL)
        .arg("-c")
        .arg(INSTALL_SCRIPT)
        .arg(SCRIPT_NAME)
        .arg(std::process::id().to_string())
        .arg(&installed)
        .arg(new_copy)
        .arg(&set_aside)
        .arg(&folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .is_ok()
}

/// Forgets a kept download once it went in or the app quits for it.
pub fn forget_ready() {
    *READY.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Returns a time in seconds since 1970 as local "28 Nov 15:20", or None before the first check.
pub fn time_label(seconds: u64) -> Option<String> {
    if seconds == 0 {
        return None;
    }
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(&NSString::from_str(TIME_LOCALE))));
    formatter.setDateFormat(Some(&NSString::from_str(TIME_FORMAT)));
    let date = NSDate::dateWithTimeIntervalSince1970(seconds as f64);
    Some(formatter.stringFromDate(&date).to_string())
}

/**
 * GETs an https address with the Mac's curl and returns the body of a successful answer. curl follows
 * GitHub's redirect to its file store, https only. -q first skips any ~/.curlrc, and `--` keeps the
 * address from being read as an option.
 */
fn fetch(url: &str, agent: &str, accept: &str) -> Option<Vec<u8>> {
    let header = format!("Accept: {accept}");
    let output = Command::new(CURL)
        .args(["-q", "-fsSL", "--proto", "=https", "--proto-redir", "=https", "--max-time", TIMEOUT_SECONDS, "--max-filesize", MAX_DOWNLOAD_BYTES, "-A", agent, "-H", header.as_str(), "--", url])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}
