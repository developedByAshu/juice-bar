//! Listens (shared, read only) to the bytech keyboard dongle's command channel (page 0xFF02) and prints
//! every pushed report with a clock time, to find the packet the dongle sends when the keyboard wakes.
//! Usage: cargo run -p juicebar-mac --example keyboard_listen -- <minutes>

#[cfg(target_os = "macos")]
fn main() {
    let minutes: u64 = std::env::args()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or(15);
    let api = hidapi::HidApi::new().expect("hidapi");
    api.set_open_exclusive(false);
    let Some(info) = api.device_list().find(|info| {
        info.vendor_id() == 0x3554 && info.product_id() == 0xFA09 && info.usage_page() == 0xFF02
    }) else {
        println!("keyboard dongle command channel not found");
        return;
    };
    let device = info.open_device(&api).expect("open");
    let start = std::time::Instant::now();
    let mut buffer = [0u8; 64];
    println!("listening {minutes} min");
    while start.elapsed().as_secs() < minutes * 60 {
        if let Ok(size) = device.read_timeout(&mut buffer, 1000) {
            if size > 0 {
                let bytes: Vec<String> = buffer[..size]
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                let clock = std::process::Command::new("date")
                    .arg("+%H:%M:%S")
                    .output()
                    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                    .unwrap_or_default();
                println!("{clock} {}", bytes.join(" "));
            }
        }
    }
    println!("done");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
