//! Builds the real device menu offscreen from the demo devices plus an offline one, and prints its size.
//! Usage: cargo run -p juicebar-mac --example menu_width

#[cfg(target_os = "macos")]
fn main() {
    use juicebar_mac::delegate::main_thread;
    use juicebar_shared::demo::sample_devices;
    use objc2_app_kit::{NSApplication, NSMenu};
    use objc2::AnyThread;

    let _app = NSApplication::sharedApplication(main_thread());
    let mut devices = sample_devices(82, 15);
    let mut offline = devices[0].clone();
    offline.id = "demo:offline".to_string();
    offline.battery = None;
    devices.push(offline);
    let menu = NSMenu::new(main_thread());
    juicebar_mac::menu_ui::show_menu(&menu, &devices);
    let size = menu.size();
    println!("menu {:.0} x {:.0} pt", size.width, size.height);
    use objc2_app_kit::{NSAttributedStringNSStringDrawing, NSFont, NSFontAttributeName};
    use objc2_foundation::{NSAttributedString, NSDictionary, NSString};
    let font = NSFont::menuFontOfSize(12.0);
    let attrs = unsafe { NSDictionary::from_slices(&[NSFontAttributeName], &[font.as_ref()]) };
    for text in ["Kreo Swarm 75", "\u{26A1}100%", "Kreo Swarm 75  \u{26A1}100%"] {
        let measured = unsafe { NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &NSString::from_str(text), Some(&attrs)) };
        println!("{text}: {:.0} pt", measured.size().width);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
