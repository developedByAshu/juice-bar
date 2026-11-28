//! The Mac app: a menu bar battery icon and a native menu. Everything else lives in `juicebar_shared`.

#[cfg(target_os = "macos")]
pub mod app;
#[cfg(target_os = "macos")]
pub mod ble_battery;
#[cfg(target_os = "macos")]
pub mod bluetooth;
#[cfg(target_os = "macos")]
pub mod canvas;
#[cfg(target_os = "macos")]
pub mod delegate;
#[cfg(target_os = "macos")]
pub mod menu_ui;
#[cfg(target_os = "macos")]
pub mod permissions;
#[cfg(target_os = "macos")]
pub mod settings_ui;
pub mod startup;
#[cfg(target_os = "macos")]
pub mod status_icon;
#[cfg(target_os = "macos")]
pub mod surface;
#[cfg(target_os = "macos")]
pub mod theme;
#[cfg(target_os = "macos")]
pub mod updater;
