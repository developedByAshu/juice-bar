//! The app's state: devices, settings and the poll. Scans run on a background thread and hand
//! their result back to the main thread, so the menu never waits on a dongle.

use crate::bluetooth::{bluetooth_devices, read_bluetooth_entries, remembered_bluetooth_names, BluetoothEntry};
use crate::delegate::{delegate, main_thread, start_timer, watch_system_events};
use crate::menu_ui::{self, PopupKind};
use crate::status_icon;
use crate::theme::set_theme_choice;
use crate::updater;
use juicebar_shared::demo::{is_demo_mode, sample_devices};
use juicebar_shared::device::Device;
use juicebar_shared::report::report_url;
use juicebar_shared::scan::{build_device, probe_usb_device, scan_dongles, ScannedDevice, UsbDevice};
use juicebar_shared::settings::{ClickAction, HoverAction, Settings};
use juicebar_shared::tone::{icon_tone, lowest_percent};
use juicebar_shared::update::is_check_due;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu, NSStatusItem};
use objc2_foundation::NSTimer;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};

// The first look for an update waits, so it never slows startup; then it looks hourly whether one is due.
const LAUNCH_CHECK_DELAY_SECONDS: f64 = 30.0;
const UPDATE_RECHECK_SECONDS: f64 = 60.0 * 60.0;

/// Everything kept between scans.
struct App {
    status_item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    settings: Settings,
    scanned: Vec<ScannedDevice>,
    bluetooth: Vec<BluetoothEntry>,
    devices: Vec<Device>,
    /// The same devices under their own names, before the user's renames, for the settings window.
    found: Vec<Device>,
    /// The rows in the menu, in menu order; a row's tag is its index here.
    shown: Vec<Device>,
    poll_timer: Option<Retained<NSTimer>>,
    is_scanning: bool,
    /// The popup the menu shows while it is open; None while it is closed.
    open_popup: Option<PopupKind>,
    /// Waits to see whether the pointer rests on the icon.
    hover_timer: Option<Retained<NSTimer>>,
    /// When each device (by row id) last gave a battery reading, so one missed read keeps its %.
    last_read: std::collections::HashMap<String, std::time::Instant>,
    /// Keyboards whose dongle said they fell asleep; these show as asleep at once, without the grace.
    asleep: Vec<(u16, u16)>,
    /// Each keyboard's last % from just before it fell asleep (row id to battery), put back the moment it
    /// wakes, so a wake read that misses does not leave the row blank until the next check.
    slept: std::collections::HashMap<String, juicebar_shared::device::Battery>,
    /// False until the first update timer fires, which is the "On launch" check.
    has_launch_check_run: bool,
    /// A downloaded, checked update and its version, waiting for the settings window to close.
    waiting_update: Option<(String, PathBuf)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<T>(action: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut state| state.as_mut().map(action)))
}

/// Starts the app and runs until Quit.
pub fn run() {
    let application = NSApplication::sharedApplication(main_thread());
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let app_delegate = delegate();
    application.setDelegate(Some(ProtocolObject::from_ref(&*app_delegate)));
    application.run();
}

/// Creates the menu bar item once launching has finished, then takes the first reading.
pub fn finish_startup() {
    crate::startup::refresh_path();
    let status_item = status_icon::create_status_icon();
    let menu = NSMenu::new(main_thread());
    menu.setDelegate(Some(ProtocolObject::from_ref(&*delegate())));
    status_icon::watch_clicks_and_hover(&status_item);
    let settings = Settings::load();
    set_theme_choice(settings.window_theme);
    let poll_timer = Some(start_timer(f64::from(settings.poll_minutes) * 60.0, sel!(poll:), true));
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App { status_item, menu, settings, scanned: Vec::new(), bluetooth: Vec::new(), devices: Vec::new(), found: Vec::new(), shown: Vec::new(), poll_timer, is_scanning: false, open_popup: None, hover_timer: None, last_read: std::collections::HashMap::new(), asleep: Vec::new(), slept: std::collections::HashMap::new(), has_launch_check_run: false, waiting_update: None });
    });
    start_timer(LAUNCH_CHECK_DELAY_SECONDS, sel!(updateTimer:), false);
    with_app(sync_item_menu);
    watch_system_events();
    refresh();
    start_capture_if_asked();
}

