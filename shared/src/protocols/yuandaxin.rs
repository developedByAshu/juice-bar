//! The "yuandaxin" protocol, as Kreo Kontrol speaks it. Bytes recovered from Kontrol's driver, see docs/PROTOCOLS.md.

use super::{BatteryProtocol, Reading};
use crate::transport::HidTransport;

/// The yuandaxin chip family.
pub struct Yuandaxin;

impl BatteryProtocol for Yuandaxin {
    fn id(&self) -> &'static str {
        "yuandaxin"
    }

    fn usage_page(&self) -> u16 {
        0
    }

    fn read_battery(&self, _transport: &mut dyn HidTransport) -> Result<Reading, String> {
        Ok(Reading::NoAnswer)
    }
}
