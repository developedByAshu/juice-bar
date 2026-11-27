//! The "bytech" keyboard protocol (Kreo Swarm, Hive), as Kreo Kontrol speaks it over the dongle.
//!
//! Packet, after the report id: [command, packet count, packet index, (profile << 4) | payload length,
//! payload (up to 14 bytes), zero padding, checksum], 19 bytes. Checksum = (report id + every byte
//! before it) & 0xFF. Layout taken from the firmware simulator shipped in Kontrol, see docs/PROTOCOLS.md.

use super::{BatteryProtocol, Reading};
use crate::device::Battery;
use crate::transport::HidTransport;

/// Report id of the wireless command channel on usage page 0xFF02 (the real dongle uses 19; Kontrol's simulator says 4).
const REPORT_ID: u8 = 19;
/// Packet length after the report id.
const PACKET_LENGTH: usize = 19;
/// Command that returns battery % and charging.
const GET_POWER: u8 = 0x4A;
/// How long to wait for the answer in total.
const ANSWER_TIMEOUT_MS: i32 = 600;

/// The bytech chip family.
pub struct Bytech;

/// Returns the checksum of a packet (all bytes but the last).
pub fn checksum(report_id: u8, packet: &[u8]) -> u8 {
    packet[..PACKET_LENGTH - 1].iter().fold(report_id, |sum, byte| sum.wrapping_add(*byte))
}

/// Builds a request with no payload: the report id, then the 19 byte packet.
pub fn build_request(command: u8) -> Vec<u8> {
    let mut packet = vec![0u8; PACKET_LENGTH];
    packet[0] = command;
    packet[1] = 1;
    packet[PACKET_LENGTH - 1] = checksum(REPORT_ID, &packet);
    let mut report = vec![REPORT_ID];
    report.extend(packet);
    report
}

/// Reads a GET_POWER answer. `report` starts with the report id. None if it is another packet or corrupt.
pub fn parse_power_answer(report: &[u8]) -> Option<Battery> {
    let (&report_id, packet) = report.split_first()?;
    if report_id != REPORT_ID || packet.len() < PACKET_LENGTH || packet[0] != GET_POWER {
        return None;
    }
    if packet[PACKET_LENGTH - 1] != checksum(REPORT_ID, packet) || packet[3] & 0x0F < 2 {
        return None;
    }
    let percent = packet[4];
    (percent <= 100).then_some(Battery { percent, is_charging: (packet[5] >> 4) & 1 == 1 })
}

/// Command byte of a packet the dongle pushes on its own (no request), on the same channel.
const EVENT: u8 = 0x0A;
/// Event kind: the keyboard's link to the dongle changed; the next byte is 1 for awake, 0 for asleep.
const EVENT_LINK: u8 = 0x02;
/// Event kind: the dongle reports the battery; the next byte is the %.
const EVENT_BATTERY: u8 = 0x05;

/// What the dongle tells us without being asked. Recorded on the Swarm 75 dongle on 28 Nov 2026:
/// `0a 01 00 04 02 00` when the keyboard fell asleep, `02 01` on the first key press after, `05 47 01`
/// as a battery push (71%). This is the callback the app waits on, instead of polling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardEvent {
    Awake,
    Asleep,
    Battery(Battery),
}

/// Reads a pushed event packet. `report` starts with the report id. None for anything else or a bad checksum.
pub fn parse_event(report: &[u8]) -> Option<KeyboardEvent> {
    let (&report_id, packet) = report.split_first()?;
    if report_id != REPORT_ID || packet.len() < PACKET_LENGTH || packet[0] != EVENT || packet[PACKET_LENGTH - 1] != checksum(REPORT_ID, packet) {
        return None;
    }
    match packet[4] {
        EVENT_LINK if packet[5] == 1 => Some(KeyboardEvent::Awake),
        EVENT_LINK => Some(KeyboardEvent::Asleep),
        EVENT_BATTERY if (1..=100).contains(&packet[5]) => Some(KeyboardEvent::Battery(Battery { percent: packet[5], is_charging: false })),
        _ => None,
    }
}

impl BatteryProtocol for Bytech {
    fn id(&self) -> &'static str {
        "bytech"
    }

    fn usage_page(&self) -> u16 {
        0xFF02
    }

    fn read_battery(&self, transport: &mut dyn HidTransport) -> Result<Reading, String> {
        transport.write(&build_request(GET_POWER))?;
        let mut waited = 0;
        while waited < ANSWER_TIMEOUT_MS {
            let mut buffer = [0u8; 64];
            let length = transport.read_timeout(&mut buffer, 100)?;
            waited += 100;
            if length == 0 {
                continue;
            }
            if let Some(battery) = parse_power_answer(&buffer[..length]) {
                return Ok(Reading::Battery(battery));
            }
        }
        // The dongle took the request but the keyboard did not answer: asleep or switched off.
        Ok(Reading::DeviceOffline)
    }
}