const HOVER_DELAY_SECONDS: f64 = 0.45;

/// Checks whether a left click opens Settings rather than the devices popup. The only place that names a
/// click choice, so the shared enum's names can change here alone.
fn click_opens_settings(settings: &Settings) -> bool {
    settings.click_action == ClickAction::Settings
}

/// Returns the click choice for the settings window's "On click" segments: Devices or Settings.
pub fn click_choice(opens_settings: bool) -> ClickAction {
    if opens_settings {
        ClickAction::Settings
    } else {
        ClickAction::Devices
    }
}

/// Returns true when the settings choose Settings for a left click, for the settings window.
pub fn is_click_settings(settings: &Settings) -> bool {
    click_opens_settings(settings)
}

/// Keeps the menu on the item while a left click shows the devices popup, as NetSpeedGauge keeps its
/// own menu on while a click shows its card. A click that opens Settings comes to the button instead,
/// and a right click then opens the menu itself.
fn sync_item_menu(app: &mut App) {
    let menu = (!click_opens_settings(&app.settings)).then(|| app.menu.clone());
    app.status_item.setMenu(menu.as_deref());
}

/// Returns the item's frame and its screen's usable area.
fn frames_of(item: &NSStatusItem) -> Option<(objc2_foundation::NSRect, objc2_foundation::NSRect)> {
    let window = item.button(main_thread()).and_then(|button| button.window())?;
    let area = window.screen()?.visibleFrame();
    Some((window.frame(), area))
}

fn cancel_hover(app: &mut App) {
    if let Some(timer) = app.hover_timer.take() {
        timer.invalidate();
    }
}

/// Starts waiting to see whether the pointer rests on the icon, when hovering shows the battery card.
pub fn on_icon_enter() {
    with_app(|app| {
        cancel_hover(app);
        if app.settings.hover_action == HoverAction::Card && app.open_popup.is_none() {
            app.hover_timer = Some(start_timer(HOVER_DELAY_SECONDS, sel!(hoverFired:), false));
        }
    });
}

/// Closes what hovering opened when the pointer leaves.
pub fn on_icon_exit() {
    with_app(cancel_hover);
    menu_ui::hide_hover_card();
}

/// Shows the battery card without taking the keyboard. It closes again when the pointer leaves.
pub fn on_hover_fired() {
    let wanted = with_app(|app| {
        app.hover_timer = None;
        (app.open_popup.is_none()).then(|| (app.shown.clone(), app.settings.menu_font_points, frames_of(&app.status_item)))
    })
    .flatten();
    if let Some((shown, points, frames)) = wanted {
        menu_ui::show_hover_card(&shown, points, frames);
    }
}

/// Handles a click that came to the button because no menu is attached: Settings on a left click, the
/// actions popup on a right or Control click.
pub fn on_icon_clicked() {
    with_app(cancel_hover);
    menu_ui::hide_hover_card();
    if crate::delegate::current_event_asks_for_menu() {
        open_item_menu();
    } else {
        crate::settings_ui::open_settings();
    }
}

/// Opens the menu from the item for a right click when a left click does something else, as NetSpeedGauge does.
pub fn open_item_menu() {
    let Some((item, menu)) = with_app(|app| (app.status_item.clone(), app.menu.clone())) else {
        return;
    };
    let Some(button) = item.button(main_thread()) else {
        return;
    };
    item.setMenu(Some(&menu));
    // SAFETY: a nil sender is allowed, and the button stays alive through the call.
    unsafe { button.performClick(None) };
    item.setMenu(None);
}

/// Forgets which popup was open once the menu closes.
pub fn on_menu_closed() {
    with_app(|app| app.open_popup = None);
}

/// Reads a capture switch from the environment. Only debug builds have capture mode.
#[cfg(debug_assertions)]
fn capture_switch(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
}

/// A release build reads no capture switch, so nothing outside can drive its UI or make it write files.
#[cfg(not(debug_assertions))]
fn capture_switch(_name: &str) -> Result<String, std::env::VarError> {
    Err(std::env::VarError::NotPresent)
}

