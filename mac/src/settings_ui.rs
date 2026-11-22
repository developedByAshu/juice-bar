//! The settings window, built from standard AppKit controls so it looks and behaves like any Mac app:
//! General (launch at login, % next to the icon, poll interval), Devices (rename, show), Reset, About.

use crate::confirm_sheet::show_confirm;
use crate::delegate::{delegate, main_thread, start_timer};
use crate::startup;
use juicebar_shared::device::Device;
use juicebar_shared::settings::Settings;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSFont, NSLayoutAttribute, NSPopUpButton, NSStackView,
    NSStackViewDistribution, NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;

const TAG_LAUNCH_AT_LOGIN: isize = 1;
const TAG_SHOW_PERCENT: isize = 2;
const TAG_POLL: isize = 3;
const TAG_RENAME_BASE: isize = 100;
const TAG_SHOW_BASE: isize = 200;
const POLL_CHOICES: [u32; 6] = [1, 2, 5, 10, 15, 30];
const WINDOW_WIDTH: f64 = 460.0;
const RESET_NOTE_SECONDS: f64 = 4.0;

struct SettingsWindow {
    window: Retained<NSWindow>,
    devices: Vec<Device>,
    reset_note: Retained<NSTextField>,
}

thread_local! {
    static WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
}

fn label(text: &str, is_heading: bool) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), main_thread());
    if is_heading {
        field.setFont(Some(&NSFont::boldSystemFontOfSize(NSFont::systemFontSize())));
    } else {
        field.setTextColor(Some(&objc2_app_kit::NSColor::secondaryLabelColor()));
    }
    field
}

fn checkbox(title: &str, is_on: bool, tag: isize) -> Retained<NSButton> {
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app and answers settingChanged:.
    let button = unsafe { NSButton::checkboxWithTitle_target_action(&NSString::from_str(title), Some(target), Some(sel!(settingChanged:)), main_thread()) };
    button.setState(if is_on { NSControlStateValueOn } else { NSControlStateValueOff });
    button.setTag(tag);
    button
}

fn row(views: &[&NSView]) -> Retained<NSStackView> {
    let stack = NSStackView::new(main_thread());
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(8.0);
    views.iter().for_each(|view| stack.addArrangedSubview(view));
    stack
}

fn poll_popup(minutes: u32) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::new(main_thread());
    for choice in POLL_CHOICES {
        let title = if choice == 1 { "1 minute".to_string() } else { format!("{choice} minutes") };
        popup.addItemWithTitle(&NSString::from_str(&title));
    }
    let index = POLL_CHOICES.iter().position(|choice| *choice == minutes).unwrap_or(2);
    popup.selectItemAtIndex(index as isize);
    popup.setTag(TAG_POLL);
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app.
    unsafe {
        popup.setTarget(Some(target));
        popup.setAction(Some(sel!(settingChanged:)));
    }
    popup
}

/// Builds the window's content from the current settings and rows.
fn build_content(settings: &Settings, devices: &[Device]) -> (Retained<NSStackView>, Retained<NSTextField>) {
    let stack = NSStackView::new(main_thread());
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setDistribution(NSStackViewDistribution::Fill);
    stack.setSpacing(10.0);
    stack.setEdgeInsets(NSEdgeInsets { top: 20.0, left: 24.0, bottom: 20.0, right: 24.0 });
    stack.addArrangedSubview(&label("General", true));
    stack.addArrangedSubview(&checkbox("Launch at login", startup::is_enabled(), TAG_LAUNCH_AT_LOGIN));
    stack.addArrangedSubview(&checkbox("Show the lowest % next to the icon", settings.shows_percent_in_icon, TAG_SHOW_PERCENT));
    stack.addArrangedSubview(&row(&[&label("Check batteries every", false), &poll_popup(settings.poll_minutes)]));
    stack.addArrangedSubview(&label("Devices", true));
    if devices.is_empty() {
        stack.addArrangedSubview(&label("No wireless mouse or keyboard found yet.", false));
    }
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    for (index, device) in devices.iter().enumerate() {
        let name = NSTextField::textFieldWithString(&NSString::from_str(settings.renames.get(&device.id).map_or("", String::as_str)), main_thread());
        name.setPlaceholderString(Some(&NSString::from_str(&device.name)));
        name.setTag(TAG_RENAME_BASE + index as isize);
        // SAFETY: the delegate lives for the whole app.
        unsafe {
            name.setTarget(Some(target));
            name.setAction(Some(sel!(settingChanged:)));
        }
        name.setFrameSize(NSSize::new(240.0, 22.0));
        let width = name.widthAnchor().constraintEqualToConstant(240.0);
        width.setActive(true);
        let shown = checkbox("Show", !settings.hidden.contains(&device.id), TAG_SHOW_BASE + index as isize);
        stack.addArrangedSubview(&row(&[&name, &shown]));
    }
    stack.addArrangedSubview(&label("Reset", true));
    // SAFETY: the delegate lives for the whole app.
    let reset = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str("Reset all settings…"), Some(target), Some(sel!(resetClicked:)), main_thread()) };
    let reset_note = label("", false);
    reset_note.setTextColor(Some(&objc2_app_kit::NSColor::systemGreenColor()));
    stack.addArrangedSubview(&row(&[&reset, &reset_note]));
    stack.addArrangedSubview(&label(&format!("Juice Bar {} · github.com/developedByAshu/juice-bar", juicebar_shared::APP_VERSION), false));
    (stack, reset_note)
}

