use juicebar_shared::device::{Battery, Connection, Device, DeviceKind, NameSource};
use juicebar_shared::names::{resolve_name, NameCandidates};
use juicebar_shared::protocols::bytech::{build_request, parse_event, parse_power_answer, with_pushed_percent, Bytech, KeyboardEvent};
use juicebar_shared::protocols::yuandaxin::{self, Yuandaxin};
use juicebar_shared::protocols::{BatteryProtocol, Reading};
use juicebar_shared::report::report_url;
use juicebar_shared::settings::{ClickAction, HoverAction, Settings, MAX_WINDOW_THEME, MENU_FONT_POINTS_DEFAULT, MENU_FONT_POINTS_MAX, MENU_FONT_POINTS_MIN};
use juicebar_shared::tone::{icon_tone, tone_for_percent, Tone};
use juicebar_shared::transport::ReplayTransport;
use juicebar_shared::update::UpdateCheck;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

fn device_at(percent: u8, connection: Connection, is_charging: bool) -> Device {
    Device {
        id: format!("test:{percent}"),
        kind: DeviceKind::Mouse,
        connection,
        name: "Test".into(),
        name_source: NameSource::Catalog,
        battery: Some(Battery { percent, is_charging }),
        protocol: Some("bytech"),
        vendor_id: 0x3554,
        product_id: 0xFA09,
    }
}

#[test]
fn tone_boundaries_follow_the_agreed_rule() {
    assert_eq!(tone_for_percent(100), Tone::Green);
    assert_eq!(tone_for_percent(86), Tone::Green);
    assert_eq!(tone_for_percent(85), Tone::Plain);
    assert_eq!(tone_for_percent(31), Tone::Plain);
    assert_eq!(tone_for_percent(30), Tone::Amber);
    assert_eq!(tone_for_percent(20), Tone::Amber);
    assert_eq!(tone_for_percent(19), Tone::Red);
    assert_eq!(tone_for_percent(0), Tone::Red);
}

#[test]
fn icon_takes_the_lowest_device_and_skips_cable_and_charging() {
    let both_high = [device_at(90, Connection::Dongle, false), device_at(88, Connection::Bluetooth, false)];
    assert_eq!(icon_tone(&both_high), Tone::Green);
    let one_low = [device_at(90, Connection::Dongle, false), device_at(15, Connection::Dongle, false)];
    assert_eq!(icon_tone(&one_low), Tone::Red);
    let low_on_cable = [device_at(90, Connection::Dongle, false), device_at(15, Connection::Usb, false), device_at(10, Connection::Dongle, true)];
    assert_eq!(icon_tone(&low_on_cable), Tone::Green);
    assert_eq!(icon_tone(&[]), Tone::Plain);
}

#[test]
fn names_never_show_a_brand_shared_family_name() {
    let shared_chip = NameCandidates { catalog: Some(("Yuandaxin wireless mouse".into(), true)), usb_product: Some("2.4G Wireless Receiver".into()), ..Default::default() };
    assert_eq!(resolve_name(DeviceKind::Mouse, &shared_chip), ("Unknown mouse".to_string(), NameSource::Unknown));
    let with_bluetooth = NameCandidates { bluetooth_memory: vec!["XS Flow+1".into()], ..shared_chip.clone() };
    assert_eq!(resolve_name(DeviceKind::Mouse, &with_bluetooth), ("XS Flow+1".to_string(), NameSource::BluetoothMemory));
    let two_remembered = NameCandidates { bluetooth_memory: vec!["A".into(), "B".into()], ..shared_chip };
    assert_eq!(resolve_name(DeviceKind::Mouse, &two_remembered).1, NameSource::Unknown);
    let renamed = NameCandidates { user_rename: Some("Desk mouse".into()), bluetooth_memory: vec!["XS Flow+1".into()], ..Default::default() };
    assert_eq!(resolve_name(DeviceKind::Mouse, &renamed), ("Desk mouse".to_string(), NameSource::UserRename));
}

