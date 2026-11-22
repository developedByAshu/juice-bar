//! Prints what Juice Bar reads from Windows' Bluetooth list: connected mice and keyboards with
//! their battery, and the remembered names per kind. Usage: cargo run -p juicebar-windows --example list_bluetooth

#[cfg(windows)]
fn main() {
    use juicebar_windows::bluetooth::{bluetooth_devices, remembered_names_by_kind};
    println!("Connected, with battery:");
    for device in bluetooth_devices() {
        println!("  {:?} {} ({}) battery {:?} ids {:04x}:{:04x}", device.kind, device.name, device.id, device.battery.map(|battery| battery.percent), device.vendor_id, device.product_id);
    }
    println!("Remembered names:");
    for (kind, name) in remembered_names_by_kind() {
        println!("  {kind:?} {name}");
    }
}

#[cfg(not(windows))]
fn main() {}
