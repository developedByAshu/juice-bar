//! The few HID calls a protocol needs, behind one trait so tests can replay recorded replies.

use std::collections::VecDeque;

/// One HID interface of a device, opened for talking.
pub trait HidTransport {
    /// Sends an output report. The first byte is the report id (0 when the device uses none).
    fn write(&mut self, report: &[u8]) -> Result<(), String>;
    /// Waits up to `timeout_ms` for an input report and returns its length, 0 on timeout.
    fn read_timeout(&mut self, buffer: &mut [u8], timeout_ms: i32) -> Result<usize, String>;
    /// Sends a feature report. The first byte is the report id.
    fn send_feature(&mut self, report: &[u8]) -> Result<(), String>;
    /// Reads a feature report into `buffer`, whose first byte is the report id to ask for.
    fn get_feature(&mut self, buffer: &mut [u8]) -> Result<usize, String>;
}

/// One recorded exchange: what the protocol sent and what the device answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exchange {
    Write(Vec<u8>),
    Read(Vec<u8>),
    SendFeature(Vec<u8>),
    GetFeature(Vec<u8>),
}

/// A transport that plays back a recording and fails on anything the recording did not expect.
/// Lets either machine test a protocol without the dongle plugged in.
pub struct ReplayTransport {
    script: VecDeque<Exchange>,
}

impl ReplayTransport {
    /// Creates a replay from the exchanges in order.
    pub fn new(script: Vec<Exchange>) -> Self {
        Self { script: script.into() }
    }

    /// Parses a fixture file: one exchange per line, "W", "R", "SF" or "GF" then hex bytes. # starts a comment.
    pub fn from_fixture(text: &str) -> Self {
        let script = text
            .lines()
            .map(|line| line.split('#').next().unwrap_or("").trim())
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let kind = parts.next()?;
                let bytes: Vec<u8> = parts.filter_map(|hex| u8::from_str_radix(hex, 16).ok()).collect();
                match kind {
                    "W" => Some(Exchange::Write(bytes)),
                    "R" => Some(Exchange::Read(bytes)),
                    "SF" => Some(Exchange::SendFeature(bytes)),
                    "GF" => Some(Exchange::GetFeature(bytes)),
                    _ => None,
                }
            })
            .collect();
        Self::new(script)
    }

    /// Checks whether every recorded exchange was used.
    pub fn is_finished(&self) -> bool {
        self.script.is_empty()
    }

    fn expect_sent(&mut self, sent: &[u8], is_feature: bool) -> Result<(), String> {
        match self.script.pop_front() {
            Some(Exchange::Write(expected)) if !is_feature && expected == sent => Ok(()),
            Some(Exchange::SendFeature(expected)) if is_feature && expected == sent => Ok(()),
            other => Err(format!("replay expected {other:?}, protocol sent {sent:02x?}")),
        }
    }

    fn take_reply(&mut self, buffer: &mut [u8], is_feature: bool) -> Result<usize, String> {
        match self.script.pop_front() {
            Some(Exchange::Read(reply)) if !is_feature => Ok(copy_reply(&reply, buffer)),
            Some(Exchange::GetFeature(reply)) if is_feature => Ok(copy_reply(&reply, buffer)),
            other => Err(format!("replay expected {other:?}, protocol asked for a reply")),
        }
    }
}

fn copy_reply(reply: &[u8], buffer: &mut [u8]) -> usize {
    let length = reply.len().min(buffer.len());
    buffer[..length].copy_from_slice(&reply[..length]);
    length
}

impl HidTransport for ReplayTransport {
    fn write(&mut self, report: &[u8]) -> Result<(), String> {
        self.expect_sent(report, false)
    }

    fn read_timeout(&mut self, buffer: &mut [u8], _timeout_ms: i32) -> Result<usize, String> {
        // A recording that ran out of reads behaves like a device that went quiet.
        if !matches!(self.script.front(), Some(Exchange::Read(_))) {
            return Ok(0);
        }
        self.take_reply(buffer, false)
    }

    fn send_feature(&mut self, report: &[u8]) -> Result<(), String> {
        self.expect_sent(report, true)
    }

    fn get_feature(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        self.take_reply(buffer, true)
    }
}

/// The real transport over hidapi, the same code on Mac and Windows.
pub struct HidapiTransport {
    pub device: hidapi::HidDevice,
}

impl HidTransport for HidapiTransport {
    fn write(&mut self, report: &[u8]) -> Result<(), String> {
        self.device.write(report).map(|_| ()).map_err(|error| error.to_string())
    }

    fn read_timeout(&mut self, buffer: &mut [u8], timeout_ms: i32) -> Result<usize, String> {
        self.device.read_timeout(buffer, timeout_ms).map_err(|error| error.to_string())
    }

    fn send_feature(&mut self, report: &[u8]) -> Result<(), String> {
        self.device.send_feature_report(report).map_err(|error| error.to_string())
    }

    fn get_feature(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        self.device.get_feature_report(buffer).map_err(|error| error.to_string())
    }
}
