//! Known dongles: USB ids to protocol family, kind and a family name.
//!
//! A family name is never shown on its own when the same chip ships under several brands
//! (see `brand_shared`); the name chain then prefers a remembered Bluetooth name.

use crate::device::DeviceKind;

/// One known dongle or wired device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    /// The HID usage page of the interface the protocol talks on.
    pub usage_page: u16,
    pub protocol: &'static str,
    pub kind: DeviceKind,
    pub family_name: &'static str,
    /// True when other brands sell the same hardware with the same ids, so the family name may be wrong.
    pub brand_shared: bool,
    pub is_wireless: bool,
}

#[allow(clippy::too_many_arguments)]
const fn entry(
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
    protocol: &'static str,
    kind: DeviceKind,
    family_name: &'static str,
    brand_shared: bool,
    is_wireless: bool,
) -> KnownDevice {
    KnownDevice { vendor_id, product_id, usage_page, protocol, kind, family_name, brand_shared, is_wireless }
}

/// Every device the protocols in this crate know how to read.
pub const CATALOG: &[KnownDevice] = &[
    // Kreo, from the device list in Kreo Kontrol (kontrol.kreo-tech.com), 23 Nov 2026.
    entry(0x3554, 0xFA09, 0xFF02, "bytech", DeviceKind::Keyboard, "Kreo Swarm 75", false, true),
    entry(0x258A, 0x010C, 0xFF00, "bytech", DeviceKind::Keyboard, "Kreo Swarm 75", false, false),
    entry(0x04F3, 0x026F, 0x000A, "yuandaxin", DeviceKind::Mouse, "Yuandaxin wireless mouse", true, true),
    entry(0x04F3, 0x026E, 0x000A, "yuandaxin", DeviceKind::Mouse, "Yuandaxin mouse", true, false),
    // The same devices over Bluetooth, read through the OS (ids seen on Windows, 23 Nov 2026).
    entry(0x3554, 0xFA07, 0x0000, "bluetooth", DeviceKind::Keyboard, "Kreo Swarm 75", false, true),
    entry(0x04F3, 0x026D, 0x0000, "bluetooth", DeviceKind::Mouse, "Yuandaxin wireless mouse", true, true),
];

/// Returns the catalog entry for a HID interface, if any.
pub fn find_known(vendor_id: u16, product_id: u16, usage_page: u16) -> Option<&'static KnownDevice> {
    CATALOG
        .iter()
        .find(|known| known.vendor_id == vendor_id && known.product_id == product_id && known.usage_page == usage_page)
}

/// Returns any catalog entry for the ids, ignoring the interface, to name and group a dongle.
pub fn find_by_ids(vendor_id: u16, product_id: u16) -> Option<&'static KnownDevice> {
    CATALOG.iter().find(|known| known.vendor_id == vendor_id && known.product_id == product_id)
}
