//! Prints what one dongle scan finds: each USB device, its reading and the row it would show.
//! Usage: cargo run -p juicebar-mac --example scan_dump

#[cfg(target_os = "macos")]
fn main() {
    use juicebar_shared::scan::{list_usb_devices, read_usb_device};
    use juicebar_shared::settings::Settings;

    let api = hidapi::HidApi::new().expect("hidapi");
    println!("input monitoring allowed: {}", juicebar_mac::permissions::has_input_monitoring());
    for device in list_usb_devices(&api) {
        let (reading, protocol) = read_usb_device(&api, &device);
        let row = juicebar_shared::scan::build_device(&device, &reading, protocol, &Settings::default(), &[]);
        println!("{:04x}:{:04x} {:?} protocol={protocol:?} reading={reading:?} -> {:?} battery={:?}", device.vendor_id, device.product_id, device.kind(), row.name, row.battery);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
