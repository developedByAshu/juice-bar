//! The app's state: the devices, the choices, and when to read the batteries again.
//!
//! Reads happen on launch, on wake from sleep, when a USB device comes or goes, when a keyboard
//! wakes, and every poll interval; nothing runs between them. Between jobs the read thread waits on
//! each keyboard dongle's command channel for the wake, sleep and battery events it pushes. A read talks to the dongles, which can take a moment, so
//! it runs on one long-lived read thread that owns the only HidApi and posts results back to the UI thread.

use crate::bluetooth::{bluetooth_devices, remembered_names_by_kind};
use crate::menu_ui::{close_hover_card, close_menu, is_menu_open, refresh_menu, set_menu_text_points, show_card, show_menu, take_menu_action, MenuAction};
use crate::messages::{WM_APP_KEYBOARD_EVENT, WM_APP_SCAN_DONE};
use crate::settings_ui::{close_settings, open_settings, refresh_settings};
use crate::status_icon::{create_status_icon, set_tooltip, update_status_icon, use_hover_card};
use crate::util::to_wide;
use juicebar_shared::demo::{is_demo_mode, sample_devices};
use juicebar_shared::device::{Battery, Device, NameSource};
use juicebar_shared::protocols::bytech::{parse_event, KeyboardEvent};
use juicebar_shared::report::{report_url, InterfaceSummary};
use juicebar_shared::scan::{build_device, probe_usb_device, scan_dongles, ScannedDevice};
use juicebar_shared::settings::{ClickAction, HoverAction, Settings};
use juicebar_shared::tone::{icon_tone, lowest_percent};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, MessageBoxW, PostMessageW, SetCoalescableTimer, MB_ICONINFORMATION, MB_OK, SW_SHOWNORMAL};

/// Timer ids on the main window.
pub const POLL_TIMER: usize = 1;
pub const SOON_TIMER: usize = 2;
// The poll may run up to this late, so Windows can line it up with other wakeups and save power.
const POLL_TOLERANCE_MS: u32 = 5_000;
// A dongle needs a moment after wake or plug-in before it answers.
const SETTLE_MS: u32 = 2_500;
const TOOLTIP_LIMIT: usize = 127;

/// One finished read, handed from the read thread to the UI thread.
struct ScanResult {
    devices: Vec<Device>,
    interfaces: BTreeMap<String, Vec<InterfaceSummary>>,
    /// The id of the device a Try to detect ran on, and whether it found a protocol.
    probe: Option<(String, bool)>,
}

static SCAN_RESULT: Mutex<Option<ScanResult>> = Mutex::new(None);
/// Keyboard events from the read thread, oldest first, with the dongle's vendor and product id.
static KEYBOARD_EVENTS: Mutex<Vec<(u16, u16, KeyboardEvent)>> = Mutex::new(Vec::new());

/// How long one pass waits on the held keyboard channels before checking for new work, in total.
const WATCH_WAIT_MS: i32 = 500;

/// A keyboard dongle's command channel, held open to hear what it pushes.
struct KeyboardWatch {
    vendor_id: u16,
    product_id: u16,
    device: hidapi::HidDevice,
}

/// Opens (shared) the command channel of every known bytech keyboard dongle that is plugged in.
fn open_keyboard_watches(api: &hidapi::HidApi) -> Vec<KeyboardWatch> {
    api.device_list()
        .filter(|info| juicebar_shared::catalog::find_known(info.vendor_id(), info.product_id(), info.usage_page()).is_some_and(|known| known.protocol == "bytech"))
        .filter_map(|info| info.open_device(api).ok().map(|device| KeyboardWatch { vendor_id: info.vendor_id(), product_id: info.product_id(), device }))
        .collect()
}

/// Waits briefly on the held channels and hands each pushed event to the UI thread. A channel that
/// errors (dongle unplugged) is dropped; the next read opens it again.
fn wait_for_keyboard_events(watches: &mut Vec<KeyboardWatch>, window: isize) {
    let wait = WATCH_WAIT_MS / watches.len().max(1) as i32;
    watches.retain(|watch| {
        let mut buffer = [0u8; 64];
        match watch.device.read_timeout(&mut buffer, wait) {
            Ok(size) => {
                if let Some(event) = parse_event(&buffer[..size]) {
                    KEYBOARD_EVENTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((watch.vendor_id, watch.product_id, event));
                    // SAFETY: posting to the app's own window, which outlives the read thread's work.
                    unsafe { PostMessageW(window as HWND, WM_APP_KEYBOARD_EVENT, 0, 0) };
                }
                true
            }
            Err(_) => false,
        }
    });
}

