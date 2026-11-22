//! The colour rule shared by the icon and every row in the menu.

use crate::device::Device;

/// The four colours the battery can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Every counted device is above 85%.
    Green,
    /// The plain menu bar colour, which follows light and dark mode.
    Plain,
    /// 20 to 30%.
    Amber,
    /// Below 20%.
    Red,
}

/// Returns the tone of one reading. Exactly 85 is plain, exactly 30 and 20 are amber.
pub fn tone_for_percent(percent: u8) -> Tone {
    match percent {
        86..=u8::MAX => Tone::Green,
        31..=85 => Tone::Plain,
        20..=30 => Tone::Amber,
        _ => Tone::Red,
    }
}

/// Returns the icon tone: the lowest counted device decides; nothing counted means plain.
pub fn icon_tone(devices: &[Device]) -> Tone {
    devices
        .iter()
        .filter(|device| device.counts_for_icon())
        .filter_map(|device| device.battery.map(|battery| battery.percent))
        .min()
        .map_or(Tone::Plain, tone_for_percent)
}

/// Returns the lowest counted %, for the optional number next to the icon.
pub fn lowest_percent(devices: &[Device]) -> Option<u8> {
    devices
        .iter()
        .filter(|device| device.counts_for_icon())
        .filter_map(|device| device.battery.map(|battery| battery.percent))
        .min()
}
