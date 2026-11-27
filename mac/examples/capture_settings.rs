//! Paints every settings page to PNG files without showing anything, using the demo devices in a throwaway home folder.
//! Run with: cargo run -p juicebar-mac --example capture_settings -- <output folder>

#[cfg(target_os = "macos")]
fn main() {
    use juicebar_mac::settings_ui;
    use juicebar_shared::demo::sample_devices;
    use juicebar_shared::settings::Settings;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let output = std::env::args().nth(1).expect("give an output folder");
    std::fs::create_dir_all(&output).expect("output folder");
    let home = std::env::temp_dir().join(format!("juicebar-capture-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("temp folder");
    std::env::set_var("HOME", &home);
    let mtm = MainThreadMarker::new().expect("runs on the main thread");
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let path = |name: &str| format!("{output}/{name}.png");

    let mut settings = Settings { is_saved_to_disk: false, ..Settings::default() };
    settings.renames.insert("demo:mouse".to_string(), "Work mouse".to_string());
    settings.hidden.push("demo:unknown".to_string());
    let mut devices = sample_devices(82, 15);
    devices[0].name = "Kreo Swarm 75 Wireless Mechanical Keyboard".to_string();
    settings_ui::open_for_checks(settings, devices);

    for (theme, is_dark) in [("dark", true), ("light", false)] {
        settings_ui::preview_theme(is_dark);
        for (page, name) in ["general", "appearance", "devices", "advanced", "about"].iter().enumerate() {
            settings_ui::show_page_for_checks(page);
            assert!(settings_ui::capture_to(&path(&format!("settings_{name}_{theme}"))));
        }
        settings_ui::show_page_for_checks(3);
        assert!(settings_ui::click_for_checks("ask_reset"));
        assert!(settings_ui::capture_to(&path(&format!("settings_reset_confirm_{theme}"))));
        assert!(settings_ui::click_for_checks("cancel_reset"));
        assert!(!settings_ui::is_reset_done_for_checks());
    }
    settings_ui::preview_theme(false);
    // Clicks change the window's own settings even with no app running behind it.
    assert!(settings_ui::click_for_checks("page:0"));
    assert!(settings_ui::click_for_checks("poll:6"));
    assert!(settings_ui::click_for_checks("page:1"));
    assert!(settings_ui::click_for_checks("percent"));
    assert!(settings_ui::click_for_checks("menu_font:10"));
    assert!(settings_ui::click_for_checks("page:3"));
    assert!(settings_ui::click_for_checks("hover:1"));
    assert!(settings_ui::click_for_checks("click:1"));
    let changed = settings_ui::settings_for_checks().expect("open");
    assert_eq!(changed.poll_minutes, 6);
    assert!(changed.shows_percent_in_icon);
    assert_eq!(changed.menu_font_points, 10);
    assert_eq!(changed.hover_action.number(), 1);
    assert_eq!(changed.click_action.number(), 2);
    assert!(settings_ui::capture_to(&path("settings_advanced_chosen_light")));
    assert!(settings_ui::click_for_checks("ask_reset"));
    assert!(settings_ui::click_for_checks("confirm_reset"));
    assert!(settings_ui::is_reset_done_for_checks());
    let reset = settings_ui::settings_for_checks().expect("open");
    assert_eq!(reset.poll_minutes, Settings::default().poll_minutes);
    assert!(reset.renames.is_empty() && reset.hidden.is_empty() && !reset.shows_percent_in_icon);
    assert!(reset.hover_action.number() == 0 && reset.click_action.number() == 0 && reset.menu_font_points == 9);
    assert!(settings_ui::capture_to(&path("settings_reset_done_light")));
    settings_ui::show_page_for_checks(2);
    assert!(settings_ui::capture_to(&path("settings_devices_after_reset_light")));
    let _ = std::fs::remove_dir_all(&home);
    println!("captured to {output}");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