/// Set by `--open-menu`: open the flyout once the first read is in, for screenshots.
pub static OPEN_MENU_AFTER_SCAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The app state, owned by the UI thread.
pub struct App {
    pub window: HWND,
    pub settings: Rc<RefCell<Settings>>,
    /// Every device seen, hidden ones too, so Settings can show them again.
    pub devices: Rc<RefCell<Vec<Device>>>,
    interfaces: BTreeMap<String, Vec<InterfaceSummary>>,
    /// Protocols found by Try to detect, kept for this run: device id to protocol id.
    probed: Rc<RefCell<BTreeMap<String, &'static str>>>,
    is_scanning: bool,
    is_scan_wanted: bool,
    /// When each device last gave a %, so one missed read does not blank a good one.
    last_read: HashMap<String, Instant>,
    /// Keyboards whose dongle said they fell asleep; these show as asleep at once, without the grace.
    asleep: Vec<(u16, u16)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs an action on the app, if it has started.
pub fn with_app<T>(action: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|cell| cell.borrow_mut().as_mut().map(action))
}

/// Returns demo devices for `JUICEBAR_DEMO=1`. `JUICEBAR_DEMO_PERCENTS=18,60` picks the keyboard and mouse %.
fn demo_devices() -> Vec<Device> {
    let percents: Vec<u8> = std::env::var("JUICEBAR_DEMO_PERCENTS").unwrap_or_default().split(',').filter_map(|part| part.trim().parse().ok()).collect();
    sample_devices(percents.first().copied().unwrap_or(82), percents.get(1).copied().unwrap_or(15))
}

/// One read the UI thread asks the read thread for.
struct ScanRequest {
    settings: Settings,
    probed: BTreeMap<String, &'static str>,
    /// The device a Try to detect runs on.
    probe_id: Option<String>,
    window: isize,
}

/// Reads every device once. Runs on the read thread, which owns the one HidApi.
/// Devices come back under their own names; renames are applied only where a name is shown.
fn scan(api: &mut Option<hidapi::HidApi>, request: &ScanRequest) -> ScanResult {
    let mut own_names = request.settings.clone();
    own_names.renames.clear();
    let (settings, probe_id) = (&own_names, request.probe_id.as_deref());
    if is_demo_mode() {
        let devices = demo_devices();
        return ScanResult { devices, interfaces: BTreeMap::new(), probe: probe_id.map(|id| (id.to_string(), false)) };
    }
    let mut devices = Vec::new();
    let mut interfaces = BTreeMap::new();
    let mut probe = None;
    if api.is_none() {
        *api = hidapi::HidApi::new().ok();
    } else if let Some(api) = api.as_mut() {
        let _ = api.refresh_devices();
    }
    if let Some(api) = api.as_ref() {
        // One pass over Windows' device list per read, not one per dongle.
        let remembered = remembered_names_by_kind();
        let names_for = |kind| remembered.iter().filter(|(of_kind, _)| *of_kind == kind).map(|(_, name)| name.clone()).collect::<Vec<_>>();
        for ScannedDevice { usb, mut device } in scan_dongles(api, settings, &names_for) {
            let id = usb.id();
            // Try to detect tries every protocol once; a device it found one for is probed again on later reads.
            let wants_probe = probe_id == Some(id.as_str()) || (device.protocol.is_none() && request.probed.contains_key(&id));
            if wants_probe {
                let (reading, protocol) = probe_usb_device(api, &usb);
                if protocol.is_some() {
                    device = build_device(&usb, &reading, protocol, settings, &names_for(usb.kind()));
                    if device.name_source == NameSource::Unknown {
                        device.name_source = NameSource::Catalog;
                    }
                }
                if probe_id == Some(id.as_str()) {
                    probe = Some((id.clone(), protocol.is_some()));
                }
            }
            interfaces.insert(id, usb.interfaces.iter().map(|(summary, _)| summary.clone()).collect());
            devices.push(device);
        }
    }
    devices.extend(bluetooth_devices());
    ScanResult { devices, interfaces, probe }
}

