//! The "yuandaxin" mouse protocol. Battery bytes not recovered yet (see docs/PROTOCOLS.md), so it never answers.

use super::{BatteryProtocol, Reading};
use crate::transport::HidTransport;

/// The yuandaxin chip family.
pub struct Yuandaxin;

impl BatteryProtocol for Yuandaxin {
    fn id(&self) -> &'static str {
        "yuandaxin"
    }

    fn usage_page(&self) -> u16 {
        0x000A
    }

    fn read_battery(&self, _transport: &mut dyn HidTransport) -> Result<Reading, String> {
        Ok(Reading::NoAnswer)
    }
}
