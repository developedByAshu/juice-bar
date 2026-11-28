//! The process entry: one copy only, the hidden main window and its message handling.

use crate::app::{with_app, App};
use crate::messages::{WM_APP_KEYBOARD_EVENT, WM_APP_MENU_ACTION, WM_APP_SCAN_DONE, WM_APP_SETTINGS_CHANGED, WM_APP_TRAY, WM_APP_UPDATE};
use crate::status_icon::{redraw_status_icon, remove_status_icon, restore_status_icon};
use crate::theme::notify_theme_change;
use crate::util::to_wide;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage, RegisterClassW,
    RegisterDeviceNotificationW, RegisterWindowMessageW, TranslateMessage, DBT_DEVTYP_DEVICEINTERFACE, DEVICE_NOTIFY_WINDOW_HANDLE,
    DEV_BROADCAST_DEVICEINTERFACE_W, MSG, PBT_APMRESUMEAUTOMATIC, WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_DWMCOLORIZATIONCOLORCHANGED, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_THEMECHANGED,
    WM_TIMER, WM_USER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

const SINGLE_INSTANCE_NAME: &str = "Local\\JuiceBar.SingleInstance";
const WINDOW_CLASS_NAME: &str = "JuiceBarMain";
// The tray's version 4 events: a left click or Enter on the icon.
const NIN_SELECT: u32 = WM_USER;
const NIN_KEYSELECT: u32 = WM_USER + 1;
// The pointer rests on the icon or leaves it; sent only while Windows' own tooltip is off.
const NIN_POPUPOPEN: u32 = WM_USER + 6;
const NIN_POPUPCLOSE: u32 = WM_USER + 7;
// Device arrival and removal, from Dbt.h.
const DBT_DEVICEARRIVAL: u32 = 0x8000;
const DBT_DEVICEREMOVECOMPLETE: u32 = 0x8004;
// Every HID device, so a dongle plugged in or out triggers a read.
const GUID_DEVINTERFACE_HID: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0x4D1E55B2_F16F_11CF_88CB_001111000030);

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn quit(window: HWND) {
    with_app(|app| app.close_windows());
    // SAFETY: our own main window.
    unsafe { DestroyWindow(window) };
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_APP_TRAY => {
            let event = (lparam & 0xFFFF) as u32;
            match event {
                NIN_SELECT | NIN_KEYSELECT => with_app(|app| app.on_icon_click()),
                // A right click always opens the menu, as in NetSpeedGauge.
                WM_CONTEXTMENU => with_app(|app| app.toggle_menu()),
                NIN_POPUPOPEN => with_app(|app| app.on_icon_hover(true)),
                NIN_POPUPCLOSE => with_app(|app| app.on_icon_hover(false)),
                _ => None,
            };
            0
        }
        WM_APP_SCAN_DONE => {
            with_app(App::on_scan_done);
            0
        }
        WM_APP_KEYBOARD_EVENT => {
            with_app(App::on_keyboard_events);
            0
        }
        WM_APP_MENU_ACTION => {
            if with_app(App::on_menu_action) == Some(false) {
                quit(window);
            }
            0
        }
        WM_APP_SETTINGS_CHANGED => {
            with_app(App::on_settings_changed);
            0
        }
        WM_APP_UPDATE => {
            // An installed update has started the new copy, which waits for this one to quit.
            if with_app(App::on_update) == Some(false) {
                quit(window);
            }
            0
        }
        WM_TIMER => {
            with_app(|app| app.on_timer(wparam));
            0
        }
        WM_POWERBROADCAST => {
            if wparam as u32 == PBT_APMRESUMEAUTOMATIC {
                with_app(App::on_wake);
            }
            1
        }
        WM_DEVICECHANGE => {
            if matches!(wparam as u32, DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) {
                with_app(App::on_device_change);
            }
            1
        }
        WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
            notify_theme_change();
            redraw_status_icon();
            DefWindowProcW(window, message, wparam, lparam)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            redraw_status_icon();
            DefWindowProcW(window, message, wparam, lparam)
        }
        WM_DESTROY => {
            remove_status_icon();
            PostQuitMessage(0);
            0
        }
        _ if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            restore_status_icon();
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

/// Asks Windows to tell the window whenever a HID device comes or goes.
fn listen_for_devices(window: HWND) {
    // SAFETY: an all-zero filter is valid; the fields set describe the HID interface class.
    unsafe {
        let mut filter: DEV_BROADCAST_DEVICEINTERFACE_W = std::mem::zeroed();
        filter.dbcc_size = std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32;
        filter.dbcc_devicetype = DBT_DEVTYP_DEVICEINTERFACE;
        filter.dbcc_classguid = GUID_DEVINTERFACE_HID;
        RegisterDeviceNotificationW(window, std::ptr::from_ref(&filter).cast(), DEVICE_NOTIFY_WINDOW_HANDLE);
    }
}

/**
 * Opens a window at start, for screenshots, in debug builds only: `--open-menu` the flyout once the
 * first read is in, `--open-settings[=general|appearance|devices|advanced|about|reset]` a page.
 */
#[cfg(debug_assertions)]
fn open_for_screenshots() {
    if let Some(argument) = std::env::args().find(|argument| argument.starts_with("--open-settings")) {
        use crate::settings_ui::{open_settings_on, show_reset_confirm, Page};
        let choice = argument.split_once('=').map_or("general", |(_, value)| value);
        let page = match choice {
            "appearance" => Page::Appearance,
            "devices" => Page::Devices,
            "advanced" | "reset" => Page::Advanced,
            "about" => Page::About,
            _ => Page::General,
        };
        with_app(|app| open_settings_on(app.window, app.settings.clone(), app.devices.clone(), page));
        if choice == "reset" {
            show_reset_confirm();
        }
    }
}

/// Runs the app until Quit. A debug build takes `--open-menu` and `--open-settings`, for screenshots.
pub fn run() {
    let instance_name = to_wide(SINGLE_INSTANCE_NAME);
    let class_name = to_wide(WINDOW_CLASS_NAME);
    let title = to_wide("Juice Bar");
    let taskbar_created = to_wide("TaskbarCreated");
    // After an update, wait for the old copy to quit first, so the one-copy check below lets this one in.
    crate::updater::finish_update();
    // SAFETY: Win32 setup on the main thread. All strings outlive the calls that read them.
    unsafe {
        let _instance_guard = CreateMutexW(std::ptr::null(), 0, instance_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        TASKBAR_CREATED.store(RegisterWindowMessageW(taskbar_created.as_ptr()), Ordering::Relaxed);
        let module = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: module,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class);
        // A hidden top-level window, not a message-only one: only top-level windows hear theme changes.
        let window = CreateWindowExW(WS_EX_TOOLWINDOW, class_name.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), module, std::ptr::null());
        listen_for_devices(window);
        if cfg!(debug_assertions) && std::env::args().any(|argument| argument == "--open-menu") {
            crate::app::OPEN_MENU_AFTER_SCAN.store(true, Ordering::Relaxed);
        }
        App::start(window);
        #[cfg(debug_assertions)]
        open_for_screenshots();
        // Release memory only startup needed. Anything still in use comes straight back.
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