#[test]
fn exact_catalog_name_beats_a_bluetooth_guess() {
    let keyboard = NameCandidates { catalog: Some(("Kreo Swarm 75".into(), false)), bluetooth_memory: vec!["Kreo Swarm 5.0".into()], ..Default::default() };
    assert_eq!(resolve_name(DeviceKind::Keyboard, &keyboard), ("Kreo Swarm 75".to_string(), NameSource::Catalog));
}

#[test]
fn glyph_juice_rises_with_the_level_and_stays_inside_the_glass() {
    use juicebar_shared::glyph::{juice_polygon, glyph_svg};
    assert!(juice_polygon(None).is_none());
    let low = juice_polygon(Some(10)).unwrap();
    let high = juice_polygon(Some(100)).unwrap();
    assert!(high[2].1 > low[2].1);
    assert!(high[3].0 > 1.6 && high[2].0 < 12.4 && high[2].1 < 15.2);
    assert!(glyph_svg(Some(50), "#34C759").contains("stroke=\"currentColor\""));
}

#[test]
fn settings_survive_a_round_trip_and_skip_broken_lines() {
    let mut settings = Settings { poll_minutes: 10, shows_percent_in_icon: true, is_saved_to_disk: false, ..Settings::default() };
    settings.renames.insert("3554:fa09:".into(), "Desk keyboard".into());
    settings.hidden.push("04f3:026f:".into());
    let loaded = Settings::from_text(&(settings.to_text() + "garbage line\npoll_minutes=999\n"));
    assert_eq!(loaded.renames, settings.renames);
    assert_eq!(loaded.hidden, settings.hidden);
    assert_eq!(loaded.poll_minutes, 60);
    assert!(loaded.shows_percent_in_icon);
}

#[test]
fn menu_font_points_parse_clamp_and_round_trip() {
    assert_eq!(Settings::default().menu_font_points, MENU_FONT_POINTS_DEFAULT);
    assert_eq!(Settings::from_text("menu_font_points=11\n").menu_font_points, 11);
    assert_eq!(Settings::from_text("menu_font_points=3\n").menu_font_points, MENU_FONT_POINTS_MIN);
    assert_eq!(Settings::from_text("menu_font_points=40\n").menu_font_points, MENU_FONT_POINTS_MAX);
    assert_eq!(Settings::from_text("menu_font_points=big\n").menu_font_points, MENU_FONT_POINTS_DEFAULT);
    assert_eq!(Settings::from_text("poll_minutes=5\n").menu_font_points, MENU_FONT_POINTS_DEFAULT);
    let settings = Settings { menu_font_points: 7, ..Settings::default() };
    assert_eq!(Settings::from_text(&settings.to_text()).menu_font_points, 7);
}

#[test]
fn bytech_request_matches_the_recorded_bytes() {
    assert_eq!(build_request(0x4A), vec![0x13, 0x4a, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x5e]);
}

#[test]
fn bytech_reads_the_recorded_swarm75_answer() {
    let mut transport = ReplayTransport::from_fixture(&fixture("bytech_swarm75_battery.txt"));
    assert_eq!(Bytech.read_battery(&mut transport).unwrap(), Reading::Battery(Battery { percent: 100, is_charging: false }));
    assert!(transport.is_finished());
}

#[test]
fn bytech_skips_a_pushed_event_before_the_answer() {
    let mut transport = ReplayTransport::from_fixture(&fixture("bytech_swarm75_event_first.txt"));
    assert_eq!(Bytech.read_battery(&mut transport).unwrap(), Reading::Battery(Battery { percent: 60, is_charging: false }));
}

