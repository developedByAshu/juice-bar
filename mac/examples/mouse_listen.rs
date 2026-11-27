//! Listens to the yuandaxin mouse dongle's vendor input (page 0x0A, report 4) for 12 s and prints each report.
//! Usage: cargo run -p juicebar-mac --example mouse_listen

#[cfg(target_os = "macos")]
fn main() {
    let api = hidapi::HidApi::new().expect("hidapi");
    // Shared, as browsers open it: macOS refuses an exclusive open of a mouse to a normal app.
    api.set_open_exclusive(false);
    let Some(info) = api.device_list().find(|info| info.vendor_id() == 0x04F3 && info.product_id() == 0x026F && info.usage_page() == 0x000A && info.usage() == 0) else {
        println!("mouse dongle input interface not found");
        return;
    };
    let device = info.open_device(&api).expect("open");
    let start = std::time::Instant::now();
    let mut buffer = [0u8; 64];
    while start.elapsed().as_secs() < 12 {
        if let Ok(size) = device.read_timeout(&mut buffer, 1000) {
            if size > 0 {
                let bytes: Vec<String> = buffer[..size].iter().map(|byte| format!("{byte:02x}")).collect();
                println!("{:>5.1}s {}", start.elapsed().as_secs_f32(), bytes.join(" "));
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