/// Returns the read thread's inbox, starting the thread on first use. It lives as long as the app.
fn scan_sender() -> &'static Mutex<std::sync::mpsc::Sender<ScanRequest>> {
    static SENDER: std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<ScanRequest>>> = std::sync::OnceLock::new();
    SENDER.get_or_init(|| {
        let (sender, inbox) = std::sync::mpsc::channel::<ScanRequest>();
        std::thread::spawn(move || {
            // One HidApi for the whole run: hidapi is not safe to create on several threads.
            let mut api = None;
            let mut watches: Vec<KeyboardWatch> = Vec::new();
            let mut window = 0isize;
            loop {
                // With no channel to watch, sleep until work comes; otherwise check for work between waits.
                let request = if watches.is_empty() {
                    match inbox.recv() {
                        Ok(request) => Some(request),
                        Err(_) => break,
                    }
                } else {
                    match inbox.try_recv() {
                        Ok(request) => Some(request),
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                };
                if let Some(request) = request {
                    window = request.window;
                    watches.clear();
                    let result = scan(&mut api, &request);
                    if !is_demo_mode() {
                        watches = api.as_ref().map_or_else(Vec::new, open_keyboard_watches);
                    }
                    *SCAN_RESULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
                    // SAFETY: posting to the app's own window, which outlives every read.
                    unsafe { PostMessageW(request.window as HWND, WM_APP_SCAN_DONE, 0, 0) };
                    continue;
                }
                wait_for_keyboard_events(&mut watches, window);
            }
        });
        Mutex::new(sender)
    })
}

/// Returns the tooltip: the app name, then each device and its %.
pub fn tooltip_text(devices: &[Device]) -> String {
    let mut text = String::from("Juice Bar");
    for device in devices {
        let reading = match device.battery {
            Some(battery) if battery.is_charging => format!("{}% charging", battery.percent),
            Some(battery) => format!("{}%", battery.percent),
            None if device.is_unrecognised() => "unknown".to_string(),
            None => "not connected".to_string(),
        };
        let line = format!("\n{}: {reading}", device.name);
        if text.chars().count() + line.chars().count() > TOOLTIP_LIMIT {
            break;
        }
        text.push_str(&line);
    }
    text
}

/// How long a device that just missed one read keeps its last %. A mouse that sleeps between reports
/// or a keyboard that answers late must not blank a % that was fine a minute ago.
pub const KEEP_READING_FOR: Duration = Duration::from_secs(30 * 60);

/// Fills a missed reading with the device's last %, unless its dongle said it is asleep or the last
/// reading is older than KEEP_READING_FOR. Records when each device last answered. Mirrors the Mac.
pub fn keep_recent_readings(new: &mut [Device], old: &[Device], last_read: &mut HashMap<String, Instant>, asleep: &[(u16, u16)], now: Instant) {
    for device in new.iter_mut() {
        if device.battery.is_some() {
            last_read.insert(device.id.clone(), now);
            continue;
        }
        let is_asleep = asleep.contains(&(device.vendor_id, device.product_id));
        let is_recent = last_read.get(&device.id).is_some_and(|read| now.duration_since(*read) < KEEP_READING_FOR);
        if !is_asleep && is_recent {
            device.battery = old.iter().find(|previous| previous.id == device.id).and_then(|previous| previous.battery);
        }
    }
}

/// Returns the battery after a keyboard event. A pushed battery event carries only the %, so charging
/// is kept from the last full reading; a sleep event (None) clears it. Mirrors the Mac.
pub fn pushed_battery(pushed: Option<Battery>, last: Option<Battery>) -> Option<Battery> {
    pushed.map(|battery| Battery { percent: battery.percent, is_charging: last.is_some_and(|last| last.is_charging) })
}

/// Returns the devices under the names in use: the user's rename first, else the device's own name.
pub fn with_renames(devices: &[Device], settings: &Settings) -> Vec<Device> {
    devices
        .iter()
        .cloned()
        .map(|mut device| {
            if let Some(name) = settings.renames.get(&device.id).filter(|name| !name.trim().is_empty()) {
                device.name = name.clone();
                device.name_source = NameSource::UserRename;
            }
            device
        })
        .collect()
}

