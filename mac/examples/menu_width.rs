//! Builds the real device menu offscreen from the demo devices plus an offline one, at the smallest,
//! default and largest menu text sizes. Prints the width and the gap after the longest name, paints it
//! light and dark, and checks that every right-hand %, symbol and shortcut ends on one pixel.
//! Usage: cargo run -p juicebar-mac --example menu_width -- <output folder>

#[cfg(target_os = "macos")]
fn main() {
    use juicebar_mac::delegate::main_thread;
    use juicebar_mac::menu_ui::{capture_menu_for_checks, name_gap_for_checks, show_menu};
    use juicebar_shared::demo::sample_devices;
    use juicebar_shared::device::{Battery, DeviceKind};
    use juicebar_shared::settings::{MENU_FONT_POINTS_DEFAULT, MENU_FONT_POINTS_MAX, MENU_FONT_POINTS_MIN};
    use objc2_app_kit::{NSApplication, NSBitmapImageRep, NSEvent, NSEventModifierFlags, NSEventType, NSMenu};
    use objc2_foundation::{NSData, NSPoint, NSString};

    let output = std::env::args().nth(1).unwrap_or_else(|| std::env::temp_dir().display().to_string());
    let _app = NSApplication::sharedApplication(main_thread());
    let mut devices = sample_devices(82, 15);
    // The longest row the owner checks: Kreo Swarm 75 charging at 100%.
    devices[0].battery = Some(Battery { percent: 100, is_charging: true });
    let mut offline = devices[0].clone();
    offline.id = "demo:offline".to_string();
    offline.battery = None;
    devices.push(offline);
    let mut green = devices[0].clone();
    green.id = "demo:green".to_string();
    green.name = "MX Master 3S".to_string();
    green.kind = DeviceKind::Mouse;
    green.battery = Some(Battery { percent: 100, is_charging: false });
    devices.push(green);

    let menu = NSMenu::new(main_thread());
    show_menu(&menu, &devices, MENU_FONT_POINTS_DEFAULT);
    // Command and comma must reach the hidden Settings item while the menu is open.
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::ZERO,
        NSEventModifierFlags::Command,
        0.0,
        0,
        None,
        &NSString::from_str(","),
        &NSString::from_str(","),
        false,
        43,
    )
    .expect("key event");
    println!("command comma matched: {}", menu.performKeyEquivalent(&event));

    for points in [MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_DEFAULT, MENU_FONT_POINTS_MAX] {
        let (width, gap, least) = name_gap_for_checks(&devices, points);
        println!("{points} pt: menu {width} pt wide, gap after the longest name {gap} pt (least allowed {least})");
        assert!(gap >= least);
        for (theme, is_dark) in [("light", false), ("dark", true)] {
            for (suffix, highlight) in [("", None), ("_hover", Some(2))] {
                let path = format!("{output}/menu_{points}pt_{theme}{suffix}.png");
                assert!(capture_menu_for_checks(&devices, points, &path, is_dark, highlight));
            }
            let path = format!("{output}/menu_{points}pt_{theme}.png");
            let data = NSData::dataWithContentsOfFile(&NSString::from_str(&path)).expect("png");
            let image = NSBitmapImageRep::imageRepWithData(&data).expect("bitmap");
            let (wide, high) = (image.pixelsWide(), image.pixelsHigh());
            let background = image.colorAtX_y(1, 1).expect("pixel");
            let is_ink = |x: isize, y: isize| {
                let pixel = image.colorAtX_y(x, y).expect("pixel");
                (pixel.redComponent() - background.redComponent()).abs()
                    + (pixel.greenComponent() - background.greenComponent()).abs()
                    + (pixel.blueComponent() - background.blueComponent()).abs()
                    > 0.25
            };
            // Each band of lines with ink in the right third is one right-column item; note where its ink ends.
            // Bands one or two pixels tall are the divider lines, which run the menu's full width.
            let column = wide * 2 / 3;
            let mut edges = Vec::new();
            let mut band: Option<(isize, isize)> = None;
            for y in 0..=high {
                let right = if y < high { (column..wide).rev().find(|&x| is_ink(x, y)) } else { None };
                match (right, band) {
                    (Some(x), Some((edge, tall))) => band = Some((edge.max(x + 1), tall + 1)),
                    (Some(x), None) => band = Some((x + 1, 1)),
                    (None, Some((edge, tall))) => {
                        if tall > 2 {
                            edges.push(edge);
                        }
                        band = None;
                    }
                    (None, None) => {}
                }
            }
            println!("  {theme}: right column ink ends at pixels {edges:?} of {wide}");
            assert!(edges.windows(2).all(|pair| pair[0] == pair[1]), "right column is not on one edge");
        }
    }
    println!("saved to {output}");

    // With no devices the note must show whole: its ink ends before the menu's right padding.
    for points in [MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_DEFAULT, MENU_FONT_POINTS_MAX] {
        let path = format!("{output}/menu_empty_{points}pt.png");
        assert!(capture_menu_for_checks(&[], points, &path, true, None));
        let data = NSData::dataWithContentsOfFile(&NSString::from_str(&path)).expect("png");
        let image = NSBitmapImageRep::imageRepWithData(&data).expect("bitmap");
        let (wide, high) = (image.pixelsWide(), image.pixelsHigh());
        let background = image.colorAtX_y(1, 1).expect("pixel");
        let is_ink = |x: isize, y: isize| {
            let pixel = image.colorAtX_y(x, y).expect("pixel");
            (pixel.redComponent() - background.redComponent()).abs() + (pixel.greenComponent() - background.greenComponent()).abs() + (pixel.blueComponent() - background.blueComponent()).abs() > 0.25
        };
        let right_ink = (0..wide).rev().find(|&x| (0..high).any(|y| is_ink(x, y))).unwrap_or(0);
        println!("{points} pt empty menu: {wide} px wide, ink ends at px {right_ink}");
        assert!(right_ink < wide - 4, "the empty note runs into the right edge");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
