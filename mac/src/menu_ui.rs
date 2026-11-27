//! The menu under the battery, drawn the same way as NetSpeedGauge's popup menu: the same width, row
//! height, padding, icons, fonts, hover and dividers. Every right-hand thing (a %, the no-signal
//! symbol, a shortcut) ends on one shared right edge.
//!
//! Every row draws inside one panel item, sized from the menu text size setting, and the panel places
//! the menu window where NetSpeedGauge places its own: touching the menu bar, under the icon.

use crate::canvas::{self, mix, Align, Font, WEIGHT_MEDIUM, WEIGHT_REGULAR, WEIGHT_SEMIBOLD};
use crate::delegate::{delegate, main_thread};
use crate::theme::{dark_theme, light_theme, theme_for, tone_rgb, Theme};
use juicebar_shared::device::{Connection, Device, DeviceKind};
use juicebar_shared::names::short_name;
use juicebar_shared::settings::{MENU_FONT_POINTS_DEFAULT, MENU_FONT_POINTS_MAX, MENU_FONT_POINTS_MIN};
use juicebar_shared::tone::tone_for_percent;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAppearanceCustomization, NSAutoresizingMaskOptions, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};

// NetSpeedGauge's menu numbers, the geometry at full size. The menu draws them at MENU_SCALE when the
// text is the default size, and in proportion to the text size otherwise.
/// The note shown when no wireless mouse or keyboard is found.
const EMPTY_TEXT: &str = "No devices found";
const MENU_WIDTH: f64 = 164.0;
const SUMMARY_HEIGHT: f64 = 34.0;
const ROW_HEIGHT: f64 = 32.0;
const PADDING: f64 = 5.0;
const HOVER_RADIUS: f64 = 5.0;
const ICON_POINTS: f64 = 10.0;
const ICON_CENTER_X: f64 = 22.0;
const LABEL_X: f64 = 40.0;
const HEADER_X: f64 = 14.0;
// The shared right edge: every %, symbol and shortcut ends this far from the menu's right side.
const RIGHT_INSET: f64 = 14.0;
// The least space between a name and the right column, on the longest row.
const NAME_GAP: f64 = 24.0;
const BOLT_GAP: f64 = 2.0;
const BOLT_POINTS: f64 = 8.0;
// The menu is 80% of NetSpeedGauge's at the default text size.
const MENU_SCALE: f64 = 0.8;
// Header text is one point under the rows' text, as NetSpeedGauge's "Today" is.
const HEADER_POINTS_BELOW: f64 = 1.0;
// The Mac's own panels, measured on Weather, start one point above the bottom of the menu bar.
const MENU_OVERLAP_WITH_BAR: f64 = 1.0;
// The selected background ends 1 point past the item, and 2 more makes the rounded corner look lined up with it.
const MENU_PAST_ITEM: f64 = 3.0;
const ICON_BOLT: &str = "bolt.fill";
/// Shown in place of the % when a known device is off or out of range; the words stay in the tooltip.
const NOT_CONNECTED_SYMBOL: &str = "antenna.radiowaves.left.and.right.slash";
const NOT_CONNECTED_TEXT: &str = "Not connected";

/// Which popup the menu bar item shows, as NetSpeedGauge splits its card and menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupKind {
    /// Left click and hover: the header and one row per device.
    Devices,
    /// Right click: Settings and Quit.
    Actions,
}

/// What one row of the menu shows.
#[derive(Clone)]
enum MenuRow {
    Header,
    Device(Device),
    Empty,
    Settings,
    Quit,
    /// The battery card's bottom padding, where the menu has Settings and Quit.
    Spacer,
}

/// What sits at a device row's right edge.
enum Trailing {
    Percent { text: String, color: u32, has_bolt: bool },
    NotConnected,
    Nothing,
}

