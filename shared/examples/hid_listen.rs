//! Lists a device's HID interfaces, reads the given feature reports and listens to input reports.
//! Read only: it never sends an output or feature report.
//! Usage: cargo run -p juicebar-shared --example hid_listen -- <vid hex> <pid hex> <seconds> [feature ids...]

use std::time::{Duration, Instant};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parse_hex = |text: &str| u16::from_str_radix(text.trim_start_matches("0x"), 16).expect("hex id");
    let (vendor_id, product_id) = (parse_hex(&args[0]), parse_hex(&args[1]));
    let seconds: u64 = args.get(2).and_then(|text| text.parse().ok()).unwrap_or(10);
    let feature_ids: Vec<u8> = args.iter().skip(3).filter_map(|text| text.parse().ok()).collect();
    let api = hidapi::HidApi::new().expect("hidapi");
    let mut opened = Vec::new();
    for info in api.device_list().filter(|info| info.vendor_id() == vendor_id && info.product_id() == product_id) {
        let page = info.usage_page();
        println!("interface page=0x{page:04x} usage=0x{:04x} product={:?}", info.usage(), info.product_string());
        // Keyboard and mouse pages need Input Monitoring and carry no battery, so only vendor pages are opened.
        if page == 0x01 || page == 0x0C {
            continue;
        }
        match api.open_path(info.path()) {
            Ok(device) => {
                for &report_id in &feature_ids {
                    let mut buffer = [0u8; 65];
                    buffer[0] = report_id;
                    match device.get_feature_report(&mut buffer) {
                        Ok(length) => println!("  GF page=0x{page:04x} id={report_id}: {}", hex(&buffer[..length])),
                        Err(error) => println!("  GF page=0x{page:04x} id={report_id}: error {error}"),
                    }
                }
                opened.push((page, device));
            }
            Err(error) => println!("  open failed: {error}"),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    println!("listening {seconds}s on {} interfaces", opened.len());
    while Instant::now() < deadline {
        for (page, device) in &opened {
            let mut buffer = [0u8; 65];
            if let Ok(length) = device.read_timeout(&mut buffer, 20) {
                if length > 0 {
                    println!("  IN page=0x{page:04x}: {}", hex(&buffer[..length]));
                }
            }
        }
    }
}