/// Capture mode for screenshots without touching the mouse, in debug builds only: JUICEBAR_CAPTURE=menu (the devices popup),
/// actions (Settings and Quit), hover (the battery card) or settings opens that view after the first scan and quits a few seconds later. JUICEBAR_CAPTURE_PAGE picks the
/// settings page (general, appearance, devices, advanced, about). JUICEBAR_CAPTURE=reset walks a real reset with a
/// rename being typed, writes JUICEBAR_CAPTURE_PATH after it, then closes the window mid edit.
fn start_capture_if_asked() {
    match capture_switch("JUICEBAR_CAPTURE").as_deref() {
        Ok(CAPTURE_DEVICES | CAPTURE_ACTIONS) => {
            start_timer(3.0, sel!(captureOpenMenu:), false);
            start_timer(9.0, sel!(captureFinish:), false);
        }
        Ok("rename") => {
            crate::settings_ui::open_settings();
            start_timer(2.0, sel!(captureRename:), false);
            start_timer(4.0, sel!(captureFinish:), false);
        }
        Ok(CAPTURE_HOVER) => {
            start_timer(3.0, sel!(hoverFired:), false);
            start_timer(5.0, sel!(captureFinish:), false);
        }
        Ok("settings") => {
            crate::settings_ui::open_settings();
            let page = capture_switch("JUICEBAR_CAPTURE_PAGE").unwrap_or_default();
            if let Some(index) = CAPTURE_PAGES.iter().position(|name| *name == page) {
                crate::settings_ui::show_page_for_checks(index);
            }
            start_timer(4.0, sel!(captureFinish:), false);
        }
        Ok("reset") => {
            crate::settings_ui::open_settings();
            start_timer(2.0, sel!(captureReset:), false);
            start_timer(4.0, sel!(captureCloseWhileEditing:), false);
            start_timer(6.0, sel!(captureFinish:), false);
        }
        _ => {}
    }
}

// Capture mode names: the devices popup a left click opens, Settings and Quit from a right click, and the hover card.
const CAPTURE_DEVICES: &str = "menu";
const CAPTURE_ACTIONS: &str = "actions";
const CAPTURE_HOVER: &str = "hover";

fn capture_wants(mode: &str) -> bool {
    capture_switch("JUICEBAR_CAPTURE").as_deref() == Ok(mode)
}

/// The settings pages capture mode can open, in sidebar order.
const CAPTURE_PAGES: [&str; 5] = ["general", "appearance", "devices", "advanced", "about"];

fn log_renames(step: &str) {
    let renames = with_app(|app| app.settings.renames.clone()).unwrap_or_default();
    eprintln!("capture: {step}: saved renames {renames:?}");
}

/// Types a rename, leaves the page mid edit, then resets everything through the confirm card. Used only by capture mode.
pub fn capture_reset() {
    use crate::settings_ui::{click_for_checks, is_reset_done_for_checks, show_page_for_checks, type_rename_for_checks};
    show_page_for_checks(2);
    eprintln!("capture: typed a rename: {}", type_rename_for_checks(0, "Desk keyboard"));
    eprintln!("capture: opened Advanced while typing: {}", click_for_checks("page:3"));
    log_renames("after leaving the rename (expected unsaved)");
    eprintln!("capture: asked to reset: {}", click_for_checks("ask_reset"));
    if let Ok(path) = capture_switch("JUICEBAR_CAPTURE_CONFIRM_PATH") {
        eprintln!("capture: confirm card saved: {}", crate::settings_ui::capture_to(&path));
    }
    eprintln!("capture: confirmed the reset: {}", click_for_checks("confirm_reset"));
    log_renames("after the reset");
    eprintln!("capture: done cue showing: {}", is_reset_done_for_checks());
    if let Ok(path) = capture_switch("JUICEBAR_CAPTURE_PATH") {
        eprintln!("capture: reset page saved: {}", crate::settings_ui::capture_to(&path));
    }
}

