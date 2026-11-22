//! Scans the plugged-in dongles the way the apps do and prints each row. Sends battery read commands only.
//! Usage: cargo run -p juicebar-shared --example read_batteries

use juicebar_shared::scan::{build_device, list_usb_devices, read_usb_device};
use juicebar_shared::settings::Settings;

fn main() {
    let api = hidapi::HidApi::new().expect("hidapi");
    let settings = Settings { is_saved_to_disk: false, ..Settings::default() };
    for usb in list_usb_devices(&api) {
        let (reading, protocol) = read_usb_device(&api, &usb);
        let device = build_device(&usb, &reading, protocol, &settings, &[]);
        println!("{:<24} {:?} {:?} {:?} via {:?} (reading {reading:?})", device.name, device.kind, device.connection, device.battery, device.protocol);
    }
}
