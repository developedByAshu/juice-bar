//! "Show on the taskbar": keeps the tray icon next to the clock instead of under the ^ arrow.
//! Windows 11 stores that choice per program in the user's NotifyIconSettings; this flips the
//! same switch as Settings > Personalisation > Taskbar > Other system tray icons.

use crate::startup::program_path;
use crate::util::to_wide;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

const ICON_SETTINGS: &str = "Control Panel\\NotifyIconSettings";
const PATH_VALUE: &str = "ExecutablePath";
const PROMOTED_VALUE: &str = "IsPromoted";
const MAX_KEY_UNITS: usize = 256;
const MAX_PATH_UNITS: usize = 1024;

/// Returns true when a stored program path is this program, ignoring case as Windows paths do.
pub fn is_same_program(stored: &str, program: &str) -> bool {
    stored.eq_ignore_ascii_case(program)
}

/// Reads one text value of a key under the tray icon settings.
fn read_path(key: &str) -> Option<String> {
    let (path, name) = (to_wide(&format!("{ICON_SETTINGS}\\{key}")), to_wide(PATH_VALUE));
    let mut buffer = vec![0u16; MAX_PATH_UNITS];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: the strings are null-terminated, and the size passed matches the buffer.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size) };
    (status == 0).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
}

/// Returns the name of this program's entry in Windows' tray icon list, once Windows has made one.
fn own_key() -> Option<String> {
    let program = program_path()?;
    let root = to_wide(ICON_SETTINGS);
    let mut settings: HKEY = std::ptr::null_mut();
    // SAFETY: `root` is null-terminated and `settings` receives the opened key, closed below.
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, root.as_ptr(), 0, KEY_READ, &mut settings) } != 0 {
        return None;
    }
    let mut found = None;
    for index in 0.. {
        let mut name = [0u16; MAX_KEY_UNITS];
        let mut length = name.len() as u32;
        // SAFETY: `name` holds `length` units; the other out pointers are optional and left null.
        let status = unsafe {
            RegEnumKeyExW(settings, index, name.as_mut_ptr(), &mut length, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        if status != 0 {
            break;
        }
        let key = String::from_utf16_lossy(&name[..length as usize]);
        if read_path(&key).is_some_and(|stored| is_same_program(&stored, &program)) {
            found = Some(key);
            break;
        }
    }
    // SAFETY: the key was opened above and is closed once here.
    unsafe { RegCloseKey(settings) };
    found
}

/// Returns true when Windows shows the icon on the taskbar rather than under the ^ arrow.
pub fn is_pinned() -> bool {
    let Some(key) = own_key() else { return false };
    let (path, name) = (to_wide(&format!("{ICON_SETTINGS}\\{key}")), to_wide(PROMOTED_VALUE));
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: the strings are null-terminated, and `value` and `size` are valid out pointers.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), std::ptr::from_mut(&mut value).cast(), &mut size) };
    status == 0 && value == 1
}

/// Shows the icon on the taskbar or puts it back under the ^ arrow. Returns false when Windows has
/// no entry for the icon yet, which happens only before its first start.
pub fn set_pinned(pinned: bool) -> bool {
    let Some(key) = own_key() else { return false };
    let (path, name) = (to_wide(&format!("{ICON_SETTINGS}\\{key}")), to_wide(PROMOTED_VALUE));
    let value = u32::from(pinned);
    // SAFETY: the strings are null-terminated and `value` outlives the call.
    let status = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), REG_DWORD, std::ptr::from_ref(&value).cast(), std::mem::size_of::<u32>() as u32) };
    status == 0
}
