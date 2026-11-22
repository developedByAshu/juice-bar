//! Logic behind the Windows app that runs without a screen: rows, tones, placement, the glyph and
//! the theme rules. Run with `cargo test -p juicebar-windows`.
#![cfg(windows)]

use juicebar_shared::device::{Battery, Connection, Device, DeviceKind, NameSource};
use juicebar_shared::tone::Tone;
use juicebar_windows::app::tooltip_text;
use juicebar_windows::glyph::glass_image;
use juicebar_windows::menu_ui::{build_device_row, menu_height, place_flyout, RowEnd};
use juicebar_windows::startup::startup_command;
use juicebar_windows::status_icon::{glyph_color, unpremultiply};
use juicebar_windows::theme::{build_theme, AMBER, GREEN, RED};
use windows_sys::Win32::Foundation::RECT;

fn device(name: &str, battery: Option<(u8, bool)>, protocol: Option<&'static str>) -> Device {
    Device {
        id: format!("test:{name}"),
        kind: DeviceKind::Mouse,
        connection: Connection::Dongle,
        name: name.to_string(),
        name_source: if protocol.is_some() { NameSource::Catalog } else { NameSource::Unknown },
        battery: battery.map(|(percent, is_charging)| Battery { percent, is_charging }),
        protocol,
        vendor_id: 0x04F3,
        product_id: 0x026F,
    }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT { left, top, right, bottom }
}

#[test]
fn row_shows_percent_in_its_tone() {
    let row = build_device_row(&device("Kreo Swarm 75", Some((82, false)), Some("bytech")));
    assert_eq!(row.end, RowEnd::Percent { text: "82%".into(), tone: Tone::Plain, is_charging: false });
    assert_eq!(row.connection, "2.4 GHz");
    assert!(!row.is_dimmed);
}

#[test]
fn row_tone_boundaries_match_the_spec() {
    let tone_of = |percent| match build_device_row(&device("x", Some((percent, false)), Some("bytech"))).end {
        RowEnd::Percent { tone, .. } => tone,
        other => panic!("expected a percent, got {other:?}"),
    };
    assert_eq!(tone_of(86), Tone::Green);
    assert_eq!(tone_of(85), Tone::Plain);
    assert_eq!(tone_of(31), Tone::Plain);
    assert_eq!(tone_of(30), Tone::Amber);
    assert_eq!(tone_of(20), Tone::Amber);
    assert_eq!(tone_of(19), Tone::Red);
}

#[test]
fn charging_row_carries_the_bolt() {
    let row = build_device_row(&device("XS Flow+1", Some((15, true)), Some("yuandaxin")));
    assert_eq!(row.end, RowEnd::Percent { text: "15%".into(), tone: Tone::Red, is_charging: true });
}

#[test]
fn unrecognised_device_offers_report_and_detect() {
    let row = build_device_row(&device("Unknown mouse", None, None));
    assert_eq!(row.end, RowEnd::Actions);
    assert!(!row.is_dimmed);
    assert_eq!(row.dot, None);
}

#[test]
fn known_device_without_reading_is_not_connected_and_grey() {
    let row = build_device_row(&device("XS Flow+1", None, Some("yuandaxin")));
    assert_eq!(row.end, RowEnd::NotConnected);
    assert!(row.is_dimmed);
}

#[test]
fn flyout_sits_above_a_bottom_taskbar_inside_the_screen() {
    let monitor = rect(0, 0, 1920, 1080);
    let work = rect(0, 0, 1920, 1032);
    let icon = rect(1800, 1040, 1824, 1072);
    let (x, y) = place_flyout(icon, monitor, work, 380, 260, 12);
    assert_eq!(y, 1032 - 12 - 260);
    assert_eq!(x, 1920 - 12 - 380, "a flyout near the right edge is pushed back inside");
}

#[test]
fn flyout_sits_below_a_top_taskbar() {
    let (_, y) = place_flyout(rect(900, 8, 924, 40), rect(0, 0, 1920, 1080), rect(0, 48, 1920, 1080), 380, 260, 12);
    assert_eq!(y, 48 + 12);
}

