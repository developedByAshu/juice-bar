//! The "bytech" protocol, as Kreo Kontrol speaks it. Bytes recovered from Kontrol's driver, see docs/PROTOCOLS.md.

use super::{BatteryProtocol, Reading};
use crate::transport::HidTransport;

/// The bytech chip family.
pub struct Bytech;

impl BatteryProtocol for Bytech {
    fn id(&self) -> &'static str {
        "bytech"
    }

    fn usage_page(&self) -> u16 {
        0
    }

    fn read_battery(&self, _transport: &mut dyn HidTransport) -> Result<Reading, String> {
        Ok(Reading::NoAnswer)
    }
}
