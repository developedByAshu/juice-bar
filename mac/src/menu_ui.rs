//! The menu under the battery, drawn the same way as NetSpeedGauge's popup menu: the same width, row
//! height, padding, icons, fonts, hover and dividers. Every right-hand thing (a %, the no-signal
//! symbol, a shortcut) ends on one shared right edge.
//!
//! Each row is its own menu item holding a drawn view, rather than one view for the whole menu, so
//! Settings and Quit keep their real key equivalents (⌘, and ⌘Q work while the menu is open) and the
//! arrow keys and Return move through the rows as in any Mac menu.

use crate::canvas::{self, mix, Align, Font, WEIGHT_MEDIUM, WEIGHT_REGULAR, WEIGHT_SEMIBOLD};
use crate::delegate::{delegate, main_thread};
use crate::theme::{dark_theme, light_theme, theme_for, tone_rgb, Theme};
use juicebar_shared::device::{Connection, Device, DeviceKind};
use juicebar_shared::names::short_name;
use juicebar_shared::tone::tone_for_percent;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAppearanceCustomization, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem, NSTrackingArea, NSTrackingAreaOptions, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::Cell;

const MENU_WIDTH: i32 = 164;
const SUMMARY_HEIGHT: i32 = 34;
const ROW_HEIGHT: i32 = 32;
const DIVIDER_HEIGHT: i32 = 9;
const PADDING: i32 = 5;
const HOVER_RADIUS: f32 = 5.0;
const ICON_POINTS: f64 = 10.0;
const ICON_CENTER_X: i32 = 22;
const LABEL_X: i32 = 40;
const LABEL_TOP: i32 = 7;
// The shared right edge: every %, symbol and shortcut ends this far from the menu's right side.
const RIGHT_INSET: i32 = 14;
// The least space between a name and the right column, on the longest row.
const NAME_GAP: i32 = 24;
const BOLT_GAP: i32 = 2;
const BOLT_POINTS: f64 = 8.0;
const ICON_BOLT: &str = "bolt.fill";
/// Shown in place of the % when a known device is off or out of range; the words stay in the tooltip.
const NOT_CONNECTED_SYMBOL: &str = "antenna.radiowaves.left.and.right.slash";
const NOT_CONNECTED_TEXT: &str = "Not connected";

/// What one row of the menu shows.
#[derive(Clone)]
enum MenuRow {
    Header,
    Device(Device),
    Empty,
    Divider,
    Settings,
    Quit,
}

/// What sits at a device row's right edge.
enum Trailing {
    Percent { text: String, color: u32, has_bolt: bool },
    NotConnected,
    Nothing,
}

struct MenuFonts {
    body: Font,
    strong: Font,
    small: Font,
    digits: Font,
}

impl MenuFonts {
    fn new() -> Self {
        Self {
            body: Font::system(10.0, WEIGHT_REGULAR),
            strong: Font::system(10.0, WEIGHT_SEMIBOLD),
            small: Font::system(9.0, WEIGHT_REGULAR),
            digits: Font::digits(10.0, WEIGHT_MEDIUM),
        }
    }
}

pub struct RowState {
    row: MenuRow,
    /// The row's place in the menu, header first, for checks that paint one row as selected.
    index: usize,
    is_mouse_inside: Cell<bool>,
}

thread_local! {
    // Captures paint a row as highlighted and paint the menu's own background, so a PNG shows both.
    static FORCED_HIGHLIGHT: Cell<Option<usize>> = const { Cell::new(None) };
    static PAINTS_BACKGROUND: Cell<bool> = const { Cell::new(false) };
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarMenuRowView"]
    #[ivars = RowState]
    pub struct MenuRowView;

    impl MenuRowView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().is_mouse_inside.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_mouse_inside.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let wants_report = event.modifierFlags().contains(NSEventModifierFlags::Option);
            self.clicked(wants_report);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, _event: &NSEvent) {
            self.clicked(true);
        }
    }
);

/// Returns the right-hand text: "82%", "15%" with a bolt when charging, "Charging" on a cable, or nothing.
pub fn percent_label(device: &Device) -> String {
    match (device.connection, device.battery) {
        (_, Some(battery)) => format!("{}%", battery.percent),
        (Connection::Usb, None) => "Charging".to_string(),
        (_, None) if device.is_unrecognised() => String::new(),
        (_, None) => NOT_CONNECTED_TEXT.to_string(),
    }
}

