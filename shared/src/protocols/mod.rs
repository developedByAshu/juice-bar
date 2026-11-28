//! Battery protocols. Each one asks a device for its battery in its own language.
//!
//! Adding a protocol: a file here with a type implementing `BatteryProtocol`, an entry in
//! `protocol_by_id`, a catalog row per device, and a fixture test under `shared/tests/fixtures`.

use crate::device::Battery;
use crate::transport::HidTransport;

pub mod bytech;
pub mod yuandaxin;

/// What a read can return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    Battery(Battery),
    /// The dongle answered but the device behind it is asleep or switched off.
    DeviceOffline,
    /// The interface did not answer in this protocol's language.
    NoAnswer,
}

/// A battery protocol for one chip family.
pub trait BatteryProtocol {
    /// Short id, as used in the catalog and the Report text.
    fn id(&self) -> &'static str;
    /// The HID usage page of the interface to open for this protocol.
    fn usage_page(&self) -> u16;
    /// Asks the device for its battery. Only read commands, never a setting change.
    fn read_battery(&self, transport: &mut dyn HidTransport) -> Result<Reading, String>;
    /// The report id the protocol sends its read command with, or None when it only listens. The
    /// opt-in probe sends nothing to an interface whose report descriptor has no such report.
    fn command_report_id(&self) -> Option<u8> {
        None
    }
}

// A HID report descriptor item: the low two bits of its first byte give its data size, the rest its
// tag and type. 0x84 is the global Report ID item; 0xFE starts a long item, which carries its own size.
const ITEM_TAG_MASK: u8 = 0xFC;
const ITEM_REPORT_ID: u8 = 0x84;
const ITEM_LONG: u8 = 0xFE;

/// Returns true when a HID report descriptor declares this report id. A broken descriptor gives false.
pub fn declares_report_id(descriptor: &[u8], report_id: u8) -> bool {
    let mut index = 0;
    while let Some(&prefix) = descriptor.get(index) {
        if prefix == ITEM_LONG {
            let Some(&size) = descriptor.get(index + 1) else { return false };
            index += 3 + usize::from(size);
            continue;
        }
        let size = [0, 1, 2, 4][usize::from(prefix & 0x03)];
        let Some(data) = descriptor.get(index + 1..index + 1 + size) else { return false };
        if prefix & ITEM_TAG_MASK == ITEM_REPORT_ID && data.first() == Some(&report_id) {
            return true;
        }
        index += 1 + size;
    }
    false
}

/// Returns the protocol for a catalog id.
pub fn protocol_by_id(id: &str) -> Option<Box<dyn BatteryProtocol>> {
    match id {
        "bytech" => Some(Box::new(bytech::Bytech)),
        "yuandaxin" => Some(Box::new(yuandaxin::Yuandaxin)),
        _ => None,
    }
}

/// Every protocol, in the order the opt-in probe tries them.
pub fn all_protocols() -> Vec<Box<dyn BatteryProtocol>> {
    ["bytech", "yuandaxin"].iter().filter_map(|id| protocol_by_id(id)).collect()
}