/// Opens a web address in the user's browser.
pub fn open_url(url: &str) {
    let (verb, target) = (to_wide("open"), to_wide(url));
    // SAFETY: both strings are null-terminated and outlive the call.
    unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}

/// Returns "Windows 11 (build 26100)" for the Report text.
fn os_label() -> String {
    let build = crate::theme::windows_build();
    let name = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("{name} (build {build})")
}

impl App {
    /// Builds the app on its main window, puts the icon in the tray and reads the batteries.
    pub fn start(window: HWND) {
        let app = App {
            window,
            settings: Rc::new(RefCell::new(Settings::load())),
            devices: Rc::new(RefCell::new(Vec::new())),
            interfaces: BTreeMap::new(),
            probed: Rc::new(RefCell::new(BTreeMap::new())),
            is_scanning: false,
            is_scan_wanted: false,
            last_read: HashMap::new(),
            asleep: Vec::new(),
        };
        create_status_icon(window);
        crate::menu_ui::set_menu_owner(window);
        use_hover_card(app.settings.borrow().hover_action == HoverAction::Card);
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        with_app(|app| {
            app.restart_poll_timer();
            app.refresh();
        });
    }

    /// Starts a read now, unless one is running; then it runs again right after.
    pub fn refresh(&mut self) {
        self.start_scan(None);
    }

    fn start_scan(&mut self, probe_id: Option<String>) {
        if self.is_scanning {
            self.is_scan_wanted = true;
            return;
        }
        self.is_scanning = true;
        let request = ScanRequest { settings: self.settings.borrow().clone(), probed: self.probed.borrow().clone(), probe_id, window: self.window as isize };
        let _ = scan_sender().lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(request);
    }