/// The menu's fonts and sizes for one text size. Every length is NetSpeedGauge's scaled by the same factor.
struct Metrics {
    body: Font,
    strong: Font,
    small: Font,
    digits: Font,
    min_width: i32,
    summary_height: i32,
    row_height: i32,
    padding: i32,
    hover_radius: f32,
    icon_points: f64,
    icon_center_x: i32,
    label_x: i32,
    label_top: i32,
    header_x: i32,
    header_top: i32,
    right_inset: i32,
    name_gap: i32,
    bolt_gap: i32,
    bolt_points: f64,
}

impl Metrics {
    fn new(font_points: u32) -> Self {
        let points = f64::from(font_points.clamp(MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_MAX));
        let scale = MENU_SCALE * points / f64::from(MENU_FONT_POINTS_DEFAULT);
        let length = |value: f64| (value * scale).round() as i32;
        let body = Font::system(points, WEIGHT_REGULAR);
        let small = Font::system(points - HEADER_POINTS_BELOW, WEIGHT_REGULAR);
        let (summary_height, row_height) = (length(SUMMARY_HEIGHT), length(ROW_HEIGHT));
        Self {
            label_top: (row_height - body.box_height) / 2,
            header_top: (summary_height - small.box_height) / 2,
            body,
            strong: Font::system(points, WEIGHT_SEMIBOLD),
            small,
            digits: Font::digits(points, WEIGHT_MEDIUM),
            min_width: length(MENU_WIDTH),
            summary_height,
            row_height,
            padding: length(PADDING),
            hover_radius: (HOVER_RADIUS * scale) as f32,
            icon_points: ICON_POINTS * scale,
            icon_center_x: length(ICON_CENTER_X),
            label_x: length(LABEL_X),
            header_x: length(HEADER_X),
            right_inset: length(RIGHT_INSET),
            name_gap: length(NAME_GAP),
            bolt_gap: length(BOLT_GAP),
            bolt_points: BOLT_POINTS * scale,
        }
    }
}

