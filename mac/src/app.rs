//! The app's state: devices, settings and the poll. Scans run on a background thread and hand
//! their result back to the main thread, so the menu never waits on a dongle.

use crate::bluetooth::{bluetooth_devices, read_bluetooth_entries, remembered_bluetooth_names, BluetoothEntry};
use crate::delegate::{delegate, main_thread, start_timer, watch_system_events};
use crate::{menu_ui, status_icon};
use juicebar_shared::demo::{is_demo_mode, sample_devices};
use juicebar_shared::device::Device;
use juicebar_shared::report::report_url;
use juicebar_shared::scan::{build_device, probe_usb_device, scan_dongles, ScannedDevice};
use juicebar_shared::settings::Settings;
use juicebar_shared::tone::{icon_tone, lowest_percent};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu, NSStatusItem};
use objc2_foundation::NSTimer;
use std::cell::RefCell;

/// Everything kept between scans.
struct App {
    status_item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    settings: Settings,
    scanned: Vec<ScannedDevice>,
    bluetooth: Vec<BluetoothEntry>,
    devices: Vec<Device>,
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
        *cell.borrow_mut() = Some(App { status_item, menu, settings, scanned: Vec::new(), bluetooth: Vec::new(), devices: Vec::new(), poll_timer, is_scanning: false });
    });
    watch_system_events();
    refresh();
}

/// Rebuilds the visible rows from the last scan, then redraws the icon and an open menu.
fn rebuild(app: &mut App) {
    if is_demo_mode() {
        app.devices = sample_devices(82, 15);
    } else {
        let mut rows: Vec<Device> = app.scanned.iter().map(|scanned| scanned.device.clone()).collect();
        rows.extend(bluetooth_devices(&app.bluetooth).into_iter().filter(|device| !app.settings.hidden.contains(&device.id)));
        for row in &mut rows {
            if let Some(name) = app.settings.renames.get(&row.id) {
                row.name = name.clone();
            }
        }
        app.devices = rows;
    }
    status_icon::update_status_icon(&app.status_item, icon_tone(&app.devices), lowest_percent(&app.devices), app.settings.shows_percent_in_icon);
    menu_ui::show_menu(&app.menu, &app.devices);
}

/// Starts a background scan of dongles and Bluetooth, unless one is running.
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
    std::thread::spawn(move || {
        let bluetooth = read_bluetooth_entries();
        let scanned = hidapi::HidApi::new().map_or_else(|_| Vec::new(), |api| scan_dongles(&api, &settings, &|kind| remembered_bluetooth_names(&bluetooth, kind)));
        dispatch2::DispatchQueue::main().exec_async(move || {
            with_app(|app| {
                app.scanned = scanned;
                app.bluetooth = bluetooth;
                app.is_scanning = false;
                rebuild(app);
            });
        });
    });
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
    let Some((usb, settings, bluetooth)) = with_app(|app| app.scanned.get(tag as usize).map(|scanned| (scanned.usb.clone(), app.settings.clone(), app.bluetooth.clone()))).flatten() else {
        return;
    };
    std::thread::spawn(move || {
        let Ok(api) = hidapi::HidApi::new() else {
            return;
        };
        let (reading, protocol) = probe_usb_device(&api, &usb);
        let device = build_device(&usb, &reading, protocol, &settings, &remembered_bluetooth_names(&bluetooth, usb.kind()));
        dispatch2::DispatchQueue::main().exec_async(move || {
            with_app(|app| {
                if let Some(scanned) = app.scanned.get_mut(tag as usize) {
                    scanned.device = device;
                }
                rebuild(app);
            });
        });
    });
}

/// Returns the Report link for the device behind a row.
pub fn report_url_for(tag: isize) -> Option<String> {
    with_app(|app| {
        let device = app.devices.get(tag as usize)?;
        let interfaces: Vec<_> = app.scanned.iter().find(|scanned| scanned.device.id == device.id).map(|scanned| scanned.usb.interfaces.iter().map(|(summary, _)| summary.clone()).collect()).unwrap_or_default();
        let os = format!("macOS {}", objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersionString());
        Some(report_url(device, &interfaces, &os, juicebar_shared::APP_VERSION))
    })
    .flatten()
}

/// Returns a copy of the settings and the current rows, for the settings window.
pub fn settings_and_devices() -> Option<(Settings, Vec<Device>)> {
    with_app(|app| (app.settings.clone(), app.devices.clone()))
}

/// Saves new settings, restarts the poll if its interval changed, and redraws.
pub fn apply_settings(settings: Settings) {
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
    });
}