/// Returns the connection label used in the hover text.
pub fn connection_label(connection: Connection) -> &'static str {
    match connection {
        Connection::Dongle => "2.4 GHz",
        Connection::Bluetooth => "Bluetooth",
        Connection::Usb => "USB",
    }
}

/// Returns the hover text: full name and connection, plus "Not connected" when it is off.
fn row_tooltip(device: &Device) -> String {
    let base = format!("{} · {}", device.name, connection_label(device.connection));
    if device.is_unrecognised() {
        return format!("{base} · Click to try to detect, right-click or ⌥-click to report it");
    }
    if percent_label(device) == NOT_CONNECTED_TEXT {
        format!("{base} · {NOT_CONNECTED_TEXT}")
    } else {
        base
    }
}

fn trailing(device: &Device, theme: &Theme) -> Trailing {
    let label = percent_label(device);
    if label == NOT_CONNECTED_TEXT {
        return Trailing::NotConnected;
    }
    if label.is_empty() {
        return Trailing::Nothing;
    }
    let is_charging = device.connection == Connection::Usb || device.battery.is_some_and(|battery| battery.is_charging);
    let color = match device.battery {
        Some(battery) if !is_charging => tone_rgb(tone_for_percent(battery.percent), theme),
        _ => theme.text,
    };
    Trailing::Percent { text: label, color, has_bolt: is_charging }
}

/// Returns the width of the right column: the widest thing it can ever hold, a charging "100%" or a shortcut.
/// It does not depend on today's levels, so the menu keeps one width as batteries change.
fn right_column_width(fonts: &MenuFonts) -> i32 {
    let charging_full = canvas::symbol_width(ICON_BOLT, BOLT_POINTS) + BOLT_GAP + canvas::text_width("100%", &fonts.digits);
    let shortcut = ["⌘,", "⌘Q"].iter().map(|key| canvas::text_width(key, &fonts.body)).max().unwrap_or(0);
    charging_full.max(shortcut).max(canvas::symbol_width(NOT_CONNECTED_SYMBOL, ICON_POINTS))
}

/// Returns the name as it fits in `room`: the menu's short name, cut further only if still too wide.
fn fitted_name(name: &str, font: &Font, room: i32) -> String {
    let mut shown = short_name(name);
    let mut kept: Vec<char> = shown.trim_end_matches('…').chars().collect();
    while canvas::text_width(&shown, font) > room && kept.len() > 1 {
        kept.pop();
        shown = format!("{}…", kept.iter().collect::<String>().trim_end());
    }
    shown
}

fn kind_icon(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Keyboard => "keyboard",
        _ => "computermouse",
    }
}