pub struct RowState {
    row: MenuRow,
    /// The menu's text size, which sets every length in the row.
    font_points: u32,
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

define_class!(
    // Holds every row. Just before it draws, it moves the menu's window where NetSpeedGauge puts its own.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarMenuPanelView"]
    pub struct MenuPanelView;

    impl MenuPanelView {
        #[unsafe(method(viewWillDraw))]
        fn view_will_draw(&self) {
            if let Some(window) = self.window() {
                place_menu_window(&window);
            }
            // SAFETY: this calls NSView's own method.
            let _: () = unsafe { msg_send![super(self), viewWillDraw] };
        }
    }
);

impl MenuPanelView {
    fn create(width: i32, height: f64) -> Retained<Self> {
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(width), height));
        let this = Self::alloc(main_thread()).set_ivars(());
        // SAFETY: initWithFrame: is NSView's designated initialiser.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// Returns where the menu's window goes, as NetSpeedGauge's `on_will_draw_in`: its top one point into the
/// menu bar, and its right edge at the end of the icon's selected background, kept on the item's screen.
pub fn menu_origin(size: NSSize, anchor: NSRect, area: NSRect) -> NSPoint {
    let top = area.origin.y + area.size.height + MENU_OVERLAP_WITH_BAR;
    let right = anchor.origin.x + anchor.size.width + MENU_PAST_ITEM;
    NSPoint::new((right - size.width).max(area.origin.x), top - size.height)
}

fn place_menu_window(window: &NSWindow) {
    let Some((anchor, area)) = crate::app::item_and_screen_frames() else {
        return;
    };
    let origin = menu_origin(window.frame().size, anchor, area);
    if window.frame().origin != origin {
        window.setFrameOrigin(origin);
    }
}

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
fn right_column_width(metrics: &Metrics) -> i32 {
    let charging_full = canvas::symbol_width(ICON_BOLT, metrics.bolt_points) + metrics.bolt_gap + canvas::text_width("100%", &metrics.digits);
    let shortcut = ["⌘,", "⌘Q"].iter().map(|key| canvas::text_width(key, &metrics.body)).max().unwrap_or(0);
    charging_full.max(shortcut).max(canvas::symbol_width(NOT_CONNECTED_SYMBOL, metrics.icon_points))
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
    fn create(row: MenuRow, index: usize, width: i32, font_points: u32) -> Retained<Self> {
        let metrics = Metrics::new(font_points);
        let height = match row {
            MenuRow::Header => metrics.summary_height + metrics.padding,
            // The last row carries the menu's bottom padding, as in NetSpeedGauge.
            MenuRow::Quit => metrics.row_height + metrics.padding,
            MenuRow::Spacer => metrics.padding,
            _ => metrics.row_height,
        };
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(width), f64::from(height)));
        let this = Self::alloc(main_thread()).set_ivars(RowState { row, font_points, index, is_mouse_inside: Cell::new(false) });
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
        self.ivars().is_mouse_inside.get() && self.row_item().is_some_and(|(_, item)| item.isEnabled())
    }

    fn draw(&self) {
        let theme = theme_for(&self.effectiveAppearance());
        let metrics = Metrics::new(self.ivars().font_points);
        let size = self.frame().size;
        let (width, height) = (size.width as i32, size.height as i32);
        if PAINTS_BACKGROUND.with(Cell::get) {
            canvas::round_rect(0, 0, width, height, 0.0, theme.surface);
        }
        let (row_height, padding) = (metrics.row_height, metrics.padding);
        let right = width - metrics.right_inset;
        let divider_width = width - padding * 2;
        let is_highlighted = self.is_highlighted();
        if is_highlighted {
            canvas::round_rect(padding, 0, divider_width, row_height, metrics.hover_radius, mix(theme.surface, theme.text, 0.09));
        }
        let font = if is_highlighted { &metrics.strong } else { &metrics.body };
        let label = |icon: &str, text: &str| {
            canvas::symbol(metrics.icon_center_x, row_height / 2, icon, metrics.icon_points, if is_highlighted { theme.text } else { theme.muted });
            canvas::text(metrics.label_x, metrics.label_top, text, font, theme.text, Align::Left);
        };
        match &self.ivars().row {
            MenuRow::Header => {
                canvas::text(metrics.header_x, metrics.header_top, "Juice Bar", &metrics.small, theme.muted, Align::Left);
                canvas::round_rect(padding, metrics.summary_height, divider_width, 1, 0.0, theme.border);
            }
            MenuRow::Empty => canvas::text(metrics.label_x, metrics.label_top, EMPTY_TEXT, &metrics.body, theme.muted, Align::Left),
            MenuRow::Spacer => {}
            MenuRow::Settings => {
                label("gearshape", "Settings");
                canvas::text_ink_right(right, metrics.label_top, "⌘,", &metrics.body, theme.muted);
            }
            MenuRow::Quit => {
                label("power", "Quit");
                canvas::text_ink_right(right, metrics.label_top, "⌘Q", &metrics.body, theme.muted);
            }
            MenuRow::Device(device) => {
                let room = right - right_column_width(&metrics) - metrics.name_gap - metrics.label_x;
                label(kind_icon(device.kind), &fitted_name(&device.name, font, room));
                match trailing(device, &theme) {
                    Trailing::Percent { text, color, has_bolt } => {
                        canvas::text_ink_right(right, metrics.label_top, &text, &metrics.digits, color);
                        if has_bolt {
                            let bolt_width = canvas::symbol_width(ICON_BOLT, metrics.bolt_points);
                            let bolt_x = right - canvas::text_width(&text, &metrics.digits) - metrics.bolt_gap - bolt_width / 2;
                            canvas::symbol(bolt_x, row_height / 2, ICON_BOLT, metrics.bolt_points, color);
                        }
                    }
                    Trailing::NotConnected => canvas::symbol_right(right, row_height / 2, NOT_CONNECTED_SYMBOL, metrics.icon_points, theme.muted),
                    Trailing::Nothing => {}
                }
            }
        }
    }

    /// Runs the row's item, or Report for a device on right-click or ⌥-click, then closes the menu.
    /// Returns the menu and this row's own hidden item, which holds its action, tag and shortcut.
    fn row_item(&self) -> Option<(Retained<NSMenu>, Retained<NSMenuItem>)> {
        let panel = self.enclosingMenuItem()?;
        // SAFETY: the panel item is alive while its view is on screen, so its menu is too.
        let menu = unsafe { panel.menu() }?;
        let item = menu.itemAtIndex(self.ivars().index as isize)?;
        Some((menu, item))
    }

    fn clicked(&self, wants_report: bool) {
        let Some((menu, item)) = self.row_item() else {
            return;
        };
        match &self.ivars().row {
            MenuRow::Device(_) if wants_report => {
                menu.cancelTracking();
                on_report(item.tag());
            }
            MenuRow::Header | MenuRow::Empty | MenuRow::Spacer => {}
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
/// right column and the right inset. Never narrower than NetSpeedGauge's menu at the same scale.
fn menu_width(devices: &[Device], metrics: &Metrics) -> i32 {
    let longest = devices.iter().map(|device| canvas::text_width(&short_name(&device.name), &metrics.strong)).max().unwrap_or(0);
    // With no devices the only text is the empty note, which must fit whole instead of being cut.
    let empty_note = if devices.is_empty() { metrics.label_x + canvas::text_width(EMPTY_TEXT, &metrics.body) + metrics.right_inset } else { 0 };
    (metrics.label_x + longest + metrics.name_gap + right_column_width(metrics) + metrics.right_inset).max(metrics.min_width).max(empty_note)
}

/// Returns (menu width, the gap between the longest name and the right column, the scaled least gap), for checks.
pub fn name_gap_for_checks(devices: &[Device], font_points: u32) -> (i32, i32, i32) {
    let metrics = Metrics::new(font_points);
    let width = menu_width(devices, &metrics);
    let longest = devices.iter().map(|device| canvas::text_width(&short_name(&device.name), &metrics.body)).max().unwrap_or(0);
    (width, width - metrics.right_inset - right_column_width(&metrics) - (metrics.label_x + longest), metrics.name_gap)
}

fn settings_item() -> Retained<NSMenuItem> {
    targeted_item(sel!(openSettings:), ",", -1)
}

fn quit_item() -> Retained<NSMenuItem> {
    action_item(Some(sel!(terminate:)), "q", -1)
}

/// Returns the rows to show, each with its item (which holds the action, tag and shortcut) and hover text.
/// The devices popup is the header and one row per device; the actions popup is Settings and Quit.
fn row_list(devices: &[Device], kind: PopupKind) -> Vec<(Retained<NSMenuItem>, MenuRow, Option<String>)> {
    if kind == PopupKind::Actions {
        // The top padding matches the bottom one, which the Quit row carries.
        return vec![(action_item(None, "", -1), MenuRow::Spacer, None), (settings_item(), MenuRow::Settings, None), (quit_item(), MenuRow::Quit, None)];
    }
    let mut rows = vec![(action_item(None, "", -1), MenuRow::Header, None)];
    if devices.is_empty() {
        rows.push((action_item(None, "", -1), MenuRow::Empty, None));
    }
    for (index, device) in devices.iter().enumerate() {
        // An unknown device runs Try to detect on click; a known one opens Settings.
        let action = if device.is_unrecognised() { sel!(tryToDetect:) } else { sel!(rowClicked:) };
        rows.push((targeted_item(action, "", index as isize), MenuRow::Device(device.clone()), Some(row_tooltip(device))));
    }
    rows.push((action_item(None, "", -1), MenuRow::Spacer, None));
    rows
}

/// Returns a popup's width: the devices popup fits its longest name, the actions popup its labels and shortcuts.
fn popup_width(devices: &[Device], metrics: &Metrics, kind: PopupKind) -> i32 {
    match kind {
        PopupKind::Devices => menu_width(devices, metrics),
        // Sized to its own content, as on Windows: the longest label in its hover weight, the gap, the
        // wider shortcut and the right inset.
        PopupKind::Actions => {
            let longest = ["Settings", "Quit"].iter().map(|label| canvas::text_width(label, &metrics.strong)).max().unwrap_or(0);
            let shortcut = ["⌘,", "⌘Q"].iter().map(|key| canvas::text_width(key, &metrics.body)).max().unwrap_or(0);
            metrics.label_x + longest + metrics.name_gap + shortcut + metrics.right_inset
        }
    }
}

/// Creates the drawn view for each row, top first, all the same width.
fn row_views(rows: Vec<(Retained<NSMenuItem>, MenuRow, Option<String>)>, width: i32, font_points: u32) -> Vec<(Retained<NSMenuItem>, Retained<MenuRowView>)> {
    rows.into_iter()
        .enumerate()
        .map(|(index, (item, row, tooltip))| {
            let view = MenuRowView::create(row, index, width, font_points);
            if let Some(text) = tooltip {
                view.setToolTip(Some(&NSString::from_str(&text)));
            }
            (item, view)
        })
        .collect()
}

/// Stacks row views top first inside a container, and returns its height. A flipped container counts
/// down from its top, a normal one up from its bottom.
fn stack(container: &NSView, views: &[Retained<MenuRowView>]) -> f64 {
    let height: f64 = views.iter().map(|view| view.frame().size.height).sum();
    let is_flipped = container.isFlipped();
    let mut offset = 0.0;
    for view in views {
        let row_height = view.frame().size.height;
        let y = if is_flipped { offset } else { height - offset - row_height };
        view.setFrameOrigin(NSPoint::new(0.0, y));
        container.addSubview(view);
        offset += row_height;
    }
    height
}

/// Fills the menu: header, one row per device, a divider, then Settings and Quit with their shortcuts; or,
/// for the battery card, just the header and device rows.
/// As in NetSpeedGauge, every row is drawn inside ONE panel item: separate view items each get spacing
/// from macOS, which left about 40 pt empty under Quit. Each row keeps a hidden item of its own, so its
/// action, tag and shortcut (⌘, and ⌘Q) still work while the menu is open.
pub fn show_menu(menu: &NSMenu, devices: &[Device], font_points: u32, kind: PopupKind) {
    menu.removeAllItems();
    let width = popup_width(devices, &Metrics::new(font_points), kind);
    let rows = RefCell::new(Vec::<Retained<MenuRowView>>::new());
    for (item, view) in row_views(row_list(devices, kind), width, font_points) {
        item.setHidden(true);
        item.setAllowsKeyEquivalentWhenHidden(true);
        menu.addItem(&item);
        rows.borrow_mut().push(view);
    }
    // The devices popup shows no Settings or Quit row, but ⌘, and ⌘Q still work while it is open.
    if kind == PopupKind::Devices {
        for item in [settings_item(), quit_item()] {
            item.setHidden(true);
            item.setAllowsKeyEquivalentWhenHidden(true);
            menu.addItem(&item);
        }
    }
    let rows = rows.into_inner();
    let height: f64 = rows.iter().map(|view| view.frame().size.height).sum();
    let panel = MenuPanelView::create(width, height);
    stack(&panel, &rows);
    // macOS can make a menu wider than its content (a floor of its own, or a wide menu bar item). A
    // width-sizable item view is stretched to the menu's width, and each row draws its right edge from
    // its own width, so the drawn popup always fills the menu with no empty band.
    panel.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    rows.iter().for_each(|row| row.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable));
    let panel_item = NSMenuItem::new(main_thread());
    panel_item.setView(Some(&panel));
    menu.addItem(&panel_item);
}

/// Returns the drawn rows of a filled menu, top first.
fn drawn_rows(menu: &NSMenu) -> Vec<Retained<NSView>> {
    let Some(panel) = menu.itemArray().iter().find_map(|item| item.view()) else {
        return Vec::new();
    };
    let mut rows: Vec<Retained<NSView>> = panel.subviews().iter().collect();
    rows.sort_by(|a, b| b.frame().origin.y.total_cmp(&a.frame().origin.y));
    rows
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
pub fn capture_menu_for_checks(devices: &[Device], font_points: u32, kind: PopupKind, path: &str, is_dark: bool, highlight: Option<usize>) -> bool {
    let menu = NSMenu::new(main_thread());
    show_menu(&menu, devices, font_points, kind);
    let views = drawn_rows(&menu);
    views.iter().for_each(|view| view.removeFromSuperview());
    let width = views.first().map_or(MENU_WIDTH as i32, |view| view.frame().size.width as i32);
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
    let window = menu.itemArray().iter().find_map(|item| item.view()).and_then(|view| view.window());
    if let (Some(window), Some((anchor, area))) = (&window, crate::app::item_and_screen_frames()) {
        let frame = window.frame();
        eprintln!(
            "capture: menu window top {:.1}, menu bar bottom {:.1}, right {:.1}, icon right {:.1}, size {:.0} x {:.0}",
            frame.origin.y + frame.size.height,
            area.origin.y + area.size.height,
            frame.origin.x + frame.size.width,
            anchor.origin.x + anchor.size.width,
            frame.size.width,
            frame.size.height
        );
    }
    if let (Some(window), Some(panel)) = (&window, menu.itemArray().iter().find_map(|item| item.view())) {
        let in_window = panel.convertRect_toView(panel.bounds(), None);
        let window_height = window.frame().size.height;
        eprintln!(
            "capture: panel {:.0} x {:.0} in a {:.0} wide window, empty band on the right {:.1}, {:.1} pt of menu above it, {:.1} pt below it",
            in_window.size.width,
            in_window.size.height,
            window.frame().size.width,
            window.frame().size.width - (in_window.origin.x + in_window.size.width),
            window_height - (in_window.origin.y + in_window.size.height),
            in_window.origin.y
        );
    }
    let frame = window.and_then(|window| window.contentView());
    // The menu's see-through material paints as blank in a capture, so the rows paint their own surface.
    PAINTS_BACKGROUND.with(|cell| cell.set(true));
    let painted = frame.is_some_and(|view| crate::surface::capture_view(&view, path));
    PAINTS_BACKGROUND.with(|cell| cell.set(false));
    painted
}

// The hover card, as NetSpeedGauge's: a borderless panel under the menu bar item, shown without taking
// the keyboard, closed when the pointer leaves the icon.
const CARD_CORNER_RADIUS: f32 = 9.0;
const WINDOW_LEVEL_POPUP: isize = 25;

thread_local! {
    static HOVER_CARD: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

define_class!(
    // Paints the hover card's rounded surface and border; its rows draw on top.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarHoverCardView"]
    pub struct HoverCardView;

    impl HoverCardView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let theme = theme_for(&self.effectiveAppearance());
            let size = self.frame().size;
            let (width, height) = (size.width as i32, size.height as i32);
            canvas::round_rect(0, 0, width, height, CARD_CORNER_RADIUS, theme.border);
            canvas::round_rect(1, 1, width - 2, height - 2, CARD_CORNER_RADIUS - 1.0, theme.surface);
        }

        // The card only shows; the pointer never reaches it, and it must never take a click.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }
    }
);

/// Builds the hover card's window with its rows, sized to them, without showing it.
fn build_hover_card(devices: &[Device], font_points: u32) -> Retained<NSWindow> {
    let width = popup_width(devices, &Metrics::new(font_points), PopupKind::Devices);
    let views: Vec<Retained<MenuRowView>> = row_views(row_list(devices, PopupKind::Devices), width, font_points).into_iter().map(|(_, view)| view).collect();
    let height: f64 = views.iter().map(|view| view.frame().size.height).sum();
    let frame = NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(width), height));
    let this = HoverCardView::alloc(main_thread()).set_ivars(());
    // SAFETY: initWithFrame: is NSView's designated initialiser.
    let card: Retained<HoverCardView> = unsafe { msg_send![super(this), initWithFrame: frame] };
    stack(&card, &views);
    let style = objc2_app_kit::NSWindowStyleMask::Borderless | objc2_app_kit::NSWindowStyleMask::NonactivatingPanel;
    let panel = objc2_app_kit::NSPanel::initWithContentRect_styleMask_backing_defer(objc2_app_kit::NSPanel::alloc(main_thread()), frame, style, objc2_app_kit::NSBackingStoreType::Buffered, false);
    let window: Retained<NSWindow> = panel.into_super();
    // SAFETY: the card's state keeps its own reference, so AppKit must not free the window on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
    window.setHasShadow(true);
    window.setIgnoresMouseEvents(true);
    window.setLevel(WINDOW_LEVEL_POPUP);
    window.setContentView(Some(&card));
    window
}

