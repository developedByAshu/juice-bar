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