impl MenuRowView {
    fn create(row: MenuRow, index: usize, width: i32) -> Retained<Self> {
        let height = match row {
            MenuRow::Header => SUMMARY_HEIGHT + PADDING,
            MenuRow::Divider => DIVIDER_HEIGHT,
            // The last row carries the menu's bottom padding, as in NetSpeedGauge.
            MenuRow::Quit => ROW_HEIGHT + PADDING,
            _ => ROW_HEIGHT,
        };
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(width), f64::from(height)));
        let this = Self::alloc(main_thread()).set_ivars(RowState { row, index, is_mouse_inside: Cell::new(false) });
        // SAFETY: initWithFrame: is NSView's designated initialiser.
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let options = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
        let owner: &AnyObject = view.as_ref();
        // SAFETY: the view owns the area, and AppKit keeps its owner reference only while the area is attached.
        let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), NSRect::ZERO, options, Some(owner), None) };
        view.addTrackingArea(&area);
        view
    }

    /// Checks whether the row shows as selected: the menu's own keyboard or mouse highlight.
    fn is_highlighted(&self) -> bool {
        if let Some(forced) = FORCED_HIGHLIGHT.with(Cell::get) {
            return forced == self.ivars().index;
        }
        let Some(item) = self.enclosingMenuItem() else {
            return false;
        };
        item.isEnabled() && (item.isHighlighted() || self.ivars().is_mouse_inside.get())
    }

    fn draw(&self) {
        let theme = theme_for(&self.effectiveAppearance());
        let fonts = MenuFonts::new();
        let size = self.frame().size;
        let (width, height) = (size.width as i32, size.height as i32);
        if PAINTS_BACKGROUND.with(Cell::get) {
            canvas::round_rect(0, 0, width, height, 0.0, theme.surface);
        }
        let right = width - RIGHT_INSET;
        let divider_width = width - PADDING * 2;
        let is_highlighted = self.is_highlighted();
        if is_highlighted {
            canvas::round_rect(PADDING, 0, divider_width, ROW_HEIGHT, HOVER_RADIUS, mix(theme.surface, theme.text, 0.09));
        }
        let label = |icon: &str, text: &str| {
            canvas::symbol(ICON_CENTER_X, ROW_HEIGHT / 2, icon, ICON_POINTS, if is_highlighted { theme.text } else { theme.muted });
            let font = if is_highlighted { &fonts.strong } else { &fonts.body };
            canvas::text(LABEL_X, LABEL_TOP, text, font, theme.text, Align::Left);
        };
        match &self.ivars().row {
            MenuRow::Header => {
                canvas::text(14, 9, "Juice Bar", &fonts.small, theme.muted, Align::Left);
                canvas::round_rect(PADDING, SUMMARY_HEIGHT, divider_width, 1, 0.0, theme.border);
            }
            MenuRow::Divider => canvas::round_rect(PADDING, DIVIDER_HEIGHT / 2, divider_width, 1, 0.0, theme.border),
            MenuRow::Empty => canvas::text(LABEL_X, LABEL_TOP, "No devices found", &fonts.body, theme.muted, Align::Left),
            MenuRow::Settings => {
                label("gearshape", "Settings");
                canvas::text_ink_right(right, LABEL_TOP, "⌘,", &fonts.body, theme.muted);
            }
            MenuRow::Quit => {
                label("power", "Quit");
                canvas::text_ink_right(right, LABEL_TOP, "⌘Q", &fonts.body, theme.muted);
            }
            MenuRow::Device(device) => {
                let font = if is_highlighted { &fonts.strong } else { &fonts.body };
                let room = right - right_column_width(&fonts) - NAME_GAP - LABEL_X;
                label(kind_icon(device.kind), &fitted_name(&device.name, font, room));
                match trailing(device, &theme) {
                    Trailing::Percent { text, color, has_bolt } => {
                        canvas::text_ink_right(right, LABEL_TOP, &text, &fonts.digits, color);
                        if has_bolt {
                            let bolt_width = canvas::symbol_width(ICON_BOLT, BOLT_POINTS);
                            let bolt_x = right - canvas::text_width(&text, &fonts.digits) - BOLT_GAP - bolt_width / 2;
                            canvas::symbol(bolt_x, ROW_HEIGHT / 2, ICON_BOLT, BOLT_POINTS, color);
                        }
                    }
                    Trailing::NotConnected => {
                        canvas::symbol_right(right, ROW_HEIGHT / 2, NOT_CONNECTED_SYMBOL, ICON_POINTS, theme.muted);
                    }
                    Trailing::Nothing => {}
                }
            }
        }
    }

    /// Runs the row's item, or Report for a device on right-click or ⌥-click, then closes the menu.
    fn clicked(&self, wants_report: bool) {
        let Some(item) = self.enclosingMenuItem() else {
            return;
        };
        // SAFETY: the item is alive while its view handles the click, so its menu is too.
        let Some(menu) = (unsafe { item.menu() }) else {
            return;
        };
        match &self.ivars().row {
            MenuRow::Device(_) if wants_report => {
                menu.cancelTracking();
                on_report(item.tag());
            }
            MenuRow::Header | MenuRow::Divider | MenuRow::Empty => {}
            _ if wants_report => {}
            _ => {
                let index = menu.indexOfItem(&item);
                menu.cancelTracking();
                menu.performActionForItemAtIndex(index);
            }
        }
    }
}

fn action_item(action: Option<Sel>, key: &str, tag: isize) -> Retained<NSMenuItem> {
    // SAFETY: each action is one the delegate or the application answers.
    let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(main_thread()), &NSString::new(), action, &NSString::from_str(key)) };
    item.setTag(tag);
    item
}

fn targeted_item(action: Sel, key: &str, tag: isize) -> Retained<NSMenuItem> {
    let item = action_item(Some(action), key, tag);
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app.
    unsafe { item.setTarget(Some(target)) };
    item
}

/// Returns the menu width: the left inset and icon, the longest name in its hover weight, the gap, the
/// right column and the right inset. Never narrower than NetSpeedGauge's menu.
fn menu_width(devices: &[Device]) -> i32 {
    let fonts = MenuFonts::new();
    let longest = devices.iter().map(|device| canvas::text_width(&short_name(&device.name), &fonts.strong)).max().unwrap_or(0);
    (LABEL_X + longest + NAME_GAP + right_column_width(&fonts) + RIGHT_INSET).max(MENU_WIDTH)
}

