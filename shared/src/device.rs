//! What Juice Bar knows about one mouse or keyboard.

/// The kind of device, used for "Unknown mouse" and for matching a remembered Bluetooth name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    Mouse,
    Keyboard,
    Other,
}

/// How the device reaches the computer right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Connection {
    /// A 2.4 GHz USB dongle.
    Dongle,
    Bluetooth,
    /// A cable, so the device is charging and its % does not count for the icon.
    Usb,
}

/// One battery reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// 0 to 100.
    pub percent: u8,
    pub is_charging: bool,
}

/// Where the shown name came from, best first. A name the user typed beats all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameSource {
    UserRename,
    DeviceReported,
    BluetoothMemory,
    Catalog,
    UsbProduct,
    Unknown,
}

/// The state of one device, as the menu shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// Stable id across restarts, used for renames and hiding. For a dongle: "vid:pid:serial".
    pub id: String,
    pub kind: DeviceKind,
    pub connection: Connection,
    pub name: String,
    pub name_source: NameSource,
    /// None while the device sleeps, is switched off, or speaks a protocol nobody knows yet.
    pub battery: Option<Battery>,
    /// The protocol family that answered, for the Report text. None when nothing answered.
    pub protocol: Option<&'static str>,
    /// USB ids, for the Report text and the catalog.
    pub vendor_id: u16,
    pub product_id: u16,
}

impl Device {
    /// Checks whether the device counts towards the icon colour: on battery and answering.
    pub fn counts_for_icon(&self) -> bool {
        self.connection != Connection::Usb && self.battery.is_some_and(|battery| !battery.is_charging)
    }

    /// Checks whether no protocol recognised the device, so the row offers Report and Try to detect.
    pub fn is_unrecognised(&self) -> bool {
        self.protocol.is_none() && self.name_source == NameSource::Unknown
    }
}

/// Returns "Unknown mouse" or "Unknown keyboard" for a device no source could name.
pub fn unknown_name(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mouse => "Unknown mouse",
        DeviceKind::Keyboard => "Unknown keyboard",
        DeviceKind::Other => "Unknown device",
    }
}

/// Drops a dongle row while a Bluetooth row of the same kind and vendor is connected: that is the same
/// device switched to Bluetooth with its dongle still plugged in, and the dongle row would show the last
/// % it heard, stale. Different devices on different links (a Bluetooth mouse, a 2.4 GHz keyboard) all stay.
pub fn drop_idle_twins(rows: Vec<Device>) -> Vec<Device> {
    let on_bluetooth: Vec<(DeviceKind, u16)> = rows.iter().filter(|row| row.connection == Connection::Bluetooth && row.vendor_id != 0).map(|row| (row.kind, row.vendor_id)).collect();
    rows.into_iter().filter(|row| row.connection != Connection::Dongle || !on_bluetooth.contains(&(row.kind, row.vendor_id))).collect()
}
