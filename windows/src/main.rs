//! Starts Juice Bar. Everything lives in the library; see `app_main::run`.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    juicebar_windows::app_main::run();
}
