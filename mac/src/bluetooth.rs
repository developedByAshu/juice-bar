//! Bluetooth mice and keyboards as macOS knows them: remembered names and, when connected, battery.
//! Read from `system_profiler SPBluetoothDataType -json`, run off the main thread and only on launch,
//! wake, menu open and the poll, so it never blocks the menu.

use juicebar_shared::device::{Battery, Connection, Device, DeviceKind, NameSource};
use serde_json::Value;

/// One paired Bluetooth device.
#[derive(Clone, Debug, PartialEq)]
pub struct BluetoothEntry {
    pub name: String,
    pub kind: DeviceKind,
    pub is_connected: bool,
    pub battery: Option<u8>,
    pub vendor_id: u16,
    pub product_id: u16,
}

/// Returns every paired mouse and keyboard, connected or not.
pub fn read_bluetooth_entries() -> Vec<BluetoothEntry> {
    let Ok(output) = std::process::Command::new("/usr/sbin/system_profiler").args(["SPBluetoothDataType", "-json", "-detailLevel", "basic"]).output() else {
        return Vec::new();
    };
    serde_json::from_slice::<Value>(&output.stdout).map_or_else(|_| Vec::new(), |json| parse_entries(&json))
}

/// Parses system_profiler's JSON into entries. Split out so it can be tested with saved output.
pub fn parse_entries(json: &Value) -> Vec<BluetoothEntry> {
    let mut entries = Vec::new();
    let controllers = json["SPBluetoothDataType"].as_array().cloned().unwrap_or_default();
    for controller in controllers {
        for (key, is_connected) in [("device_connected", true), ("device_not_connected", false)] {
            for group in controller[key].as_array().cloned().unwrap_or_default() {
                let Some(object) = group.as_object() else {
                    continue;
                };
                for (name, details) in object {
                    let kind = match details["device_minorType"].as_str().unwrap_or("") {
                        "Mouse" | "Trackpad" => DeviceKind::Mouse,
                        "Keyboard" => DeviceKind::Keyboard,
                        _ => continue,
                    };
                    let battery = ["device_batteryLevelMain", "device_batteryLevel"]
                        .iter()
                        .find_map(|field| details[*field].as_str())
                        .and_then(|text| text.trim_end_matches('%').trim().parse().ok());
                    let id_of = |field: &str| details[field].as_str().and_then(|text| u16::from_str_radix(text.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
                    let (vendor_id, product_id) = (id_of("device_vendorID"), id_of("device_productID"));
                    entries.push(BluetoothEntry { name: name.trim().to_string(), kind, is_connected, battery, vendor_id, product_id });
                }
            }
        }
    }
    entries
}

/// Returns the remembered names for one kind, for naming a dongle device of that kind.
pub fn remembered_bluetooth_names(entries: &[BluetoothEntry], kind: DeviceKind) -> Vec<String> {
    entries.iter().filter(|entry| entry.kind == kind).map(|entry| entry.name.clone()).collect()
}

/// Returns menu rows for Bluetooth devices connected right now that report a battery.
pub fn bluetooth_devices(entries: &[BluetoothEntry]) -> Vec<Device> {
    entries
        .iter()
        .filter(|entry| entry.is_connected)
        .filter_map(|entry| {
            let percent = entry.battery?;
            // An exact catalog name wins, as for dongles; a brand-shared chip keeps its Bluetooth name.
            let exact = juicebar_shared::catalog::find_by_ids(entry.vendor_id, entry.product_id).filter(|known| !known.brand_shared);
            let (name, name_source) = exact.map_or((entry.name.clone(), NameSource::DeviceReported), |known| (known.family_name.to_string(), NameSource::Catalog));
            Some(Device {
                id: format!("bluetooth:{}", entry.name),
                kind: entry.kind,
                connection: Connection::Bluetooth,
                name,
                name_source,
                battery: Some(Battery { percent, is_charging: false }),
                protocol: Some("bluetooth"),
                vendor_id: entry.vendor_id,
                product_id: entry.product_id,
            })
        })
        .collect()
}
