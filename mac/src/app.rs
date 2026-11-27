//! The app's state: devices, settings and the poll. Scans run on a background thread and hand
//! their result back to the main thread, so the menu never waits on a dongle.

use crate::bluetooth::{bluetooth_devices, read_bluetooth_entries, remembered_bluetooth_names, BluetoothEntry};
use crate::delegate::{delegate, main_thread, start_timer, watch_system_events};
use crate::{menu_ui, status_icon};
use juicebar_shared::demo::{is_demo_mode, sample_devices};
use juicebar_shared::device::Device;
use juicebar_shared::report::report_url;
use juicebar_shared::scan::{build_device, probe_usb_device, scan_dongles, ScannedDevice, UsbDevice};
use juicebar_shared::settings::Settings;
use juicebar_shared::tone::{icon_tone, lowest_percent};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu, NSStatusItem};
use objc2_foundation::NSTimer;
use std::cell::RefCell;
use std::sync::mpsc::{channel, Sender};

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
    status_item.setMenu(Some(&menu));
    let settings = Settings::load();
    let poll_timer = Some(start_timer(f64::from(settings.poll_minutes) * 60.0, sel!(poll:), true));
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App { status_item, menu, settings, scanned: Vec::new(), bluetooth: Vec::new(), devices: Vec::new(), found: Vec::new(), shown: Vec::new(), poll_timer, is_scanning: false });
    });
    watch_system_events();
    refresh();
    start_capture_if_asked();
}

/// Capture mode for screenshots without touching the mouse: JUICEBAR_CAPTURE=menu or settings
/// opens that view after the first scan and quits a few seconds later. JUICEBAR_CAPTURE_PAGE picks the
/// settings page (general, devices, reset, details). JUICEBAR_CAPTURE=reset walks a real reset with a
/// rename being typed, writes JUICEBAR_CAPTURE_PATH after it, then closes the window mid edit.
fn start_capture_if_asked() {
    match std::env::var("JUICEBAR_CAPTURE").as_deref() {
        Ok("menu") => {
            start_timer(3.0, sel!(captureOpenMenu:), false);
            start_timer(9.0, sel!(captureFinish:), false);
        }
        Ok("settings") => {
            crate::settings_ui::open_settings();
            let page = std::env::var("JUICEBAR_CAPTURE_PAGE").unwrap_or_default();
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

/// The settings pages capture mode can open, in sidebar order.
const CAPTURE_PAGES: [&str; 4] = ["general", "devices", "reset", "details"];

fn log_renames(step: &str) {
    let renames = with_app(|app| app.settings.renames.clone()).unwrap_or_default();
    eprintln!("capture: {step}: saved renames {renames:?}");
}

/// Types a rename, leaves the page mid edit, then resets everything through the confirm card. Used only by capture mode.
pub fn capture_reset() {
    use crate::settings_ui::{click_for_checks, is_reset_done_for_checks, show_page_for_checks, type_rename_for_checks};
    show_page_for_checks(1);
    eprintln!("capture: typed a rename: {}", type_rename_for_checks(0, "Desk keyboard"));
    eprintln!("capture: opened Reset while typing: {}", click_for_checks("page:2"));
    log_renames("after leaving the rename");
    eprintln!("capture: asked to reset: {}", click_for_checks("ask_reset"));
    if let Ok(path) = std::env::var("JUICEBAR_CAPTURE_CONFIRM_PATH") {
        eprintln!("capture: confirm card saved: {}", crate::settings_ui::capture_to(&path));
    }
    eprintln!("capture: confirmed the reset: {}", click_for_checks("confirm_reset"));
    log_renames("after the reset");
    eprintln!("capture: done cue showing: {}", is_reset_done_for_checks());
    if let Ok(path) = std::env::var("JUICEBAR_CAPTURE_PATH") {
        eprintln!("capture: reset page saved: {}", crate::settings_ui::capture_to(&path));
    }
}

/// Types a rename and closes the window with Escape before it is finished. Used only by capture mode.
pub fn capture_close_while_editing() {
    crate::settings_ui::show_page_for_checks(1);
    eprintln!("capture: typed a rename: {}", crate::settings_ui::type_rename_for_checks(1, "Travel mouse"));
    crate::settings_ui::press_escape_for_checks();
    log_renames("after closing mid edit");
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
    if std::env::var("JUICEBAR_CAPTURE").as_deref() == Ok("reset") {
        eprintln!("capture: still running after the reset, quitting");
    } else if let Ok(path) = std::env::var("JUICEBAR_CAPTURE_PATH") {
        let menu = with_app(|app| app.menu.clone());
        let is_menu = std::env::var("JUICEBAR_CAPTURE").as_deref() == Ok("menu");
        let saved = match menu {
            Some(menu) if is_menu => menu_ui::capture_open_menu(&menu, &path),
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
        rows.extend(bluetooth_devices(&app.bluetooth));
        rows
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
    menu_ui::show_menu(&app.menu, &shown, app.settings.menu_font_points);
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
        for job in receiver {
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
                    dispatch2::DispatchQueue::main().exec_async(move || {
                        with_app(|app| {
                            app.scanned = scanned;
                            app.bluetooth = bluetooth;
                            app.is_scanning = false;
                            rebuild(app);
                        });
                    });
                }
                HidJob::Probe { tag, usb, settings, bluetooth } => {
                    let Some(api) = api.as_ref() else {
                        continue;
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
    });
    sender
}

fn send_hid_job(job: HidJob) {
    HID_JOBS.with(|cell| {
        let mut slot = cell.borrow_mut();
        let sender = slot.get_or_insert_with(start_hid_thread);
        let _ = sender.send(job);
    });
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

/// Shows the last known rows at once and asks for fresh ones; an open menu updates in place.
pub fn on_menu_opening(_menu: &NSMenu) {
    with_app(rebuild);
    refresh();
}

/// Redraws the icon after a light or dark mode change.
pub fn redraw_icon() {
    with_app(rebuild);
}

/// Runs the opt-in probe on one unknown dongle.
pub fn probe(tag: isize) {
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

/// Returns the Report link for the device behind a row.
pub fn report_url_for(tag: isize) -> Option<String> {
    with_app(|app| {
        let device = app.shown.get(tag as usize)?;
        let interfaces: Vec<_> = app.scanned.iter().find(|scanned| scanned.device.id == device.id).map(|scanned| scanned.usb.interfaces.iter().map(|(summary, _)| summary.clone()).collect()).unwrap_or_default();
        let os = format!("macOS {}", objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersionString());
        Some(report_url(device, &interfaces, &os, juicebar_shared::APP_VERSION))
    })
    .flatten()
}

/// Returns the menu bar item's frame and the usable area of its screen, which the menu is placed against.
pub fn item_and_screen_frames() -> Option<(objc2_foundation::NSRect, objc2_foundation::NSRect)> {
    let window = with_app(|app| app.status_item.button(main_thread()).and_then(|button| button.window())).flatten()?;
    let area = window.screen()?.visibleFrame();
    Some((window.frame(), area))
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
        settings.save();
        app.settings = settings;
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
