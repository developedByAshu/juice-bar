//! The native menu under the battery. Name on the left; connection and % on the right, in the tone colour.
//! Unknown devices get a submenu with Report and Try to detect; known ones show Report while ⌥ is held.

use crate::delegate::{delegate, main_thread};
use crate::theme::tone_color;
use juicebar_shared::device::{Connection, Device};
use juicebar_shared::tone::tone_for_percent;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, AnyThread, MainThreadOnly};
use objc2_app_kit::{
    NSColor, NSEventModifierFlags, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSMenu, NSMenuItem, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSTextAlignment, NSTextTab,
};
use objc2_foundation::{NSArray, NSDictionary, NSMutableAttributedString, NSRange, NSString};

/// Where the connection label and the % end, in points from the row's text start.
const CONNECTION_TAB: f64 = 210.0;
const PERCENT_TAB: f64 = 310.0;

/// Returns the connection label shown in each row.
pub fn connection_label(connection: Connection) -> &'static str {
    match connection {
        Connection::Dongle => "2.4 GHz",
        Connection::Bluetooth => "Bluetooth",
        Connection::Usb => "USB",
    }
}

/// Returns the right-hand text: "82%", "⚡15%", "Charging" on a cable, or "Not connected".
pub fn percent_label(device: &Device) -> String {
    match (device.connection, device.battery) {
        (_, Some(battery)) if battery.is_charging => format!("⚡{}%", battery.percent),
        (Connection::Usb, Some(battery)) => format!("⚡{}%", battery.percent),
        (Connection::Usb, None) => "⚡ Charging".to_string(),
        (_, Some(battery)) => format!("{}%", battery.percent),
        (_, None) if device.is_unrecognised() => String::new(),
        (_, None) => "Not connected".to_string(),
    }
}

fn utf16_length(text: &str) -> usize {
    text.encode_utf16().count()
}

fn color_range(text: &NSMutableAttributedString, color: &NSColor, start: usize, length: usize) {
    // SAFETY: the attribute name is AppKit's own and the value is a colour, as it expects.
    unsafe { text.addAttribute_value_range(NSForegroundColorAttributeName, color.as_ref(), NSRange::new(start, length)) };
}

/// Builds the row's styled title: name, then right-aligned connection and %.
fn row_title(device: &Device) -> Retained<NSMutableAttributedString> {
    let connection = connection_label(device.connection);
    let percent = percent_label(device);
    let plain = format!("{}\t{connection}\t{percent}", device.name);
    let text = NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &NSString::from_str(&plain));
    let whole = NSRange::new(0, utf16_length(&plain));
    let style = NSMutableParagraphStyle::new();
    let no_options = NSDictionary::new();
    // SAFETY: an empty options dictionary is valid for a text tab.
    let tabs = NSArray::from_retained_slice(&[CONNECTION_TAB, PERCENT_TAB].map(|location| unsafe {
        NSTextTab::initWithTextAlignment_location_options(NSTextTab::alloc(), NSTextAlignment::Right, location, &no_options)
    }));
    style.setTabStops(Some(&tabs));
    let font = NSFont::menuFontOfSize(0.0);
    // SAFETY: AppKit's own attribute names with the value types they expect.
    unsafe {
        text.addAttribute_value_range(NSParagraphStyleAttributeName, style.as_ref(), whole);
        text.addAttribute_value_range(NSFontAttributeName, font.as_ref(), whole);
    }
    let connection_start = utf16_length(&device.name) + 1;
    color_range(&text, &NSColor::secondaryLabelColor(), connection_start, utf16_length(connection));
    let percent_start = connection_start + utf16_length(connection) + 1;
    let percent_color = match device.battery {
        Some(battery) if device.connection != Connection::Usb && !battery.is_charging => tone_color(tone_for_percent(battery.percent)),
        Some(_) => NSColor::labelColor(),
        None => NSColor::tertiaryLabelColor(),
    };
    color_range(&text, &percent_color, percent_start, utf16_length(&percent));
    let digits = NSFont::monospacedDigitSystemFontOfSize_weight(font.pointSize(), unsafe { objc2_app_kit::NSFontWeightMedium });
    // SAFETY: as above.
    unsafe { text.addAttribute_value_range(NSFontAttributeName, digits.as_ref(), NSRange::new(percent_start, utf16_length(&percent))) };
    text
}

fn action_item(title: &str, action: objc2::runtime::Sel, key: &str, tag: isize) -> Retained<NSMenuItem> {
    // SAFETY: the action is one the delegate answers.
    let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(main_thread()), &NSString::from_str(title), Some(action), &NSString::from_str(key)) };
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app.
    unsafe { item.setTarget(Some(target)) };
    item.setTag(tag);
    item
}

/// Builds one device row, plus its ⌥ alternate or its submenu.
pub fn build_device_row(device: &Device, index: usize) -> Vec<Retained<NSMenuItem>> {
    let tag = index as isize;
    let row = action_item(&device.name, sel!(rowClicked:), "", tag);
    row.setAttributedTitle(Some(&row_title(device)));
    if device.is_unrecognised() {
        let submenu = NSMenu::new(main_thread());
        submenu.addItem(&action_item("Report device…", sel!(reportDevice:), "", tag));
        submenu.addItem(&action_item("Try to detect", sel!(tryToDetect:), "", tag));
        row.setSubmenu(Some(&submenu));
        return vec![row];
    }
    let alternate = action_item(&format!("Report {}…", device.name), sel!(reportDevice:), "", tag);
    alternate.setAlternate(true);
    alternate.setKeyEquivalentModifierMask(NSEventModifierFlags::Option);
    row.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
    vec![row, alternate]
}

/// Fills the menu: header, one row per device, then Settings and Quit with their shortcuts.
pub fn show_menu(menu: &NSMenu, devices: &[Device]) {
    menu.removeAllItems();
    let header = NSMenuItem::sectionHeaderWithTitle(&NSString::from_str("Juice Bar"), main_thread());
    menu.addItem(&header);
    if devices.is_empty() {
        let empty = NSMenuItem::new(main_thread());
        empty.setTitle(&NSString::from_str("No wireless mouse or keyboard found"));
        empty.setEnabled(false);
        menu.addItem(&empty);
    }
    for (index, device) in devices.iter().enumerate() {
        build_device_row(device, index).iter().for_each(|item| menu.addItem(item));
    }
    menu.addItem(&NSMenuItem::separatorItem(main_thread()));
    menu.addItem(&action_item("Settings…", sel!(openSettings:), ",", -1));
    // SAFETY: terminate: is answered by the application along the responder chain.
    let quit = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(main_thread()), &NSString::from_str("Quit Juice Bar"), Some(sel!(terminate:)), &NSString::from_str("q")) };
    menu.addItem(&quit);
}

/// Opens the prefilled GitHub issue for the device behind a row.
pub fn on_report(tag: isize) {
    if let Some(url) = crate::app::report_url_for(tag) {
        let url = objc2_foundation::NSURL::URLWithString(&NSString::from_str(&url));
        if let Some(url) = url {
            objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
}
