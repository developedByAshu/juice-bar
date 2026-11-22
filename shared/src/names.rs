//! Picks the name a row shows. First source that has one wins; a user rename beats all.

use crate::device::{unknown_name, DeviceKind, NameSource};

/// Everything known about a device that could give it a name.
#[derive(Clone, Debug, Default)]
pub struct NameCandidates {
    pub user_rename: Option<String>,
    pub device_reported: Option<String>,
    /// Names the OS remembers for Bluetooth devices of the same kind.
    pub bluetooth_memory: Vec<String>,
    pub catalog: Option<(String, bool)>,
    pub usb_product: Option<String>,
}

/// USB product strings that say nothing about the device.
const GENERIC_USB_NAMES: &[&str] = &["2.4g wireless receiver", "wireless receiver", "usb receiver", "usb gaming mouse", "gaming mouse", "usb keyboard", "receiver"];

/// Checks whether a USB product string is a generic receiver name.
pub fn is_generic_usb_name(name: &str) -> bool {
    let lowered = name.trim().to_lowercase();
    lowered.is_empty() || GENERIC_USB_NAMES.contains(&lowered.as_str())
}

/// Returns the name to show and where it came from.
pub fn resolve_name(kind: DeviceKind, candidates: &NameCandidates) -> (String, NameSource) {
    let clean = |name: &Option<String>| name.as_ref().map(|text| text.trim().to_string()).filter(|text| !text.is_empty());
    if let Some(name) = clean(&candidates.user_rename) {
        return (name, NameSource::UserRename);
    }
    if let Some(name) = clean(&candidates.device_reported) {
        return (name, NameSource::DeviceReported);
    }
    // An exact catalog name beats a guess by kind; a brand-shared chip's family name never shows.
    if let Some((name, brand_shared)) = &candidates.catalog {
        if !brand_shared {
            return (name.clone(), NameSource::Catalog);
        }
    }
    // One remembered Bluetooth device of the same kind is a confident match; several are not.
    if let [only] = candidates.bluetooth_memory.as_slice() {
        return (only.trim().to_string(), NameSource::BluetoothMemory);
    }
    if let Some(name) = clean(&candidates.usb_product).filter(|name| !is_generic_usb_name(name)) {
        return (name, NameSource::UsbProduct);
    }
    (unknown_name(kind).to_string(), NameSource::Unknown)
}
