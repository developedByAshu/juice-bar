//! Sends the bytech read commands GET_ONLINE and GET_POWER and prints every raw reply.
//! Usage: cargo run -p juicebar-shared --example bytech_debug

use juicebar_shared::protocols::bytech::build_request;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let api = hidapi::HidApi::new().expect("hidapi");
    let info = api.device_list().find(|info| info.vendor_id() == 0x3554 && info.product_id() == 0xFA09 && info.usage_page() == 0xFF02).expect("keyboard dongle");
    let device = api.open_path(info.path()).expect("open");
    for command in [0x07u8, 0x4A, 0x05] {
        let request = build_request(command);
        println!("OUT {}", hex(&request));
        if let Err(error) = device.write(&request) {
            println!("  write error {error}");
        }
        for _ in 0..8 {
            let mut buffer = [0u8; 64];
            match device.read_timeout(&mut buffer, 100) {
                Ok(0) => {}
                Ok(length) => println!("  IN  {}", hex(&buffer[..length])),
                Err(error) => println!("  read error {error}"),
            }
        }
    }
}
