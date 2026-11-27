//! Builds the real device menu offscreen from the demo devices plus an offline one, prints its size,
//! paints it light and dark, and checks that every right-hand %, symbol and shortcut ends on one edge.
//! Usage: cargo run -p juicebar-mac --example menu_width -- <output folder>

#[cfg(target_os = "macos")]
fn main() {
    use juicebar_mac::delegate::main_thread;
    use juicebar_mac::menu_ui::{capture_menu_for_checks, show_menu};
    use juicebar_shared::demo::sample_devices;
    use objc2_app_kit::{NSApplication, NSBitmapImageRep, NSEvent, NSEventModifierFlags, NSEventType, NSMenu};
    use objc2_foundation::{NSData, NSPoint, NSString};

    let output = std::env::args().nth(1).unwrap_or_else(|| std::env::temp_dir().display().to_string());
    let _app = NSApplication::sharedApplication(main_thread());
    let mut devices = sample_devices(82, 15);
    let mut offline = devices[0].clone();
    offline.id = "demo:offline".to_string();
    offline.name = "Kreo Swarm 75".to_string();
    offline.battery = None;
    devices.push(offline);
    let mut green = devices[0].clone();
    green.id = "demo:green".to_string();
    green.name = "MX Master 3S".to_string();
    green.kind = juicebar_shared::device::DeviceKind::Mouse;
    green.battery = Some(juicebar_shared::device::Battery { percent: 100, is_charging: false });
    devices.push(green);

    // The longest row the owner checks: Kreo Swarm 75 charging at 100%.
    devices[0].battery = Some(juicebar_shared::device::Battery { percent: 100, is_charging: true });
    let menu = NSMenu::new(main_thread());
    show_menu(&menu, &devices);
    let gap = juicebar_mac::menu_ui::name_gap_for_checks(&devices);
    println!("gap between the longest name and the right column: {gap} pt");
    assert!(gap >= 24);
    let size = menu.size();
    println!("menu {:.0} x {:.0} pt, {} items", size.width, size.height, menu.numberOfItems());

    // Command and comma must reach the Settings item while the menu is open.
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
    let quit = menu.itemArray().iter().find(|item| item.keyEquivalent().to_string() == "q").map(|item| item.action());
    println!("quit item key q, action {:?}", quit.flatten());

    for (theme, is_dark) in [("light", false), ("dark", true)] {
        for (suffix, highlight) in [("", None), ("_hover", Some(2))] {
            let path = format!("{output}/menu_{theme}{suffix}.png");
            assert!(capture_menu_for_checks(&devices, &path, is_dark, highlight));
            println!("saved {path}");
        }
        // Find each row's rightmost ink, in points, from the 2x bitmap.
        let path = format!("{output}/menu_{theme}.png");
        let data = NSData::dataWithContentsOfFile(&NSString::from_str(&path)).expect("png");
        let image = NSBitmapImageRep::imageRepWithData(&data).expect("bitmap");
        let (width, height) = (image.pixelsWide(), image.pixelsHigh());
        let scale = width as f64 / image.size().width;
        let background = image.colorAtX_y(1, 1).expect("pixel");
        let differs = |x: isize, y: isize| {
            let pixel = image.colorAtX_y(x, y).expect("pixel");
            let gap = (pixel.redComponent() - background.redComponent()).abs() + (pixel.greenComponent() - background.greenComponent()).abs() + (pixel.blueComponent() - background.blueComponent()).abs();
            gap > 0.25
        };
        let mut edges = Vec::new();
        let row_pixels = (32.0 * scale) as isize;
        let first = ((34.0 + 5.0) * scale) as isize;
        let after_devices = first + row_pixels * devices.len() as isize;
        let mut tops: Vec<isize> = (0..devices.len() as isize).map(|row| first + row * row_pixels).collect();
        tops.push(after_devices + (9.0 * scale) as isize);
        tops.push(after_devices + ((9.0 + 32.0) * scale) as isize);
        for row_top in tops {
            let right = (width / 2..width).rev().find(|&x| (row_top..row_top + row_pixels).any(|y| y < height && differs(x, y)));
            edges.push(right.map(|x| x + 1));
        }
        println!("{theme}: right edge of each row in pixels: {edges:?}");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