#[test]
fn bytech_rejects_a_corrupt_answer_and_reports_offline_on_silence() {
    let mut corrupt = vec![0x13, 0x4a, 0x01, 0x00, 0x02, 0x64, 0x01];
    corrupt.resize(20, 0);
    corrupt[19] = 0x00;
    assert_eq!(parse_power_answer(&corrupt), None);
    let mut silent = ReplayTransport::from_fixture("W 13 4a 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 5e");
    assert_eq!(Bytech.read_battery(&mut silent).unwrap(), Reading::DeviceOffline);
}

#[test]
fn yuandaxin_reads_the_recorded_xs_flow_status() {
    let mut transport = ReplayTransport::from_fixture(&fixture("yuandaxin_xsflow_status.txt"));
    assert_eq!(Yuandaxin.read_battery(&mut transport).unwrap(), Reading::Battery(Battery { percent: 50, is_charging: false }));
    assert!(transport.is_finished());
}

#[test]
fn a_battery_push_keeps_charging_from_the_last_power_answer() {
    // The recorded 71% push, which carries no charging flag the app can read.
    let Some(KeyboardEvent::Battery(pushed)) = parse_event(&hex_report("13 0a 01 00 04 05 47 01 00 00 00 00 00 00 00 00 00 00 00 6f")) else {
        panic!("the recorded push should read as a battery event");
    };
    let charging = Battery { percent: 64, is_charging: true };
    assert_eq!(with_pushed_percent(Some(charging), pushed), Battery { percent: 71, is_charging: true });
    let on_battery = Battery { percent: 64, is_charging: false };
    assert_eq!(with_pushed_percent(Some(on_battery), pushed), Battery { percent: 71, is_charging: false });
    assert_eq!(with_pushed_percent(None, pushed), Battery { percent: 71, is_charging: false });
}

#[test]
fn yuandaxin_reads_charging_from_the_recorded_cable_status() {
    let wired = [0x04, 0x01, 0x04, 0x01, 0x32, 0x01, 0x00, 0x00, 0x01, 0x03, 0x02];
    assert_eq!(yuandaxin::parse_status(&wired), Some(Battery { percent: 50, is_charging: true }));
    let dongle_while_wired = [0x04, 0x00, 0x04, 0x01, 0x32, 0x00, 0x00, 0x00, 0x01, 0x03, 0x02];
    assert_eq!(yuandaxin::parse_status(&dongle_while_wired), None);
}

#[test]
fn yuandaxin_ignores_other_reports_and_stays_silent_without_a_status() {
    assert_eq!(yuandaxin::parse_status(&[0x04, 0x01, 0x05, 0x01, 0x32]), None);
    assert_eq!(yuandaxin::parse_status(&[0x05, 0x01, 0x04, 0x01, 0x32]), None);
    assert_eq!(yuandaxin::parse_status(&[0x04, 0x01, 0x04, 0x01, 0x00]), None);
    let mut silent = ReplayTransport::from_fixture("");
    assert_eq!(Yuandaxin.read_battery(&mut silent).unwrap(), Reading::NoAnswer);
}

fn hex_report(text: &str) -> Vec<u8> {
    text.split_whitespace().map(|byte| u8::from_str_radix(byte, 16).unwrap()).collect()
}

#[test]
fn bytech_reads_the_recorded_sleep_wake_and_battery_events() {
    // Recorded on his Swarm 75 dongle, 28 Nov 2026 01:17 to 01:20.
    assert_eq!(parse_event(&hex_report("13 0a 01 00 04 02 00 00 00 00 00 00 00 00 00 00 00 00 00 24")), Some(KeyboardEvent::Asleep));
    assert_eq!(parse_event(&hex_report("13 0a 01 00 04 02 01 00 00 00 00 00 00 00 00 00 00 00 00 25")), Some(KeyboardEvent::Awake));
    assert_eq!(parse_event(&hex_report("13 0a 01 00 04 05 47 01 00 00 00 00 00 00 00 00 00 00 00 6f")), Some(KeyboardEvent::Battery(Battery { percent: 71, is_charging: false })));
    // A battery answer to our own request and a bad checksum are not events.
    assert_eq!(parse_event(&hex_report("13 4a 01 00 02 48 01 00 00 00 00 00 00 00 00 00 00 00 00 a9")), None);
    assert_eq!(parse_event(&hex_report("13 0a 01 00 04 02 01 00 00 00 00 00 00 00 00 00 00 00 00 26")), None);
}