    /// Takes a finished read and shows it everywhere.
    pub fn on_scan_done(&mut self) {
        self.is_scanning = false;
        let Some(result) = SCAN_RESULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() else {
            return;
        };
        let mut devices = result.devices;
        keep_recent_readings(&mut devices, &self.devices.borrow(), &mut self.last_read, &self.asleep, Instant::now());
        *self.devices.borrow_mut() = devices;
        self.interfaces = result.interfaces;
        if let Some((id, found)) = &result.probe {
            if *found {
                if let Some(protocol) = self.devices.borrow().iter().find(|device| &device.id == id).and_then(|device| device.protocol) {
                    self.probed.borrow_mut().insert(id.clone(), protocol);
                }
            }
        }
        self.show_devices();
        if let Some((_, found)) = result.probe {
            if !found {
                self.tell_probe_failed();
            }
        }
        if OPEN_MENU_AFTER_SCAN.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.on_icon_click();
        }
        if std::mem::take(&mut self.is_scan_wanted) {
            self.refresh();
        }
    }

    /// Acts on keyboard events at once: a wake reads the battery now, sleep clears it, a push shows it.
    pub fn on_keyboard_events(&mut self) {
        let events = std::mem::take(&mut *KEYBOARD_EVENTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let mut changed = false;
        for (vendor_id, product_id, event) in events {
            self.asleep.retain(|ids| *ids != (vendor_id, product_id));
            if event == KeyboardEvent::Asleep {
                self.asleep.push((vendor_id, product_id));
            }
            if event == KeyboardEvent::Awake {
                self.refresh();
                continue;
            }
            let battery = match event {
                KeyboardEvent::Battery(battery) => Some(battery),
                _ => None,
            };
            for device in self.devices.borrow_mut().iter_mut().filter(|device| device.vendor_id == vendor_id && device.product_id == product_id) {
                device.battery = pushed_battery(battery, device.battery);
                if battery.is_some() {
                    self.last_read.insert(device.id.clone(), Instant::now());
                }
                changed = true;
            }
        }
        if changed {
            self.show_devices();
        }
    }

    /// Updates the icon, tooltip, open flyout and open settings from the current devices.
    pub fn show_devices(&self) {
        let visible = self.visible_devices();
        let shows_percent = self.settings.borrow().shows_percent_in_icon;
        update_status_icon(icon_tone(&visible), lowest_percent(&visible), shows_percent);
        set_tooltip(&tooltip_text(&visible));
        if is_menu_open() {
            set_menu_text_points(self.settings.borrow().menu_font_points);
            refresh_menu(&visible);
        }
        refresh_settings();
    }

    /// Returns the devices the menu shows: all but the hidden ones, under the names in use.
    pub fn visible_devices(&self) -> Vec<Device> {
        let settings = self.settings.borrow();
        let shown: Vec<Device> = self.devices.borrow().iter().filter(|device| !settings.hidden.contains(&device.id)).cloned().collect();
        with_renames(&shown, &settings)
    }

    /// Reads again shortly after the computer wakes, once the dongles are back.
    pub fn on_wake(&mut self) {
        self.read_soon();
    }

    /// Reads again shortly after a USB device comes or goes; a burst of changes gives one read.
    pub fn on_device_change(&mut self) {
        self.read_soon();
    }

    fn read_soon(&self) {
        // SAFETY: a one-shot timer on our own window; setting it again only pushes it back.
        unsafe { SetCoalescableTimer(self.window, SOON_TIMER, SETTLE_MS, None, 500) };
    }

    /// Handles the app's timers.
    pub fn on_timer(&mut self, timer: usize) {
        if timer == SOON_TIMER {
            // SAFETY: stopping our own one-shot timer.
            unsafe { KillTimer(self.window, SOON_TIMER) };
        }
        self.refresh();
    }

    /// Starts the poll at the chosen interval, replacing any running one.
    pub fn restart_poll_timer(&self) {
        let minutes = self.settings.borrow().poll_minutes.clamp(1, 60);
        // SAFETY: a repeating timer on our own window.
        unsafe { SetCoalescableTimer(self.window, POLL_TIMER, minutes * 60_000, None, POLL_TOLERANCE_MS) };
    }

    /// Applies choices the settings window just changed.
    pub fn on_settings_changed(&mut self) {
        use_hover_card(self.settings.borrow().hover_action == HoverAction::Card);
        self.restart_poll_timer();
        // Renames and confirmed names are applied by the next read; demo and real alike.
        self.refresh();
        self.show_devices();
    }

    fn tell_probe_failed(&self) {
        let title = to_wide("Juice Bar");
        let text = to_wide("None of the protocols Juice Bar knows answered this device.\n\nUse Report so it can be added in a later version.");
        // SAFETY: both strings outlive the call.
        unsafe { MessageBoxW(self.window, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION) };
    }

    /// Opens the prefilled GitHub issue for a device.
    pub fn report(&self, id: &str) {
        let devices = self.devices.borrow();
        let Some(device) = devices.iter().find(|device| device.id == id) else { return };
        let interfaces = self.interfaces.get(id).cloned().unwrap_or_default();
        open_url(&report_url(device, &interfaces, &os_label(), juicebar_shared::APP_VERSION));
    }

    /// Runs what the flyout asked for.
    pub fn on_menu_action(&mut self) -> bool {
        let Some(action) = take_menu_action() else { return true };
        match action {
            MenuAction::OpenSettings => open_settings(self.window, self.settings.clone(), self.devices.clone()),
            MenuAction::Quit => return false,
            MenuAction::Report(id) => self.report(&id),
            MenuAction::Detect(id) => {
                if self.is_scanning {
                    self.is_scan_wanted = false;
                }
                self.start_scan(Some(id));
            }
        }
        true
    }

    /// Runs the chosen On click action for a left click or Enter on the icon.
    pub fn on_icon_click(&self) {
        let action = self.settings.borrow().click_action;
        match action {
            ClickAction::Devices => {
                set_menu_text_points(self.settings.borrow().menu_font_points);
                show_card(&self.visible_devices(), false);
            }
            ClickAction::Settings => {
                close_menu();
                open_settings(self.window, self.settings.clone(), self.devices.clone());
            }
        }
    }

    /// Shows or hides the battery card as the pointer rests on or leaves the icon, when chosen.
    pub fn on_icon_hover(&self, is_over: bool) {
        if self.settings.borrow().hover_action != HoverAction::Card {
            return;
        }
        if is_over {
            set_menu_text_points(self.settings.borrow().menu_font_points);
            show_card(&self.visible_devices(), true);
        } else {
            close_hover_card();
        }
    }

    /// Opens or closes the right-click menu: Settings and Quit.
    pub fn toggle_menu(&self) {
        set_menu_text_points(self.settings.borrow().menu_font_points);
        show_menu();
    }

    /// Closes every window, for quit.
    pub fn close_windows(&self) {
        close_menu();
        close_settings();
    }
}
