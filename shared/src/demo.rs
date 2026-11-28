//! Sample devices for building and screenshotting the UI on a machine without the dongles.
//! Start a debug build of either app with `JUICEBAR_DEMO=1` to show these instead of real devices.
//! A release build has no demo mode.

use crate::device::{Battery, Connection, Device, DeviceKind, NameSource};

fn sample(id: &str, kind: DeviceKind, connection: Connection, name: &str, battery: Option<(u8, bool)>, protocol: Option<&'static str>) -> Device {
    Device {
        id: id.to_string(),
        kind,
        connection,
        name: name.to_string(),
        name_source: if protocol.is_some() { NameSource::Catalog } else { NameSource::Unknown },
        battery: battery.map(|(percent, is_charging)| Battery { percent, is_charging }),
        protocol,
        vendor_id: 0,
        product_id: 0,
    }
}

/// Returns the author's two devices plus one unknown mouse, at the given %.
pub fn sample_devices(keyboard_percent: u8, mouse_percent: u8) -> Vec<Device> {
    vec![
        sample("demo:keyboard", DeviceKind::Keyboard, Connection::Dongle, "Kreo Swarm 75", Some((keyboard_percent, false)), Some("bytech")),
        sample("demo:mouse", DeviceKind::Mouse, Connection::Dongle, "XS Flow+1", Some((mouse_percent, true)), Some("yuandaxin")),
        sample("demo:unknown", DeviceKind::Mouse, Connection::Dongle, "Unknown mouse", None, None),
    ]
}

/// Checks whether the app was started in demo mode. Only debug builds read the switch.
#[cfg(debug_assertions)]
pub fn is_demo_mode() -> bool {
    std::env::var_os("JUICEBAR_DEMO").is_some()
}

/// A release build is never in demo mode, whatever the environment says.
#[cfg(not(debug_assertions))]
pub fn is_demo_mode() -> bool {
    false
}