#[test]
fn flyout_sits_beside_a_left_taskbar() {
    let (x, _) = place_flyout(rect(8, 900, 40, 924), rect(0, 0, 1920, 1080), rect(48, 0, 1920, 1080), 380, 260, 12);
    assert_eq!(x, 48 + 12);
}

#[test]
fn flyout_height_grows_with_rows_and_keeps_room_for_the_empty_line() {
    assert_eq!(menu_height(0), menu_height(1));
    assert_eq!(menu_height(3) - menu_height(2), 40);
}

#[test]
fn tooltip_lists_each_device_and_stays_within_the_tray_limit() {
    let devices = vec![device("Kreo Swarm 75", Some((82, false)), Some("bytech")), device("XS Flow+1", Some((15, true)), Some("yuandaxin")), device("Unknown mouse", None, None)];
    assert_eq!(tooltip_text(&devices), "Juice Bar\nKreo Swarm 75: 82%\nXS Flow+1: 15% charging\nUnknown mouse: unknown");
    let many: Vec<Device> = (0..20).map(|index| device(&format!("A rather long device name {index}"), Some((50, false)), Some("bytech"))).collect();
    assert!(tooltip_text(&many).chars().count() <= 127);
}

#[test]
fn plain_glyph_follows_the_taskbar_and_tones_use_contract_colours() {
    let dark_taskbar = build_theme(false, true, false, true, 26100, 0x0067C0);
    let light_taskbar = build_theme(true, false, false, true, 26100, 0x0067C0);
    assert_eq!(glyph_color(Tone::Plain, &dark_taskbar), 0xFFFFFF);
    assert_eq!(glyph_color(Tone::Plain, &light_taskbar), 0x000000);
    assert_eq!(glyph_color(Tone::Green, &dark_taskbar), GREEN);
    assert_eq!(glyph_color(Tone::Amber, &dark_taskbar), AMBER);
    assert_eq!(glyph_color(Tone::Red, &dark_taskbar), RED);
}

#[test]
fn backdrop_only_when_transparency_is_on_contrast_is_normal_and_windows_can_draw_it() {
    assert!(build_theme(true, true, false, true, 22621, 0).has_backdrop);
    assert!(!build_theme(true, true, false, false, 26100, 0).has_backdrop, "transparency effects off");
    assert!(!build_theme(true, true, true, true, 26100, 0).has_backdrop, "high contrast");
    assert!(!build_theme(true, true, false, true, 19045, 0).has_backdrop, "Windows 10");
}

#[test]
fn empty_glass_has_no_juice_and_a_full_one_does() {
    let has_juice = |pixels: &[u32]| pixels.iter().any(|pixel| pixel & 0x00FF_FFFF == 0x0000_FF00 && pixel >> 24 == 0xFF);
    let empty = glass_image(32, 0xFFFFFF, 0x00FF00, None);
    let full = glass_image(32, 0xFFFFFF, 0x00FF00, Some(100));
    assert_eq!(empty.len(), 32 * 32);
    assert!(!has_juice(&empty));
    assert!(has_juice(&full));
}

#[test]
fn juice_rises_with_the_battery() {
    let juice_rows = |percent| {
        let pixels = glass_image(32, 0xFFFFFF, 0x00FF00, Some(percent));
        (0..32).filter(|row| (0..32).any(|column| pixels[row * 32 + column] == 0xFF00_FF00)).count()
    };
    assert!(juice_rows(20) < juice_rows(60));
    assert!(juice_rows(60) < juice_rows(100));
    assert!(juice_rows(1) > 0, "a nearly empty battery still shows a sliver");
}

#[test]
fn icon_pixels_lose_their_premultiplied_alpha() {
    assert_eq!(unpremultiply(0x8040_2010), 0x8080_4020);
    assert_eq!(unpremultiply(0xFF12_3456), 0xFF12_3456);
    assert_eq!(unpremultiply(0x0000_0000), 0);
}

#[test]
fn startup_command_quotes_paths_with_spaces() {
    assert_eq!(startup_command("C:\\Program Files\\Juice Bar\\JuiceBar.exe"), "\"C:\\Program Files\\Juice Bar\\JuiceBar.exe\"");
}