/// Shows the battery card under the menu bar item without taking the keyboard, replacing any open one.
/// `frames` is the item's frame and its screen's usable area.
pub fn show_hover_card(devices: &[Device], font_points: u32, frames: Option<(NSRect, NSRect)>) {
    hide_hover_card();
    let window = build_hover_card(devices, font_points);
    // Flush with the menu bar under the icon, where the click popups open, so all three sit in one place.
    if let Some((anchor, area)) = frames {
        window.setFrameOrigin(menu_origin(window.frame().size, anchor, area));
    }
    window.orderFrontRegardless();
    HOVER_CARD.with(|cell| *cell.borrow_mut() = Some(window));
}

/// Closes the hover card, if it is open.
pub fn hide_hover_card() {
    let open = HOVER_CARD.with(|cell| cell.borrow_mut().take());
    if let Some(window) = open {
        window.orderOut(None);
    }
}

/// Returns true while the hover card is showing.
pub fn is_hover_card_open() -> bool {
    HOVER_CARD.with(|cell| cell.borrow().is_some())
}

/// Paints the open hover card to a PNG and logs its window against its drawn card, for capture mode.
pub fn capture_hover_card(path: &str) -> bool {
    let Some(window) = HOVER_CARD.with(|cell| cell.borrow().clone()) else {
        eprintln!("capture: no hover card is open");
        return false;
    };
    let frame = window.frame();
    if let (Some(card), Some((anchor, area))) = (window.contentView(), crate::app::item_and_screen_frames()) {
        eprintln!(
            "capture: hover card window {:.0} x {:.0}, drawn card {:.0} x {:.0}, top {:.1} under the bar bottom {:.1}, right {:.1}, icon right {:.1}",
            frame.size.width,
            frame.size.height,
            card.frame().size.width,
            card.frame().size.height,
            frame.origin.y + frame.size.height,
            area.origin.y + area.size.height,
            frame.origin.x + frame.size.width,
            anchor.origin.x + anchor.size.width
        );
        return crate::surface::capture_view(&card, path);
    }
    false
}

/// Paints the battery card offscreen to a PNG in a chosen look, for checks.
pub fn capture_card_for_checks(devices: &[Device], font_points: u32, path: &str, is_dark: bool) -> bool {
    let window = build_hover_card(devices, font_points);
    let theme = if is_dark { dark_theme() } else { light_theme() };
    window.setAppearance(crate::theme::appearance_for(&theme).as_deref());
    window.contentView().is_some_and(|card| crate::surface::capture_view(&card, path))
}