/// Opens the settings window, or brings it forward if it is open.
pub fn open_settings() {
    let mtm = main_thread();
    NSApplication::sharedApplication(mtm).activate();
    if WINDOW.with(|cell| cell.borrow().as_ref().map(|open| open.window.makeKeyAndOrderFront(None)).is_some()) {
        return;
    }
    let Some((settings, devices)) = crate::app::settings_and_devices() else {
        return;
    };
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_WIDTH, 300.0));
    // SAFETY: standard window creation on the main thread.
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, style, NSBackingStoreType::Buffered, false) };
    // SAFETY: the window is owned by WINDOW, so AppKit must not free it on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Juice Bar Settings"));
    let (content, reset_note) = build_content(&settings, &devices);
    window.setContentView(Some(&content));
    window.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*delegate())));
    window.center();
    window.makeKeyAndOrderFront(None);
    WINDOW.with(|cell| *cell.borrow_mut() = Some(SettingsWindow { window, devices, reset_note }));
}

/// Forgets the window once it closes, so the next open shows fresh rows.
pub fn on_closed() {
    WINDOW.with(|cell| cell.borrow_mut().take());
}

/// Reads one changed control into the settings and applies them.
pub fn on_control_changed(sender: &AnyObject) {
    let Some((mut settings, _)) = crate::app::settings_and_devices() else {
        return;
    };
    let devices = WINDOW.with(|cell| cell.borrow().as_ref().map(|open| open.devices.clone())).unwrap_or_default();
    // SAFETY: every control that sends settingChanged: is an NSControl.
    let control: &objc2_app_kit::NSControl = unsafe { &*(sender as *const AnyObject).cast() };
    let tag = control.tag();
    match tag {
        TAG_LAUNCH_AT_LOGIN => startup::set_enabled(!startup::is_enabled()),
        TAG_SHOW_PERCENT => settings.shows_percent_in_icon = !settings.shows_percent_in_icon,
        TAG_POLL => {
            // SAFETY: the TAG_POLL control is the popup built above.
            let popup: &NSPopUpButton = unsafe { &*(sender as *const AnyObject).cast() };
            settings.poll_minutes = POLL_CHOICES.get(popup.indexOfSelectedItem() as usize).copied().unwrap_or(5);
        }
        _ if (TAG_RENAME_BASE..TAG_SHOW_BASE).contains(&tag) => {
            if let Some(device) = devices.get((tag - TAG_RENAME_BASE) as usize) {
                let text = control.stringValue().to_string();
                if text.trim().is_empty() {
                    settings.renames.remove(&device.id);
                } else {
                    settings.renames.insert(device.id.clone(), text.trim().to_string());
                }
            }
        }
        _ if tag >= TAG_SHOW_BASE => {
            if let Some(device) = devices.get((tag - TAG_SHOW_BASE) as usize) {
                if settings.hidden.contains(&device.id) {
                    settings.hidden.retain(|id| id != &device.id);
                } else {
                    settings.hidden.push(device.id.clone());
                }
            }
        }
        _ => return,
    }
    crate::app::apply_settings(settings);
}

/// Asks inside the window before resetting, then resets, rebuilds the controls and shows a short note.
pub fn on_reset_clicked() {
    let Some(window) = WINDOW.with(|cell| cell.borrow().as_ref().map(|open| open.window.clone())) else {
        return;
    };
    show_confirm(&window, "Reset all settings?", "Names, hidden devices, the check interval and the % next to the icon go back to their defaults.", "Reset", || {
        let fresh = Settings::default();
        crate::app::apply_settings(fresh.clone());
        WINDOW.with(|cell| {
            if let Some(open) = cell.borrow_mut().as_mut() {
                let (content, note) = build_content(&fresh, &open.devices);
                open.window.setContentView(Some(&content));
                note.setStringValue(&NSString::from_str("✓ All settings are back to their defaults"));
                open.reset_note = note;
            }
        });
        start_timer(RESET_NOTE_SECONDS, sel!(hideResetNote:), false);
    });
}

/// Clears the reset note after a few seconds.
pub fn hide_reset_note() {
    WINDOW.with(|cell| {
        if let Some(open) = cell.borrow().as_ref() {
            open.reset_note.setStringValue(&NSString::from_str(""));
        }
    });
}