/// Walks the rename as a user would: the idle pencil, the pencil opening the field, Return, ✕, clear and ✓,
/// a click elsewhere, and Escape. Each step is painted in both looks under JUICEBAR_CAPTURE_DIR. Used only by capture mode.
pub fn capture_rename() {
    use crate::settings_ui::{capture_both_looks_for_checks, click_for_checks, editing_row_for_checks, leave_rename_for_checks, press_in_rename_for_checks, show_page_for_checks, type_rename_for_checks};
    let folder = capture_switch("JUICEBAR_CAPTURE_DIR").unwrap_or_default();
    let shot = |name: &str| eprintln!("capture: {name} saved: {}", capture_both_looks_for_checks(&format!("{folder}/rename_{name}")));
    show_page_for_checks(2);
    shot("1_idle");
    eprintln!("capture: pencil opened row 0: {}", click_for_checks("edit:0"));
    eprintln!("capture: editing row {:?}", editing_row_for_checks());
    shot("2_editing");
    type_rename_for_checks(0, "Desk keyboard");
    eprintln!("capture: pressed Return: {}", press_in_rename_for_checks(0, true));
    log_renames("right after Return");
    eprintln!("capture: editing row after Return {:?}", editing_row_for_checks());
    shot("3_saved");
    type_rename_for_checks(1, "Oops typo");
    shot("4_typing");
    eprintln!("capture: clicked the cross: {}", click_for_checks("cancel_name"));
    log_renames("after the cross");
    shot("5_cancelled");
    type_rename_for_checks(0, "");
    eprintln!("capture: clicked the tick: {}", click_for_checks("save:0"));
    log_renames("after clearing and the tick");
    shot("6_cleared");
    type_rename_for_checks(1, "Travel mouse");
    leave_rename_for_checks();
    log_renames("after typing and clicking elsewhere (expected unsaved)");
    eprintln!("capture: editing row after clicking elsewhere {:?}", editing_row_for_checks());
    type_rename_for_checks(1, "Travel mouse");
    press_in_rename_for_checks(1, false);
    log_renames("after typing and Escape (expected unsaved)");
    shot("7_after_escape");
}

/// Types a rename and closes the window with Escape before it is finished. Used only by capture mode.
pub fn capture_close_while_editing() {
    crate::settings_ui::show_page_for_checks(2);
    eprintln!("capture: typed a rename: {}", crate::settings_ui::type_rename_for_checks(1, "Travel mouse"));
    crate::settings_ui::press_escape_for_checks();
    log_renames("after closing mid edit (expected unsaved)");
}

/// Opens the menu under the item without any click. Used only by capture mode.
pub fn capture_open_menu() {
    let Some((item, menu)) = with_app(|app| (app.status_item.clone(), app.menu.clone())) else {
        return;
    };
    #[allow(deprecated)]
    item.popUpStatusItemMenu(&menu);
}

/// Closes an open menu and quits. Used only by capture mode.
pub fn capture_finish() {
    if capture_switch("JUICEBAR_CAPTURE").as_deref() == Ok("reset") {
        eprintln!("capture: still running after the reset, quitting");
    } else if let Ok(path) = capture_switch("JUICEBAR_CAPTURE_PATH") {
        let menu = with_app(|app| app.menu.clone());
        let is_menu = capture_wants(CAPTURE_DEVICES) || capture_wants(CAPTURE_ACTIONS);
        let saved = match menu {
            Some(menu) if is_menu => menu_ui::capture_open_menu(&menu, &path),
            _ if capture_wants(CAPTURE_HOVER) => menu_ui::capture_hover_card(&path),
            _ => crate::settings_ui::capture_to(&path),
        };
        eprintln!("capture: saved {path}: {saved}");
    }
    with_app(|app| app.menu.cancelTracking());
    NSApplication::sharedApplication(main_thread()).terminate(None);
}

/// Rebuilds the visible rows from the last scan, then redraws the icon and an open menu.
fn rebuild(app: &mut App) {
    let mut rows: Vec<Device> = if is_demo_mode() {
        sample_devices(82, 15)
    } else {
        let mut rows: Vec<Device> = app.scanned.iter().map(|scanned| scanned.device.clone()).collect();
        let mut bluetooth = app.bluetooth.clone();
        crate::ble_battery::fill_levels(&mut bluetooth);
        rows.extend(bluetooth_devices(&bluetooth));
        juicebar_shared::device::drop_idle_twins(rows)
    };
    app.found = rows.clone();
    for row in &mut rows {
        if let Some(name) = app.settings.renames.get(&row.id) {
            row.name = name.clone();
        }
    }
    app.devices = rows;
    // Settings lists every device; the icon and menu leave out the hidden ones.
    let shown: Vec<Device> = app.devices.iter().filter(|device| !app.settings.hidden.contains(&device.id)).cloned().collect();
    status_icon::update_status_icon(&app.status_item, icon_tone(&shown), lowest_percent(&shown), app.settings.shows_percent_in_icon);
    menu_ui::show_menu(&app.menu, &shown, app.settings.menu_font_points, app.open_popup.unwrap_or(PopupKind::Devices));
    if menu_ui::is_hover_card_open() {
        menu_ui::show_hover_card(&shown, app.settings.menu_font_points, frames_of(&app.status_item));
    }
    app.shown = shown;
    crate::settings_ui::on_devices_changed(&app.settings, &app.found);
}

