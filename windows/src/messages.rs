//! The private messages the app's windows send each other.

use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

/// The tray icon was clicked, hovered or opened by keyboard.
pub const WM_APP_TRAY: u32 = WM_APP + 1;
/// A background scan finished; its devices wait in the app.
pub const WM_APP_SCAN_DONE: u32 = WM_APP + 2;
/// The flyout chose an action; it waits in the flyout.
pub const WM_APP_MENU_ACTION: u32 = WM_APP + 3;
/// The settings window changed a choice that the icon or the menu shows.
pub const WM_APP_SETTINGS_CHANGED: u32 = WM_APP + 4;
