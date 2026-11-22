//! Batteries Windows reads itself from paired Bluetooth mice and keyboards, and the names it
//! remembers for them, which can name the same device when it is on its 2.4 GHz dongle.

use juicebar_shared::device::{Device, DeviceKind};

/// Returns the paired Bluetooth mice and keyboards that report a battery to Windows.
pub fn bluetooth_devices() -> Vec<Device> {
    Vec::new()
}

/// Returns the names Windows remembers for paired Bluetooth devices of a kind.
pub fn remembered_bluetooth_names(kind: DeviceKind) -> Vec<String> {
    let _ = kind;
    Vec::new()
}
