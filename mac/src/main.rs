//! Starts Juice Bar.

#[cfg(target_os = "macos")]
fn main() {
    juicebar_mac::app::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {}