/// Returns the space between the longest name and the right column, for checks.
pub fn name_gap_for_checks(devices: &[Device]) -> i32 {
    let fonts = MenuFonts::new();
    let width = menu_width(devices);
    let longest = devices.iter().map(|device| canvas::text_width(&short_name(&device.name), &fonts.body)).max().unwrap_or(0);
    width - RIGHT_INSET - right_column_width(&fonts) - (LABEL_X + longest)
}

/// Fills the menu: header, one row per device, a divider, then Settings and Quit with their shortcuts.
pub fn show_menu(menu: &NSMenu, devices: &[Device]) {
    menu.removeAllItems();
    let width = menu_width(devices);
    let add = |item: Retained<NSMenuItem>, row: MenuRow, tooltip: Option<String>| {
        let view = MenuRowView::create(row, menu.numberOfItems() as usize, width);
        if let Some(text) = tooltip {
            let text = NSString::from_str(&text);
            item.setToolTip(Some(&text));
            view.setToolTip(Some(&text));
        }
        item.setView(Some(&view));
        menu.addItem(&item);
    };
    add(action_item(None, "", -1), MenuRow::Header, None);
    if devices.is_empty() {
        add(action_item(None, "", -1), MenuRow::Empty, None);
    }
    for (index, device) in devices.iter().enumerate() {
        // An unknown device runs Try to detect on click; a known one opens Settings.
        let action = if device.is_unrecognised() { sel!(tryToDetect:) } else { sel!(rowClicked:) };
        add(targeted_item(action, "", index as isize), MenuRow::Device(device.clone()), Some(row_tooltip(device)));
    }
    add(action_item(None, "", -1), MenuRow::Divider, None);
    add(targeted_item(sel!(openSettings:), ",", -1), MenuRow::Settings, None);
    add(action_item(Some(sel!(terminate:)), "q", -1), MenuRow::Quit, None);
}

/// Opens the prefilled GitHub issue for the device behind a row.
pub fn on_report(tag: isize) {
    if let Some(url) = crate::app::report_url_for(tag) {
        if let Some(url) = objc2_foundation::NSURL::URLWithString(&NSString::from_str(&url)) {
            objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
}

/// Paints the menu's rows, stacked as the menu shows them, to a PNG in a chosen look, for checks.
/// `highlight` paints that row (counting from the header as 0) as selected.
pub fn capture_menu_for_checks(devices: &[Device], path: &str, is_dark: bool, highlight: Option<usize>) -> bool {
    let menu = NSMenu::new(main_thread());
    show_menu(&menu, devices);
    let views: Vec<Retained<NSView>> = menu.itemArray().iter().filter_map(|item| item.view()).collect();
    menu.itemArray().iter().for_each(|item| item.setView(None));
    let width = views.first().map_or(MENU_WIDTH, |view| view.frame().size.width as i32);
    let height: f64 = views.iter().map(|view| view.frame().size.height).sum();
    let container = NSView::initWithFrame(NSView::alloc(main_thread()), NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(width), height)));
    let theme = if is_dark { dark_theme() } else { light_theme() };
    container.setAppearance(crate::theme::appearance_for(&theme).as_deref());
    let mut top = height;
    for view in &views {
        top -= view.frame().size.height;
        view.setFrameOrigin(NSPoint::new(0.0, top));
        container.addSubview(view);
    }
    FORCED_HIGHLIGHT.with(|cell| cell.set(highlight));
    PAINTS_BACKGROUND.with(|cell| cell.set(true));
    let painted = crate::surface::capture_view(&container, path);
    FORCED_HIGHLIGHT.with(|cell| cell.set(None));
    PAINTS_BACKGROUND.with(|cell| cell.set(false));
    painted
}

/// Paints the open menu's rows to a PNG as the Mac shows them, for capture mode.
pub fn capture_open_menu(menu: &NSMenu, path: &str) -> bool {
    let frame = menu.itemArray().iter().find_map(|item| item.view()).and_then(|view| view.window()).and_then(|window| window.contentView());
    // The menu's see-through material paints as blank in a capture, so the rows paint their own surface.
    PAINTS_BACKGROUND.with(|cell| cell.set(true));
    let painted = frame.is_some_and(|view| crate::surface::capture_view(&view, path));
    PAINTS_BACKGROUND.with(|cell| cell.set(false));
    painted
}