/// Work for the HID thread.
enum HidJob {
    Scan(Settings),
    Probe { tag: isize, usb: UsbDevice, settings: Settings, bluetooth: Vec<BluetoothEntry> },
}

thread_local! {
    static HID_JOBS: RefCell<Option<Sender<HidJob>>> = const { RefCell::new(None) };
}

/// Starts the one thread that owns the HID manager. macOS ties hidapi's global manager to the run loop
/// of the thread that created it, so every scan must run on that same thread.
fn start_hid_thread() -> Sender<HidJob> {
    let (sender, receiver) = channel::<HidJob>();
    std::thread::spawn(move || {
        let mut api = hidapi::HidApi::new().ok();
        // Shared, as browsers open devices: macOS refuses an exclusive open of a mouse to a normal app
        // (privilege violation), which hid the XS Flow mouse's battery report.
        if let Some(api) = api.as_ref() {
            api.set_open_exclusive(false);
        }
        // Keyboard dongles held open between scans, so their pushed events (wake, sleep, battery)
        // arrive as they happen. Waiting on them is the callback; nothing is polled on a timer.
        let mut watches: Vec<KeyboardWatch> = Vec::new();
        loop {
            let job = if watches.is_empty() {
                match receiver.recv() {
                    Ok(job) => Some(job),
                    Err(_) => break,
                }
            } else {
                match receiver.try_recv() {
                    Ok(job) => Some(job),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            };
            // HID objects macOS hands back while opening and reading devices are autoreleased. This plain
            // thread has no pool of its own, so without one per pass every scan leaked about 1 MB of them.
            objc2::rc::autoreleasepool(|_| {
                if let Some(job) = job {
                    if let Some(api) = api.as_mut() {
                        let _ = api.refresh_devices();
                    }
                    match job {
                        HidJob::Scan(settings) => {
                            // The mouse's interface (page 0x0A) opens only with Input Monitoring; ask once, when such a dongle is present.
                            if let Some(api) = api.as_ref() {
                                let needs_monitoring = api.device_list().any(|info| info.usage_page() == 0x000A && juicebar_shared::catalog::find_known(info.vendor_id(), info.product_id(), 0x000A).is_some());
                                if needs_monitoring && !crate::permissions::has_input_monitoring() {
                                    crate::permissions::request_input_monitoring();
                                }
                            }
                            let bluetooth = read_bluetooth_entries();
                            let scanned = api.as_ref().map_or_else(Vec::new, |api| scan_dongles(api, &settings, &|kind| remembered_bluetooth_names(&bluetooth, kind)));
                            watches = api.as_ref().map_or_else(Vec::new, open_keyboard_watches);
                            dispatch2::DispatchQueue::main().exec_async(move || {
                                with_app(|app| {
                                    let scanned = keep_recent_readings(app, scanned);
                                    app.scanned = scanned;
                                    crate::ble_battery::refresh(&bluetooth);
                                    app.bluetooth = bluetooth;
                                    app.is_scanning = false;
                                    rebuild(app);
                                });
                            });
                        }
                        HidJob::Probe { tag, usb, settings, bluetooth } => {
                            let Some(api) = api.as_ref() else {
                                return;
                            };
                            let (reading, protocol) = probe_usb_device(api, &usb);
                            let device = build_device(&usb, &reading, protocol, &settings, &remembered_bluetooth_names(&bluetooth, usb.kind()));
                            dispatch2::DispatchQueue::main().exec_async(move || {
                                with_app(|app| {
                                    if let Some(scanned) = app.scanned.get_mut(tag as usize) {
                                        scanned.device = device;
                                    }
                                    rebuild(app);
                                });
                            });
                        }
                    }
                }
                wait_for_keyboard_events(&mut watches);
            });
        }
    });
    sender
}

/// How long a device keeps its last % through missed reads. A read can miss now and then (the
/// mouse pushes its report every few seconds, a busy keyboard can answer late), and one miss must not
/// blank a % that was fine a minute ago.
const KEEP_READING_FOR: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Fills a missed reading with the device's last %, unless its dongle said it is asleep or the last
/// reading is older than `KEEP_READING_FOR`. Records when each device last answered.
fn keep_recent_readings(app: &mut App, mut scanned: Vec<ScannedDevice>) -> Vec<ScannedDevice> {
    let now = std::time::Instant::now();
    for new in &mut scanned {
        let id = new.device.id.clone();
        if new.device.battery.is_some() {
            app.last_read.insert(id, now);
            continue;
        }
        let is_asleep = app.asleep.contains(&(new.device.vendor_id, new.device.product_id));
        let is_recent = app.last_read.get(&id).is_some_and(|read| now.duration_since(*read) < KEEP_READING_FOR);
        if !is_asleep && is_recent {
            new.device.battery = app.scanned.iter().find(|old| old.device.id == id).and_then(|old| old.device.battery);
        }
    }
    scanned
}

/// A keyboard dongle's command channel, held open to hear what it pushes.
struct KeyboardWatch {
    vendor_id: u16,
    product_id: u16,
    device: hidapi::HidDevice,
}

/// How long one pass waits on the held channels before checking for new work, in total.
const WATCH_WAIT_MS: i32 = 500;

/// Opens (shared) the command channel of every known bytech keyboard dongle that is plugged in.
fn open_keyboard_watches(api: &hidapi::HidApi) -> Vec<KeyboardWatch> {
    api.device_list()
        .filter(|info| juicebar_shared::catalog::find_known(info.vendor_id(), info.product_id(), info.usage_page()).is_some_and(|known| known.protocol == "bytech"))
        .filter_map(|info| info.open_device(api).ok().map(|device| KeyboardWatch { vendor_id: info.vendor_id(), product_id: info.product_id(), device }))
        .collect()
}

/// Waits briefly on the held channels and hands each pushed event to the main thread. A channel that
/// errors (dongle unplugged) is dropped; the next scan opens it again.
fn wait_for_keyboard_events(watches: &mut Vec<KeyboardWatch>) {
    if watches.is_empty() {
        return;
    }
    let wait = WATCH_WAIT_MS / watches.len() as i32;
    watches.retain(|watch| {
        let mut buffer = [0u8; 64];
        match watch.device.read_timeout(&mut buffer, wait) {
            Ok(size) => {
                if let Some(event) = juicebar_shared::protocols::bytech::parse_event(&buffer[..size]) {
                    let (vendor_id, product_id) = (watch.vendor_id, watch.product_id);
                    dispatch2::DispatchQueue::main().exec_async(move || on_keyboard_event(vendor_id, product_id, event));
                }
                true
            }
            Err(_) => false,
        }
    });
}

/// Acts on a keyboard event at once: a wake reads the battery now, sleep marks it, a push shows it.
fn on_keyboard_event(vendor_id: u16, product_id: u16, event: juicebar_shared::protocols::bytech::KeyboardEvent) {
    use juicebar_shared::protocols::bytech::KeyboardEvent;
    with_app(|app| {
        app.asleep.retain(|ids| *ids != (vendor_id, product_id));
        if event == KeyboardEvent::Asleep {
            app.asleep.push((vendor_id, product_id));
        }
    });
    if event == KeyboardEvent::Awake {
        // Show the % it had before sleeping at once, and count it as fresh so a missed wake read keeps it.
        with_app(|app| {
            let now = std::time::Instant::now();
            for scanned in app.scanned.iter_mut().filter(|scanned| scanned.device.vendor_id == vendor_id && scanned.device.product_id == product_id) {
                if scanned.device.battery.is_none() {
                    if let Some(battery) = app.slept.remove(&scanned.device.id) {
                        scanned.device.battery = Some(battery);
                        app.last_read.insert(scanned.device.id.clone(), now);
                    }
                }
            }
            rebuild(app);
        });
        refresh();
        return;
    }
    with_app(|app| {
        for scanned in app.scanned.iter_mut().filter(|scanned| scanned.device.vendor_id == vendor_id && scanned.device.product_id == product_id) {
            if event == KeyboardEvent::Asleep {
                if let Some(battery) = scanned.device.battery {
                    app.slept.insert(scanned.device.id.clone(), battery);
                }
            }
            // A push carries only the %: it keeps the charging state the last real reading gave.
            scanned.device.battery = match event {
                KeyboardEvent::Battery(pushed) => Some(juicebar_shared::protocols::bytech::with_pushed_percent(scanned.device.battery, pushed)),
                _ => None,
            };
        }
        rebuild(app);
    });
}

fn send_hid_job(job: HidJob) {
    HID_JOBS.with(|cell| {
        let mut slot = cell.borrow_mut();
        let sender = slot.get_or_insert_with(start_hid_thread);
        let _ = sender.send(job);
    });
}

/// A Bluetooth device reported a new level or went away: redraw with what is known.
pub fn on_bluetooth_battery() {
    with_app(rebuild);
}

/// Asks the HID thread for a fresh scan of dongles and Bluetooth, unless one is running.
pub fn refresh() {
    let Some(settings) = with_app(|app| {
        if app.is_scanning || is_demo_mode() {
            rebuild(app);
            return None;
        }
        app.is_scanning = true;
        Some(app.settings.clone())
    })
    .flatten() else {
        return;
    };
    send_hid_job(HidJob::Scan(settings));
}

/// Picks the popup for this opening, shows the last known rows at once and asks for fresh ones; an open
/// popup updates in place. A right or Control click opens Settings and Quit; a left click the devices.
pub fn on_menu_opening(_menu: &NSMenu) {
    let wants_actions = crate::delegate::current_event_asks_for_menu() || capture_wants(CAPTURE_ACTIONS);
    let kind = if wants_actions { PopupKind::Actions } else { PopupKind::Devices };
    menu_ui::hide_hover_card();
    with_app(|app| {
        cancel_hover(app);
        app.open_popup = Some(kind);
        rebuild(app);
    });
    refresh();
}

/// Redraws the icon after a light or dark mode change.
pub fn redraw_icon() {
    with_app(rebuild);
}

/// Runs the opt-in probe on one unknown dongle, once the user has agreed to it the first time.
pub fn probe(tag: isize) {
    if !with_app(|app| app.settings.is_probe_agreed).unwrap_or(false) {
        if !asks_to_probe() {
            return;
        }
        with_app(|app| {
            app.settings.is_probe_agreed = true;
            app.settings.save();
            // The open window keeps its own copy, so it must not save the old choice back later.
            crate::settings_ui::on_devices_changed(&app.settings, &app.found);
        });
    }
    let job = with_app(|app| {
        let id = &app.shown.get(tag as usize)?.id;
        let index = app.scanned.iter().position(|scanned| &scanned.device.id == id)?;
        Some(HidJob::Probe { tag: index as isize, usb: app.scanned[index].usb.clone(), settings: app.settings.clone(), bluetooth: app.bluetooth.clone() })
    })
    .flatten();
    if let Some(job) = job {
        send_hid_job(job);
    }
}

/// Asks once whether Try to detect may send a battery read command to an unknown device. True for Try.
fn asks_to_probe() -> bool {
    use juicebar_shared::scan::{PROBE_DETAIL, PROBE_QUESTION};
    let mtm = main_thread();
    let alert = objc2_app_kit::NSAlert::new(mtm);
    alert.setMessageText(&objc2_foundation::NSString::from_str(PROBE_QUESTION));
    alert.setInformativeText(&objc2_foundation::NSString::from_str(PROBE_DETAIL));
    alert.addButtonWithTitle(&objc2_foundation::NSString::from_str("Try"));
    alert.addButtonWithTitle(&objc2_foundation::NSString::from_str("Cancel"));
    let application = NSApplication::sharedApplication(mtm);
    // SAFETY: a plain call on the shared application; a menu bar app must come forward for its alert to show.
    let _: () = unsafe { objc2::msg_send![&*application, activateIgnoringOtherApps: true] };
    alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
}

/// Returns the Report link for the device behind a row.
pub fn report_url_for(tag: isize) -> Option<String> {
    with_app(|app| {
        let device = app.shown.get(tag as usize)?;
        let scanned = app.scanned.iter().find(|scanned| scanned.device.id == device.id);
        let interfaces: Vec<_> = scanned.map(|scanned| scanned.usb.interfaces.iter().map(|(summary, _)| summary.clone()).collect()).unwrap_or_default();
        let os = format!("macOS {}", objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersionString());
        Some(report_url(device, scanned.map(|scanned| scanned.usb.product.as_str()), &interfaces, &os, juicebar_shared::APP_VERSION))
    })
    .flatten()
}

/// Returns the menu bar item's frame and the usable area of its screen, which the menu is placed against.
pub fn item_and_screen_frames() -> Option<(objc2_foundation::NSRect, objc2_foundation::NSRect)> {
    with_app(|app| frames_of(&app.status_item)).flatten()
}

/// Returns a copy of the settings and every device under its own name, for the settings window.
pub fn settings_and_devices() -> Option<(Settings, Vec<Device>)> {
    with_app(|app| (app.settings.clone(), app.found.clone()))
}

/// Saves new settings, restarts the poll if its interval changed, and redraws.
/// Returns false when the app state was busy (or not started), so nothing was applied.
pub fn apply_settings(settings: Settings) -> bool {
    with_app(|app| {
        let interval_changed = settings.poll_minutes != app.settings.poll_minutes;
        let theme_changed = settings.window_theme != app.settings.window_theme;
        settings.save();
        app.settings = settings;
        sync_item_menu(app);
        if theme_changed {
            // Only Juice Bar's own window follows the choice; the menu bar icon keeps the Mac's look.
            set_theme_choice(app.settings.window_theme);
            crate::settings_ui::apply_theme();
        }
        if interval_changed {
            if let Some(timer) = app.poll_timer.take() {
                timer.invalidate();
            }
            app.poll_timer = Some(start_timer(f64::from(app.settings.poll_minutes) * 60.0, sel!(poll:), true));
        }
        rebuild(app);
    })
    .is_some()
}

/// Looks for an update when the chosen interval says one is due. The first run is the launch check.
pub fn on_update_timer() {
    let due = with_app(|app| {
        let is_launch = !app.has_launch_check_run;
        if is_launch {
            app.has_launch_check_run = true;
            // The one-shot launch timer has fired; the hourly one takes over.
            start_timer(UPDATE_RECHECK_SECONDS, sel!(updateTimer:), true);
        }
        is_check_due(app.settings.update_check, app.settings.last_update_check, updater::now_seconds(), is_launch)
    });
    if due == Some(true) {
        updater::check_now();
    }
}

/**
 * Takes a step of the update check: saves when GitHub answered, and once the settings window is
 * closed asks before a checked update goes in. On Install the app quits and the install script opens
 * the new copy; on Later the download is kept and the next check asks again.
 */
pub fn on_update() {
    if let Some(outcome) = updater::take_outcome() {
        with_app(|app| {
            if let Some(checked_at) = outcome.checked_at {
                app.settings.last_update_check = checked_at;
                app.settings.save();
                // The open window keeps its own copy, so it must not save the old time back later.
                crate::settings_ui::on_devices_changed(&app.settings, &app.found);
            }
            if outcome.new_copy.is_some() {
                app.waiting_update = outcome.new_copy;
            }
        });
    }
    crate::settings_ui::refresh();
    if crate::settings_ui::is_open() {
        return;
    }
    let Some((version, new_copy)) = with_app(|app| app.waiting_update.take()).flatten() else {
        return;
    };
    if !asks_to_install(&version) {
        return;
    }
    if updater::install(&new_copy) {
        updater::forget_ready();
        NSApplication::sharedApplication(main_thread()).terminate(None);
    }
}

/// Asks "Juice Bar 0.2.0 is ready. Install now?" with Install and Later. Returns true for Install.
fn asks_to_install(version: &str) -> bool {
    let mtm = main_thread();
    let alert = objc2_app_kit::NSAlert::new(mtm);
    alert.setMessageText(&objc2_foundation::NSString::from_str(&juicebar_shared::update::install_question("Juice Bar", version)));
    alert.setInformativeText(&objc2_foundation::NSString::from_str("Juice Bar quits, swaps in the new version and opens again."));
    alert.addButtonWithTitle(&objc2_foundation::NSString::from_str("Install"));
    alert.addButtonWithTitle(&objc2_foundation::NSString::from_str("Later"));
    let application = NSApplication::sharedApplication(mtm);
    // SAFETY: a plain call on the shared application; a menu bar app must come forward for its alert to show.
    let _: () = unsafe { objc2::msg_send![&*application, activateIgnoringOtherApps: true] };
    alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
}