#[test]
fn report_url_carries_ids_and_no_personal_data() {
    let url = report_url(&device_at(50, Connection::Dongle, false), &[], "macOS 26", "0.1.0");
    assert!(url.starts_with("https://github.com/developedByAshu/juice-bar/issues/new?title=Device%20report%3A%203554%3Afa09"));
    assert!(url.contains("Vendor%20id%3A%200x3554"));
}

#[test]
fn hover_and_click_choices_survive_a_save_and_default_to_todays_behaviour() {
    let fresh = Settings::from_text("");
    assert_eq!((fresh.hover_action, fresh.click_action), (HoverAction::Nothing, ClickAction::Devices));
    let chosen = Settings { hover_action: HoverAction::Card, click_action: ClickAction::Settings, ..Settings::default() };
    let loaded = Settings::from_text(&chosen.to_text());
    assert_eq!((loaded.hover_action, loaded.click_action), (HoverAction::Card, ClickAction::Settings));
    assert_eq!(Settings::from_text("click_action=9").click_action, ClickAction::Devices, "an unknown number falls back");
    assert_eq!(Settings::from_text("click_action=1").click_action, ClickAction::Devices, "the old card loads as Devices");
}

#[test]
fn window_and_update_choices_survive_a_save_and_an_old_file_loads_the_defaults() {
    let fresh = Settings::from_text("poll_minutes=5\n");
    assert_eq!((fresh.window_theme, fresh.keeps_on_top, fresh.update_check, fresh.last_update_check), (0, false, UpdateCheck::Daily, 0));
    let chosen = Settings { window_theme: 2, keeps_on_top: true, update_check: UpdateCheck::Weekly, last_update_check: 1_795_000_000, ..Settings::default() };
    let loaded = Settings::from_text(&chosen.to_text());
    assert_eq!((loaded.window_theme, loaded.keeps_on_top, loaded.update_check, loaded.last_update_check), (2, true, UpdateCheck::Weekly, 1_795_000_000));
    assert_eq!(Settings::from_text("window_theme=7").window_theme, MAX_WINDOW_THEME, "an unknown theme is held to dark");
    assert_eq!(Settings::from_text("update_check=x").update_check, UpdateCheck::Daily);
}

#[test]
fn reset_this_page_puts_back_only_the_appearance_choices() {
    let mut settings = Settings { shows_percent_in_icon: true, menu_font_points: MENU_FONT_POINTS_MAX, window_theme: 1, poll_minutes: 20, hover_action: HoverAction::Card, ..Settings::default() };
    settings.renames.insert("mouse".into(), "My mouse".into());
    settings.reset_appearance();
    assert_eq!((settings.shows_percent_in_icon, settings.menu_font_points, settings.window_theme), (false, MENU_FONT_POINTS_DEFAULT, 0));
    assert_eq!((settings.poll_minutes, settings.hover_action, settings.renames.len()), (20, HoverAction::Card, 1), "other pages keep their choices");
}

#[test]
fn reset_all_keeps_only_when_updates_were_last_checked() {
    let mut settings = Settings { last_update_check: 123, update_check: UpdateCheck::Never, keeps_on_top: true, poll_minutes: 20, is_saved_to_disk: false, ..Settings::default() };
    settings.renames.insert("mouse".into(), "My mouse".into());
    settings.reset_all();
    assert_eq!(settings.last_update_check, 123);
    assert_eq!((settings.update_check, settings.keeps_on_top, settings.poll_minutes, settings.renames.len()), (UpdateCheck::Daily, false, 5, 0));
    assert!(!settings.is_saved_to_disk, "a preview stays a preview");
}