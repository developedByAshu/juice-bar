//! The "yuandaxin" mouse protocol (dongle 04f3:026f: Amkette XS Flow Pro Plus, Kreo Hawk Pro).
//!
//! The mouse pushes a status report on its own about every 5 s, on the page 0x0A input interface:
//! report id 4, then `01 04 01 <percent> 00 00 00 01 03 02 ...`. Recorded from Kreo Kontrol's WebHID
//! traffic on 28 Nov 2026 (see docs/PROTOCOLS.md). Nothing is ever sent to the mouse: it only listens.

use super::{BatteryProtocol, Reading};
use crate::device::Battery;
use crate::transport::HidTransport;

/// Report id of the pushed status report.
const REPORT_ID: u8 = 0x04;
/// The bytes after the report id that mark a battery status report.
const STATUS_PREFIX: [u8; 3] = [0x01, 0x04, 0x01];
/// Reads to wait, one second each: the mouse pushes about every 5 s, so 6 covers one full cycle.
const LISTEN_READS: usize = 6;
const READ_TIMEOUT_MS: i32 = 1000;

/// The yuandaxin chip family.
pub struct Yuandaxin;

/// Reads a pushed status report. `report` starts with the report id. None if it is another report.
pub fn parse_status(report: &[u8]) -> Option<Battery> {
    let (&id, rest) = report.split_first()?;
    if id != REPORT_ID || rest.len() < 4 || rest[..3] != STATUS_PREFIX {
        return None;
    }
    let percent = rest[3];
    (1..=100).contains(&percent).then_some(Battery { percent, is_charging: false })
}

impl BatteryProtocol for Yuandaxin {
    fn id(&self) -> &'static str {
        "yuandaxin"
    }

    fn usage_page(&self) -> u16 {
        0x000A
    }

    fn read_battery(&self, transport: &mut dyn HidTransport) -> Result<Reading, String> {
        let mut buffer = [0u8; 64];
        for _ in 0..LISTEN_READS {
            let size = transport.read_timeout(&mut buffer, READ_TIMEOUT_MS)?;
            if let Some(battery) = parse_status(&buffer[..size]) {
                return Ok(Reading::Battery(battery));
            }
        }
        Ok(Reading::NoAnswer)
    }
}
