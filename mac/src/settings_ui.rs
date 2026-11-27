//! The Settings window, drawn the same way as NetSpeedGauge's: a sidebar of pages, a fixed header, and
//! rows in rounded cards. It exists only while open. Every change applies at once.
//!
//! The one rule that keeps it alive: never hold the WINDOW borrow across an AppKit call that can call
//! back. A rename box that loses focus ends its edit and sends its action, which borrows WINDOW again,
//! and a second borrow aborts the app. So state is changed inside `with_window`, and anything that can
//! end an edit (focus, hiding or removing a box, closing) runs after the borrow is dropped.

use crate::canvas::{self, mix, Align, Font, WEIGHT_REGULAR, WEIGHT_SEMIBOLD};
use crate::delegate::{delegate, main_thread, start_timer};
use crate::surface::{self, SettingsView, KEY_ESCAPE};
use crate::theme::{apply_title_bar, current_theme, dark_theme, draw_made_with_love, light_theme, Theme};
use crate::{app, startup};
use juicebar_shared::device::{Connection, Device, DeviceKind};
use juicebar_shared::names::short_name;
use juicebar_shared::settings::{HoverAction, Settings, MENU_FONT_POINTS_MAX, MENU_FONT_POINTS_MIN};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSControl, NSFocusRingType, NSTextField, NSTextView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSTimer};
use std::cell::RefCell;

const WINDOW_TITLE: &str = "Juice Bar Settings";
const WIDTH: i32 = 680;
const HEIGHT: i32 = 640;
const SIDEBAR_WIDTH: i32 = 176;
const MARGIN: i32 = 20;
const HEADER_HEIGHT: i32 = 62;
const ROW_HEIGHT: i32 = 50;
const ROW_GAP: i32 = 4;
const SECTION_GAP: i32 = 34;
const NOTE_HEIGHT: i32 = 46;
const ABOUT_CARD_HEIGHT: i32 = 96;
const APP_TILE: i32 = 64;
const APP_GLASS_HEIGHT: i32 = 40;
const PRIVACY_LINE: i32 = 26;
const PRIVACY_HEADER_HEIGHT: i32 = 68;
const LOCK_TILE: i32 = 40;
const CARD_RADIUS: i32 = 6;
const NAV_TOP: i32 = 62;
const NAV_ROW: i32 = 40;
// Space each side of a segment's text. The system font runs wider than Windows', so this is tighter than there.
const SEGMENT_PADDING: i32 = 8;
const TOGGLE_WIDTH: i32 = 40;
const FIELD_HEIGHT: i32 = 30;
const FIELD_GAP: i32 = 16;
// The real text box sits inside the drawn box, inset so its text lines up with the row's own text.
const FIELD_INSET_X: i32 = 8;
const FIELD_TEXT_HEIGHT: i32 = 18;
const CONFIRM_WIDTH: i32 = 400;
const CONFIRM_HEIGHT: i32 = 184;
const BUTTON_HEIGHT: i32 = 30;
const RESET_NOTE_SECONDS: f64 = 4.0;
const FIELD_TAG_BASE: isize = 100;
const ICON_MINUS: &str = "minus";
const ICON_PLUS: &str = "plus";
const DISABLED_FILL: f32 = 0.35;
const DISABLED_TEXT: f32 = 0.5;
// The rename controls: a pencil on an idle row; a field with ✓ and ✕ while that row is being renamed.
const ICON_PENCIL: &str = "pencil";
const ICON_SAVE: &str = "checkmark";
const ICON_CANCEL: &str = "xmark";
const EDIT_BUTTON: i32 = 26;
const EDIT_BUTTON_GAP: i32 = 4;
const RENAME_SAVED: &str = "Saved";
const RENAME_SAVED_SECONDS: f64 = 2.0;
const ICON_CHECK: &str = "checkmark";
const ICON_DONE: &str = "checkmark.circle.fill";
const ICON_LOCK: &str = "lock.fill";
const ICON_RESET: &str = "arrow.counterclockwise";

/// The pages in the sidebar, with their icon and title.
// The same split, icons and order as NetSpeedGauge's sidebar and Juice Bar's Windows settings.
const PAGES: [(&str, &str); 5] = [
    ("gearshape", "General"),
    ("paintbrush", "Appearance"),
    ("computermouse", "Devices"),
    ("slider.horizontal.3", "Advanced"),
    ("info.circle", "About"),
];
const APPEARANCE_PAGE: usize = 1;
const DEVICES_PAGE: usize = 2;
const ADVANCED_PAGE: usize = 3;

/// The check intervals offered, in minutes, with their labels.
const MIN_POLL_MINUTES: u32 = 1;
const MAX_POLL_MINUTES: u32 = 60;
const HOVER_OPTIONS: [&str; 2] = ["Nothing", "Battery card"];
const CLICK_OPTIONS: [&str; 2] = ["Devices", "Settings"];

/// What Juice Bar does and does not do with data, shown on the Details page.
const PRIVACY_LINES: [&str; 4] = [
    "Nothing leaves this Mac. It never connects to the internet.",
    "It only reads battery levels from your mice and keyboards.",
    "Your names and settings are stored only on this computer.",
    "No account, no tracking, no ads and no servers.",
];

const APP_NAME: &str = "Juice Bar";
const DEVELOPER_NAME: &str = "Ashutosh Anand";
const DEVELOPER_ORIGIN: &str = "From India";
const APP_TAGLINE: &str = "Mouse and keyboard battery in the menu bar.";
const RESET_DETAIL: [&str; 2] = ["Names, hidden devices, the check interval and the %", "next to the icon go back to their defaults."];

/// One row on a page.
#[derive(Clone, Copy)]
enum Row {
    Section(&'static str),
    Note(&'static str),
    LaunchAtLogin,
    ShowPercent,
    MenuTextSize,
    PollInterval,
    OnHover,
    OnClick,
    DeviceList,
    ResetAll,
    AboutCard,
    Privacy,
    Developer,
}

const GENERAL_ROWS: &[Row] = &[Row::Section("Startup"), Row::LaunchAtLogin, Row::Section("Battery checks"), Row::PollInterval];
const APPEARANCE_ROWS: &[Row] = &[Row::Section("Menu bar icon"), Row::ShowPercent, Row::Section("Menu"), Row::MenuTextSize];
const DEVICES_ROWS: &[Row] = &[
    Row::Section("Your devices"),
    Row::Note("Click the pencil to give a device a name you will recognise.\nSwitch a device off to leave it out of the menu and the icon."),
    Row::DeviceList,
];
const ADVANCED_ROWS: &[Row] = &[Row::Section("Behaviour"), Row::OnHover, Row::OnClick, Row::Section("Reset"), Row::ResetAll];
const ABOUT_ROWS: &[Row] = &[Row::AboutCard, Row::Section("Privacy"), Row::Privacy, Row::Section("Know your developer"), Row::Developer];

fn page_rows(page: usize) -> &'static [Row] {
    match page {
        0 => GENERAL_ROWS,
        APPEARANCE_PAGE => APPEARANCE_ROWS,
        DEVICES_PAGE => DEVICES_ROWS,
        ADVANCED_PAGE => ADVANCED_ROWS,
        _ => ABOUT_ROWS,
    }
}

/// What a click on an area does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Page(usize),
    ToggleLaunchAtLogin,
    TogglePercent,
    SetPoll(u32),
    SetMenuFont(u32),
    SetHover(u8),
    SetClick(u8),
    ToggleShown(usize),
    EditName(usize),
    SaveName(usize),
    CancelName,
    AskReset,
    CancelReset,
    ConfirmReset,
}

#[derive(Clone, Copy)]
struct HitArea {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    action: Action,
}

/// The pointer to show over a spot.
pub enum Cursor {
    Arrow,
    Hand,
    Text,
}

struct Fonts {
    body: Font,
    strong: Font,
    small: Font,
    title: Font,
}

impl Fonts {
    fn new() -> Self {
        Self {
            body: Font::system(10.0, WEIGHT_REGULAR),
            strong: Font::system(10.0, WEIGHT_SEMIBOLD),
            small: Font::system(9.0, WEIGHT_REGULAR),
            title: Font::system(16.0, WEIGHT_SEMIBOLD),
        }
    }
}

/// What a click asks for once the WINDOW borrow is dropped.
#[derive(Default)]
struct Effects {
    /// A row to start renaming: its field gets the name, all selected.
    begins_edit: Option<usize>,
    /// A row whose field to save, read once the borrow is dropped.
    saves_edit: Option<usize>,
    apply: Option<Settings>,
    layout: bool,
    starts_done_timer: bool,
}

struct SettingsWindow {
    window: Retained<NSWindow>,
    view: Retained<SettingsView>,
    settings: Settings,
    /// Every device found, under its own name (not the user's rename), in menu order.
    devices: Vec<Device>,
    /// One rename box per device, in the same order.
    fields: Vec<Retained<NSTextField>>,
    fonts: Fonts,
    theme: Theme,
    page: usize,
    scroll: i32,
    content_height: i32,
    hits: Vec<HitArea>,
    hover: Option<Action>,
    is_confirming_reset: bool,
    is_reset_done: bool,
    done_timer: Option<Retained<NSTimer>>,
    /// The one row being renamed, if any. Only ✓ or Return saves it; anything else cancels it.
    editing: Option<usize>,
    /// The row whose hint shows "Saved" for a moment after its rename was saved.
    saved_row: Option<usize>,
    saved_timer: Option<Retained<NSTimer>>,
}

thread_local! {
    static WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
}

/// Runs `action` on the open window's state. Returns None when there is no window or it is already borrowed.
fn with_window<T>(action: impl FnOnce(&mut SettingsWindow) -> T) -> Option<T> {
    WINDOW.with(|cell| cell.try_borrow_mut().ok().and_then(|mut state| state.as_mut().map(action)))
}

fn content_left() -> i32 {
    SIDEBAR_WIDTH + MARGIN - 4
}

fn content_width() -> i32 {
    WIDTH - SIDEBAR_WIDTH - MARGIN * 2 + 4
}

fn privacy_height() -> i32 {
    PRIVACY_HEADER_HEIGHT + PRIVACY_LINE * PRIVACY_LINES.len() as i32 + 20
}

/// Splits a note into its two lines.
fn split_note(text: &'static str) -> (&'static str, &'static str) {
    text.split_once('\n').unwrap_or((text, ""))
}

/// Returns the line under a device's name: what it is, how it is connected and its battery.
fn device_hint(device: &Device) -> String {
    let kind = match device.kind {
        DeviceKind::Mouse => "Mouse",
        DeviceKind::Keyboard => "Keyboard",
        DeviceKind::Other => "Device",
    };
    let link = match device.connection {
        Connection::Dongle => "USB receiver",
        Connection::Bluetooth => "Bluetooth",
        Connection::Usb => "cable",
    };
    let level = match device.battery {
        // On its cable a device charges, whatever its reading says, as the menu shows it.
        Some(battery) if battery.is_charging || device.connection == Connection::Usb => format!("{}%, charging", battery.percent),
        None if device.connection == Connection::Usb => "charging".to_string(),
        Some(battery) => format!("{}%", battery.percent),
        None => "no reading yet".to_string(),
    };
    format!("{kind}, {link}, {level}")
}

impl SettingsWindow {
    fn add_hit(&mut self, x: i32, y: i32, width: i32, height: i32, action: Action) {
        self.hits.push(HitArea { x, y, width, height, action });
    }

    fn is_hover(&self, action: Action) -> bool {
        self.hover == Some(action)
    }

    fn invalidate(&self) {
        self.view.setNeedsDisplay(true);
    }

    /// Returns how tall a row is, gap below included.
    fn row_height(&self, row: Row) -> i32 {
        match row {
            Row::Section(_) => SECTION_GAP + 8,
            Row::Note(_) => NOTE_HEIGHT,
            Row::LaunchAtLogin | Row::ShowPercent | Row::MenuTextSize | Row::PollInterval | Row::OnHover | Row::OnClick | Row::ResetAll => ROW_HEIGHT + ROW_GAP,
            Row::DeviceList => self.devices.len().max(1) as i32 * (ROW_HEIGHT + ROW_GAP),
            Row::AboutCard => ABOUT_CARD_HEIGHT + ROW_GAP,
            Row::Privacy => privacy_height() + ROW_GAP,
            Row::Developer => ROW_HEIGHT + ROW_GAP,
        }
    }

    /// Returns where the rename controls sit on a device row: (field left, field width, ✓ left, ✕ or pencil left).
    fn edit_geometry(left: i32, width: i32) -> (i32, i32, i32, i32) {
        let toggle_left = left + width - 14 - TOGGLE_WIDTH;
        let cancel_x = toggle_left - FIELD_GAP - EDIT_BUTTON;
        let save_x = cancel_x - EDIT_BUTTON_GAP - EDIT_BUTTON;
        let field_x = left + 8;
        (field_x, save_x - 8 - field_x, save_x, cancel_x)
    }

    /// Returns where each rename box goes, or None when it must be hidden: every box but the one being
    /// renamed, and that one on another page, under the header, or behind the confirm card.
    fn field_frames(&self) -> Vec<Option<NSRect>> {
        let mut frames = vec![None; self.fields.len()];
        if self.page != DEVICES_PAGE || self.is_confirming_reset {
            return frames;
        }
        let mut y = HEADER_HEIGHT - self.scroll;
        for row in page_rows(self.page) {
            if matches!(row, Row::DeviceList) {
                break;
            }
            y += self.row_height(*row);
        }
        let (box_x, box_width, _, _) = Self::edit_geometry(content_left(), content_width());
        for (index, frame) in frames.iter_mut().enumerate() {
            if self.editing != Some(index) {
                continue;
            }
            let box_y = y + index as i32 * (ROW_HEIGHT + ROW_GAP) + (ROW_HEIGHT - FIELD_HEIGHT) / 2;
            if box_y < HEADER_HEIGHT || box_y + FIELD_HEIGHT > HEIGHT {
                continue;
            }
            let text_y = box_y + (FIELD_HEIGHT - FIELD_TEXT_HEIGHT) / 2;
            *frame = Some(NSRect::new(
                NSPoint::new(f64::from(box_x + FIELD_INSET_X), f64::from(text_y)),
                NSSize::new(f64::from(box_width - FIELD_INSET_X * 2), f64::from(FIELD_TEXT_HEIGHT)),
            ));
        }
        frames
    }

    /// Checks whether a point is on a drawn rename box, edge included.
    fn is_over_field(&self, x: i32, y: i32) -> bool {
        self.field_frames().iter().flatten().any(|frame| {
            let left = frame.origin.x as i32 - FIELD_INSET_X;
            let top = frame.origin.y as i32 - (FIELD_HEIGHT - FIELD_TEXT_HEIGHT) / 2;
            let box_width = frame.size.width as i32 + FIELD_INSET_X * 2;
            x >= left && x < left + box_width && y >= top && y < top + FIELD_HEIGHT
        })
    }

    /// Paints the window: the page first, then the fixed header and sidebar over anything scrolled under them.
    fn draw(&mut self) {
        let theme = self.theme;
        self.hits.clear();
        canvas::round_rect(0, 0, WIDTH, HEIGHT, 0.0, theme.background);
        self.draw_page();
        self.hits.retain(|hit| hit.y + hit.height > HEADER_HEIGHT);
        canvas::round_rect(SIDEBAR_WIDTH, 0, WIDTH - SIDEBAR_WIDTH, HEADER_HEIGHT, 0.0, theme.background);
        canvas::text(SIDEBAR_WIDTH + MARGIN - 4, 20, PAGES[self.page].1, &self.fonts.title, theme.text, Align::Left);
        self.draw_sidebar();
        self.draw_scrollbar();
        if self.is_confirming_reset {
            self.draw_confirm_card();
        }
    }

    fn draw_sidebar(&mut self) {
        let theme = self.theme;
        canvas::round_rect(0, 0, SIDEBAR_WIDTH, HEIGHT, 0.0, mix(theme.background, theme.card, 0.45));
        canvas::text(18, 22, APP_NAME, &self.fonts.strong, theme.muted, Align::Left);
        for (position, (icon, label)) in PAGES.iter().enumerate() {
            let y = NAV_TOP + position as i32 * NAV_ROW;
            let (x, width, height) = (8, SIDEBAR_WIDTH - 16, NAV_ROW - 4);
            let is_selected = position == self.page;
            let action = Action::Page(position);
            if is_selected || self.is_hover(action) {
                let fill = if is_selected { theme.selected } else { mix(theme.background, theme.selected, 0.5) };
                canvas::round_rect(x, y, width, height, CARD_RADIUS as f32, fill);
            }
            if is_selected {
                canvas::round_rect(x, y + 10, 3, height - 20, 2.0, theme.accent);
            }
            canvas::symbol(x + 20, y + height / 2, icon, 10.0, if is_selected { theme.text } else { theme.muted });
            let font = if is_selected { &self.fonts.strong } else { &self.fonts.body };
            canvas::text(x + 38, y + 9, label, font, theme.text, Align::Left);
            self.add_hit(x, y, width, height, action);
        }
        draw_made_with_love(SIDEBAR_WIDTH / 2, HEIGHT - 34, &self.fonts.small, theme.muted);
    }

    fn draw_scrollbar(&self) {
        let view = HEIGHT - HEADER_HEIGHT;
        if self.content_height <= view {
            return;
        }
        let thumb_height = (view * view / self.content_height).max(30);
        let thumb_y = HEADER_HEIGHT + (view - thumb_height) * self.scroll / (self.content_height - view).max(1);
        canvas::round_rect(WIDTH - 7, thumb_y, 3, thumb_height, 2.0, mix(self.theme.background, self.theme.muted, 0.5));
    }

    /// Draws every row of the current page, top to bottom, and works out how tall the page is.
    fn draw_page(&mut self) {
        let (left, width) = (content_left(), content_width());
        let mut y = HEADER_HEIGHT - self.scroll;
        for row in page_rows(self.page) {
            self.draw_row(*row, left, y, width);
            y += self.row_height(*row);
        }
        self.content_height = y + self.scroll - HEADER_HEIGHT + MARGIN;
    }

    fn row_card(&self, left: i32, y: i32, width: i32, height: i32) {
        canvas::round_rect(left, y, width, height, CARD_RADIUS as f32, self.theme.card);
    }

    /// Draws a row's label, with its hint under it when there is one.
    fn row_label(&self, left: i32, y: i32, label: &str, hint: &str) {
        let theme = self.theme;
        let x = left + 14;
        if hint.is_empty() {
            canvas::text(x, y + 15, label, &self.fonts.body, theme.text, Align::Left);
        } else {
            canvas::text(x, y + 7, label, &self.fonts.body, theme.text, Align::Left);
            canvas::text(x, y + 27, hint, &self.fonts.small, theme.muted, Align::Left);
        }
    }

    fn draw_row(&mut self, row: Row, left: i32, y: i32, width: i32) {
        match row {
            Row::Section(title) => canvas::text(left + 2, y + 14, title, &self.fonts.strong, self.theme.text, Align::Left),
            Row::Note(text) => {
                let (first, second) = split_note(text);
                canvas::text(left + 2, y + 2, first, &self.fonts.small, self.theme.muted, Align::Left);
                canvas::text(left + 2, y + 19, second, &self.fonts.small, self.theme.muted, Align::Left);
            }
            Row::LaunchAtLogin => {
                let is_on = startup::is_enabled();
                self.draw_toggle_row(left, y, width, ("Open at login", "Opens Juice Bar when you log in"), is_on, Action::ToggleLaunchAtLogin);
            }
            Row::ShowPercent => {
                let is_on = self.settings.shows_percent_in_icon;
                self.draw_toggle_row(left, y, width, ("Show the lowest % in the icon", "Shows the number next to the juice glass"), is_on, Action::TogglePercent);
            }
            Row::MenuTextSize => {
                self.row_card(left, y, width, ROW_HEIGHT);
                self.row_label(left, y, "Menu text size", "The menu and battery card scale with it");
                let points = self.settings.menu_font_points;
                self.draw_stepper(left + width - 14, y, (points, " pt"), (MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_MAX), Action::SetMenuFont);
            }
            Row::PollInterval => {
                self.row_card(left, y, width, ROW_HEIGHT);
                self.row_label(left, y, "Check batteries every", "Juice Bar sleeps in between, so it costs no power");
                let minutes = self.settings.poll_minutes;
                self.draw_stepper(left + width - 14, y, (minutes, " min"), (MIN_POLL_MINUTES, MAX_POLL_MINUTES), Action::SetPoll);
            }
            Row::OnHover => {
                self.row_card(left, y, width, ROW_HEIGHT);
                self.row_label(left, y, "On hover", "When the pointer rests on the icon");
                let chosen = self.settings.hover_action.number();
                self.draw_segments(left + width - 14, y, &HOVER_OPTIONS, chosen, Action::SetHover);
            }
            Row::OnClick => {
                self.row_card(left, y, width, ROW_HEIGHT);
                self.row_label(left, y, "On click", "Right-click opens Settings and Quit");
                let chosen = u8::from(app::is_click_settings(&self.settings));
                self.draw_segments(left + width - 14, y, &CLICK_OPTIONS, chosen, Action::SetClick);
            }
            Row::DeviceList => self.draw_device_list(left, y, width),
            Row::ResetAll => self.draw_reset_row(left, y, width),
            Row::AboutCard => self.draw_about_card(left, y, width),
            Row::Developer => {
                self.row_card(left, y, width, ROW_HEIGHT);
                self.row_label(left, y, DEVELOPER_NAME, DEVELOPER_ORIGIN);
            }
            Row::Privacy => {
                self.row_card(left, y, width, privacy_height());
                self.draw_privacy_header(left, y, width);
                for (position, line) in PRIVACY_LINES.iter().enumerate() {
                    let line_y = y + PRIVACY_HEADER_HEIGHT + 10 + position as i32 * PRIVACY_LINE;
                    canvas::symbol(left + 20, line_y + 10, ICON_CHECK, 8.0, self.theme.accent);
                    canvas::text(left + 36, line_y, line, &self.fonts.body, self.theme.text, Align::Left);
                }
            }
        }
    }

    /// Draws a row with a label, a hint and a toggle. The whole card takes the click, as in NetSpeedGauge.
    fn draw_toggle_row(&mut self, left: i32, y: i32, width: i32, (label, hint): (&str, &str), is_on: bool, action: Action) {
        self.row_card(left, y, width, ROW_HEIGHT);
        self.row_label(left, y, label, hint);
        self.draw_toggle(left + width - 14, y, ROW_HEIGHT, is_on);
        self.add_hit(left, y, width, ROW_HEIGHT, action);
    }

    fn draw_toggle(&self, right: i32, y: i32, row_height: i32, is_on: bool) {
        let theme = self.theme;
        let (width, height) = (TOGGLE_WIDTH, 20);
        let (x, top) = (right - width, y + (row_height - height) / 2);
        if is_on {
            canvas::round_rect(x, top, width, height, height as f32 / 2.0, theme.accent);
        } else {
            canvas::round_rect(x, top, width, height, height as f32 / 2.0, theme.muted);
            canvas::round_rect(x + 1, top + 1, width - 2, height - 2, (height - 2) as f32 / 2.0, theme.card);
        }
        let knob = 12;
        let knob_x = if is_on { x + width - knob - 4 } else { x + 4 };
        let on_knob = if theme.is_dark { 0x1B1B1B } else { 0xFFFFFF };
        let knob_color = if is_on { on_knob } else { theme.muted };
        canvas::round_rect(knob_x, top + (height - knob) / 2, knob, knob, knob as f32 / 2.0, knob_color);
    }

    /// Draws a segmented choice ending at `right`, as NetSpeedGauge's: the chosen option on a raised pill.
    fn draw_segments(&mut self, right: i32, y: i32, options: &[&str], chosen: u8, action_for: fn(u8) -> Action) {
        let theme = self.theme;
        let widths: Vec<i32> = options.iter().map(|option| canvas::text_width(option, &self.fonts.strong) + SEGMENT_PADDING * 2).collect();
        let total: i32 = widths.iter().sum();
        let (height, top) = (32, y + 9);
        let mut x = right - total;
        let radius = CARD_RADIUS as f32;
        canvas::round_rect(x, top, total, height, radius, theme.segment_track_on_card);
        let inset = 3;
        for (position, option) in options.iter().enumerate() {
            let segment = widths[position];
            let action = action_for(position as u8);
            let is_selected = usize::from(chosen) == position;
            if is_selected {
                canvas::round_rect(x + inset, top + inset, segment - inset * 2, height - inset * 2, radius - inset as f32 / 2.0, theme.segment_selected);
            }
            let color = if is_selected || self.is_hover(action) { theme.text } else { theme.muted };
            let font = if is_selected { &self.fonts.strong } else { &self.fonts.body };
            canvas::text(x + segment / 2, top + 6, option, font, color, Align::Center);
            self.add_hit(x, top, segment, height, action);
            x += segment;
        }
    }

    /// Draws a minus and plus pair around the value, as NetSpeedGauge's size stepper. At either end of its
    /// range a button looks switched off and cannot be clicked.
    fn draw_stepper(&mut self, right: i32, y: i32, (value, suffix): (u32, &str), (min, max): (u32, u32), action_for: fn(u32) -> Action) {
        let theme = self.theme;
        let (size, top) = (30, y + 10);
        let value_width = 76;
        let plus_x = right - size;
        let minus_x = plus_x - value_width - size;
        let buttons = [(minus_x, ICON_MINUS, value.saturating_sub(1), value > min), (plus_x, ICON_PLUS, value + 1, value < max)];
        for (x, icon, new_value, is_enabled) in buttons {
            let action = action_for(new_value);
            let radius = CARD_RADIUS as f32;
            if is_enabled {
                let fill = if self.is_hover(action) { theme.button_hover } else { theme.button };
                canvas::round_rect(x, top, size, size, radius, fill);
                canvas::symbol(x + size / 2, top + size / 2, icon, 8.0, theme.text);
                self.add_hit(x, top, size, size, action);
            } else {
                canvas::round_rect(x, top, size, size, radius, mix(theme.card, theme.button, DISABLED_FILL));
                canvas::symbol(x + size / 2, top + size / 2, icon, 8.0, mix(theme.card, theme.muted, DISABLED_TEXT));
            }
        }
        canvas::text(minus_x + size + value_width / 2, top + 5, &format!("{value}{suffix}"), &self.fonts.body, theme.text, Align::Center);
    }

    /// Draws one row per device: its full name and details, a rename box, and a Show toggle.
    fn draw_device_list(&mut self, left: i32, y: i32, width: i32) {
        let theme = self.theme;
        if self.devices.is_empty() {
            self.row_card(left, y, width, ROW_HEIGHT);
            self.row_label(left, y, "No wireless mouse or keyboard found yet", "Plug in its USB receiver or connect it over Bluetooth");
            return;
        }
        let toggle_right = left + width - 14;
        let (field_x, field_width, save_x, cancel_x) = Self::edit_geometry(left, width);
        let room = cancel_x - 12 - (left + 14);
        for (index, device) in self.devices.clone().iter().enumerate() {
            let row_y = y + index as i32 * (ROW_HEIGHT + ROW_GAP);
            self.row_card(left, row_y, width, ROW_HEIGHT);
            let button_y = row_y + (ROW_HEIGHT - EDIT_BUTTON) / 2;
            if self.editing == Some(index) {
                // The name swaps for a field holding it, with ✓ to save and ✕ to cancel.
                canvas::round_rect(field_x, row_y + (ROW_HEIGHT - FIELD_HEIGHT) / 2, field_width, FIELD_HEIGHT, CARD_RADIUS as f32, theme.segment_track_on_card);
                self.draw_icon_button(save_x, button_y, ICON_SAVE, theme.accent, Action::SaveName(index));
                self.draw_icon_button(cancel_x, button_y, ICON_CANCEL, theme.muted, Action::CancelName);
            } else {
                // The name in use: the user's rename first, else the device's own. The full name when it
                // fits, and the menu's shortened name only when it does not.
                let in_use = self.settings.renames.get(&device.id).cloned().unwrap_or_else(|| device.name.clone());
                let name = if canvas::text_width(&in_use, &self.fonts.body) <= room { in_use } else { short_name(&in_use) };
                self.draw_device_label(left, row_y, index, &name, &device_hint(device));
                self.draw_icon_button(cancel_x, button_y, ICON_PENCIL, theme.muted, Action::EditName(index));
            }
            let is_shown = !self.settings.hidden.contains(&device.id);
            self.draw_toggle(toggle_right, row_y, ROW_HEIGHT, is_shown);
            self.add_hit(toggle_right - TOGGLE_WIDTH - 8, row_y, TOGGLE_WIDTH + 22, ROW_HEIGHT, Action::ToggleShown(index));
        }
    }

    /// Draws a small round icon button: the symbol alone, with a soft fill under it on hover.
    fn draw_icon_button(&mut self, x: i32, y: i32, icon: &str, color: u32, action: Action) {
        let theme = self.theme;
        if self.is_hover(action) {
            canvas::round_rect(x, y, EDIT_BUTTON, EDIT_BUTTON, CARD_RADIUS as f32, theme.button_hover);
        }
        let ink = if self.is_hover(action) && color == theme.muted { theme.text } else { color };
        canvas::symbol(x + EDIT_BUTTON / 2, y + EDIT_BUTTON / 2, icon, 9.0, ink);
        self.add_hit(x, y, EDIT_BUTTON, EDIT_BUTTON, action);
    }

    /// Draws a device row's name and its second line: the details, or a green "Saved" for a moment after a save.
    fn draw_device_label(&self, left: i32, y: i32, index: usize, name: &str, details: &str) {
        let theme = self.theme;
        let x = left + 14;
        canvas::text(x, y + 7, name, &self.fonts.body, theme.text, Align::Left);
        if self.saved_row == Some(index) {
            canvas::symbol(x + 6, y + 27 + self.fonts.small.box_height / 2, ICON_DONE, 8.0, theme.good);
            canvas::text(x + 16, y + 27, RENAME_SAVED, &self.fonts.small, theme.good, Align::Left);
        } else {
            canvas::text(x, y + 27, details, &self.fonts.small, theme.muted, Align::Left);
        }
    }

    /// Draws a button whose right edge is at `right` and returns its width.
    fn draw_button(&mut self, right: i32, y: i32, label: &str, (fill, text): (u32, u32), action: Action) -> i32 {
        let width = canvas::text_width(label, &self.fonts.body) + 28;
        canvas::round_rect(right - width, y, width, BUTTON_HEIGHT, CARD_RADIUS as f32, fill);
        canvas::text(right - width / 2, y + 5, label, &self.fonts.body, text, Align::Center);
        self.add_hit(right - width, y, width, BUTTON_HEIGHT, action);
        width
    }

    /// Draws the reset row. Just after a reset its hint turns into a green "done" line for a few seconds.
    fn draw_reset_row(&mut self, left: i32, y: i32, width: i32) {
        let theme = self.theme;
        self.row_card(left, y, width, ROW_HEIGHT);
        if self.is_reset_done {
            canvas::text(left + 14, y + 7, "Every setting", &self.fonts.body, theme.text, Align::Left);
            canvas::symbol(left + 20, y + 27 + self.fonts.small.box_height / 2, ICON_DONE, 8.0, theme.good);
            canvas::text(left + 30, y + 27, "Done. Every setting is back to its default.", &self.fonts.small, theme.good, Align::Left);
        } else {
            self.row_label(left, y, "Every setting", "Renames, hidden devices and the check interval");
        }
        let fill = if self.is_hover(Action::AskReset) { theme.button_hover } else { theme.button };
        self.draw_button(left + width - 14, y + (ROW_HEIGHT - BUTTON_HEIGHT) / 2, "Reset all", (fill, theme.text), Action::AskReset);
    }

    /// Draws the app card: the Juice Bar glass on a tinted tile, the name, a line about it and the version.
    fn draw_about_card(&self, left: i32, y: i32, width: i32) {
        let theme = self.theme;
        self.row_card(left, y, width, ABOUT_CARD_HEIGHT);
        let (tile_x, tile_y) = (left + 16, y + (ABOUT_CARD_HEIGHT - APP_TILE) / 2);
        canvas::round_rect(tile_x, tile_y, APP_TILE, APP_TILE, 14.0, mix(theme.card, theme.good, 0.14));
        let glass_width = APP_GLASS_HEIGHT * 16 / 18;
        canvas::glass(tile_x + (APP_TILE - glass_width) / 2, tile_y + (APP_TILE - APP_GLASS_HEIGHT) / 2, APP_GLASS_HEIGHT, theme.text, theme.good, Some(70));
        let x = tile_x + APP_TILE + 16;
        let version = format!("Version {}", juicebar_shared::APP_VERSION);
        canvas::text(x, y + 14, APP_NAME, &self.fonts.title, theme.text, Align::Left);
        canvas::text(x, y + 46, APP_TAGLINE, &self.fonts.body, theme.muted, Align::Left);
        canvas::text(x, y + 68, &version, &self.fonts.small, theme.muted, Align::Left);
    }

    /// Draws the top of the privacy card: a lock on a tinted tile, its promise, and a dividing line.
    fn draw_privacy_header(&self, left: i32, y: i32, width: i32) {
        let theme = self.theme;
        let (tile_x, tile_y) = (left + 14, y + 14);
        canvas::round_rect(tile_x, tile_y, LOCK_TILE, LOCK_TILE, (CARD_RADIUS + 2) as f32, mix(theme.card, theme.accent, 0.18));
        canvas::symbol(tile_x + LOCK_TILE / 2, tile_y + LOCK_TILE / 2, ICON_LOCK, 13.0, theme.accent);
        let text_x = tile_x + LOCK_TILE + 12;
        canvas::text(text_x, tile_y + 1, "Your data stays with you", &self.fonts.strong, theme.text, Align::Left);
        canvas::text(text_x, tile_y + 21, "Private and stored only on this Mac", &self.fonts.small, theme.muted, Align::Left);
        canvas::round_rect(left + 14, y + PRIVACY_HEADER_HEIGHT, width - 28, 1, 0.0, theme.border);
    }

    /// Dims the window and draws the reset question on a card in the middle. Only its two buttons take clicks.
    fn draw_confirm_card(&mut self) {
        let theme = self.theme;
        self.hits.clear();
        canvas::fade_round_rect(0, 0, WIDTH, HEIGHT, 0.0, 0x000000, if theme.is_dark { 0.5 } else { 0.25 });
        let (x, y) = ((WIDTH - CONFIRM_WIDTH) / 2, (HEIGHT - CONFIRM_HEIGHT) / 2 - 20);
        let radius = (CARD_RADIUS + 4) as f32;
        canvas::round_rect(x - 1, y - 1, CONFIRM_WIDTH + 2, CONFIRM_HEIGHT + 2, radius + 1.0, theme.border);
        canvas::round_rect(x, y, CONFIRM_WIDTH, CONFIRM_HEIGHT, radius, theme.card);
        let (tile_x, tile_y) = (x + 20, y + 20);
        canvas::round_rect(tile_x, tile_y, LOCK_TILE, LOCK_TILE, (CARD_RADIUS + 2) as f32, mix(theme.card, theme.danger, 0.16));
        canvas::symbol(tile_x + LOCK_TILE / 2, tile_y + LOCK_TILE / 2, ICON_RESET, 13.0, theme.danger);
        let text_x = tile_x + LOCK_TILE + 12;
        canvas::text(text_x, tile_y + 1, "Reset all settings?", &self.fonts.strong, theme.text, Align::Left);
        canvas::text(text_x, tile_y + 21, "This cannot be undone", &self.fonts.small, theme.muted, Align::Left);
        for (position, line) in RESET_DETAIL.iter().enumerate() {
            canvas::text(x + 20, y + 76 + position as i32 * 20, line, &self.fonts.body, theme.text, Align::Left);
        }
        let button_y = y + CONFIRM_HEIGHT - 20 - BUTTON_HEIGHT;
        let right = x + CONFIRM_WIDTH - 20;
        let reset_fill = if self.is_hover(Action::ConfirmReset) { mix(theme.danger, 0x000000, 0.12) } else { theme.danger };
        let reset_width = self.draw_button(right, button_y, "Reset", (reset_fill, 0xFFFFFF), Action::ConfirmReset);
        let cancel_fill = if self.is_hover(Action::CancelReset) { theme.button_hover } else { theme.button };
        self.draw_button(right - reset_width - 10, button_y, "Cancel", (cancel_fill, theme.text), Action::CancelReset);
    }

    /// Changes the state for a click and returns what must happen once the borrow is dropped.
    fn handle_click(&mut self, x: i32, y: i32) -> Effects {
        let mut effects = Effects::default();
        let action = self.action_at(x, y);
        // Only ✓ saves a rename now; a click anywhere else ends it unsaved.
        if self.editing.is_some() && !matches!(action, Some(Action::SaveName(_) | Action::CancelName | Action::EditName(_))) {
            self.editing = None;
            effects.layout = true;
            self.invalidate();
        }
        let Some(action) = action else {
            return effects;
        };
        match action {
            Action::EditName(index) => {
                self.editing = Some(index);
                self.saved_row = None;
                effects.begins_edit = Some(index);
            }
            Action::SaveName(index) => effects.saves_edit = Some(index),
            Action::CancelName => {
                self.editing = None;
                effects.layout = true;
            }
            Action::Page(page) => {
                self.page = page;
                self.scroll = 0;
                effects.layout = true;
            }
            Action::ToggleLaunchAtLogin => startup::set_enabled(!startup::is_enabled()),
            Action::TogglePercent => {
                self.settings.shows_percent_in_icon = !self.settings.shows_percent_in_icon;
                effects.apply = Some(self.settings.clone());
            }
            Action::SetMenuFont(points) => {
                self.settings.menu_font_points = points.clamp(MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_MAX);
                effects.apply = Some(self.settings.clone());
            }
            Action::SetPoll(minutes) => {
                self.settings.poll_minutes = minutes.clamp(MIN_POLL_MINUTES, MAX_POLL_MINUTES);
                effects.apply = Some(self.settings.clone());
            }
            Action::SetHover(choice) => {
                self.settings.hover_action = HoverAction::from_number(choice);
                effects.apply = Some(self.settings.clone());
            }
            Action::SetClick(choice) => {
                self.settings.click_action = app::click_choice(choice == 1);
                effects.apply = Some(self.settings.clone());
            }
            Action::ToggleShown(index) => {
                let Some(id) = self.devices.get(index).map(|device| device.id.clone()) else {
                    return effects;
                };
                if self.settings.hidden.contains(&id) {
                    self.settings.hidden.retain(|hidden| hidden != &id);
                } else {
                    self.settings.hidden.push(id);
                }
                effects.apply = Some(self.settings.clone());
            }
            Action::AskReset => {
                self.is_confirming_reset = true;
                self.is_reset_done = false;
                effects.layout = true;
            }
            Action::CancelReset => {
                self.is_confirming_reset = false;
                effects.layout = true;
            }
            Action::ConfirmReset => {
                let fresh = Settings { is_saved_to_disk: self.settings.is_saved_to_disk, ..Settings::default() };
                self.settings = fresh.clone();
                self.is_confirming_reset = false;
                self.is_reset_done = true;
                effects = Effects { apply: Some(fresh), layout: true, starts_done_timer: true, ..Effects::default() };
            }
        }
        self.hover = None;
        self.invalidate();
        effects
    }

    /// Returns what is under the mouse. The newest areas sit on top, so they are checked first.
    fn action_at(&self, x: i32, y: i32) -> Option<Action> {
        self.hits.iter().rev().find(|hit| x >= hit.x && x < hit.x + hit.width && y >= hit.y && y < hit.y + hit.height).map(|hit| hit.action)
    }
}

/// Saves the settings and applies them everywhere. If the app state is busy (a scan's redraw ended an edit), it runs a moment later.
fn apply(settings: Settings) {
    if !app::apply_settings(settings.clone()) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            app::apply_settings(settings);
        });
    }
}

/// Ends any rename in progress, which sends that box's action. Must run with WINDOW unborrowed.
fn end_editing() {
    let Some((window, view)) = with_window(|state| (state.window.clone(), state.view.clone())) else {
        return;
    };
    window.makeFirstResponder(Some(&view));
}

/// Moves, shows and hides the rename boxes to match the page. Ends an edit first if its box is about to hide.
fn layout_fields() {
    let Some((placed, is_hiding_an_edit)) = with_window(|state| {
        let placed: Vec<_> = state.fields.iter().cloned().zip(state.field_frames()).collect();
        let is_hiding_an_edit = placed.iter().any(|(field, frame)| frame.is_none() && field.currentEditor().is_some());
        (placed, is_hiding_an_edit)
    }) else {
        return;
    };
    if is_hiding_an_edit {
        end_editing();
    }
    for (field, frame) in placed {
        match frame {
            Some(frame) => {
                field.setFrame(frame);
                field.setHidden(false);
            }
            None => field.setHidden(true),
        }
    }
}

/// Creates one borderless rename box, styled to sit on the drawn box behind it.
fn make_field(index: usize, text: &str, own_name: &str, font: &Font, theme: &Theme) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(text), main_thread());
    field.setBordered(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setFocusRingType(NSFocusRingType::None);
    field.setFont(Some(font.ns_font()));
    field.setTag(FIELD_TAG_BASE + index as isize);
    field.setHidden(true);
    set_placeholder(&field, own_name, font, theme);
    if let Some(cell) = field.cell() {
        cell.setScrollable(true);
        cell.setWraps(false);
        cell.setUsesSingleLineMode(true);
        // Leaving the field cancels; only ✓ or Return saves, so it sends nothing when an edit ends.
        cell.setSendsActionOnEndEditing(false);
    }
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app and answers the text delegate calls (Return, Escape).
    unsafe {
        let _: () = objc2::msg_send![&*field, setDelegate: target];
    }
    field
}

/// Shows the device's own name in grey in an empty box: the name used when there is no rename, so
/// clearing the box visibly falls back to it. Greyed so it never reads as a typed rename.
fn set_placeholder(field: &NSTextField, own_name: &str, font: &Font, theme: &Theme) {
    let color = mix(theme.muted, theme.segment_track_on_card, 0.35);
    field.setPlaceholderAttributedString(Some(&canvas::attributed(own_name, font, color)));
}

/// Redraws every rename box's grey fallback name, after a theme change or a device's own name changing.
fn refresh_placeholders() {
    let Some((fields, names, theme)) = with_window(|state| (state.fields.clone(), state.devices.iter().map(|device| device.name.clone()).collect::<Vec<_>>(), state.theme)) else {
        return;
    };
    let font = Font::system(10.0, WEIGHT_REGULAR);
    fields.iter().zip(&names).for_each(|(field, name)| set_placeholder(field, name, &font, &theme));
}

/// Replaces the rename boxes with one per current device.
fn rebuild_fields() {
    end_editing();
    let Some((view, old, texts, names, theme)) = with_window(|state| {
        let texts: Vec<String> = state.devices.iter().map(|device| state.settings.renames.get(&device.id).cloned().unwrap_or_default()).collect();
        let names: Vec<String> = state.devices.iter().map(|device| device.name.clone()).collect();
        (state.view.clone(), std::mem::take(&mut state.fields), texts, names, state.theme)
    }) else {
        return;
    };
    old.iter().for_each(|field| field.removeFromSuperview());
    let font = Font::system(10.0, WEIGHT_REGULAR);
    let fields: Vec<_> = texts.iter().zip(&names).enumerate().map(|(index, (text, name))| make_field(index, text, name, &font, &theme)).collect();
    fields.iter().for_each(|field| view.addSubview(field));
    with_window(|state| {
        state.editing = None;
        state.saved_row = None;
        state.fields = fields;
    });
}

/// Runs what a click asked for, with WINDOW unborrowed.
fn run_effects(effects: Effects) {
    if let Some(index) = effects.begins_edit {
        begin_edit(index);
    }
    if let Some(index) = effects.saves_edit {
        if let Some(field) = with_window(|state| state.fields.get(index).cloned()).flatten() {
            commit_edit(index, &field.stringValue().to_string());
        }
    }
    if let Some(settings) = effects.apply {
        apply(settings);
    }
    if effects.layout {
        layout_fields();
    }
    if effects.starts_done_timer {
        let timer = start_timer(RESET_NOTE_SECONDS, sel!(hideResetNote:), false);
        with_window(|state| {
            if let Some(old) = state.done_timer.replace(timer) {
                old.invalidate();
            }
        });
    }
}

pub fn draw() {
    with_window(SettingsWindow::draw);
}

/// Reacts to the mouse moving and returns the pointer to show.
pub fn on_mouse_move(x: i32, y: i32) -> Cursor {
    with_window(|state| {
        if state.is_over_field(x, y) {
            return Cursor::Text;
        }
        let hover = state.action_at(x, y);
        if hover != state.hover {
            state.hover = hover;
            state.invalidate();
        }
        if hover.is_some() {
            Cursor::Hand
        } else {
            Cursor::Arrow
        }
    })
    .unwrap_or(Cursor::Arrow)
}

pub fn on_mouse_exit() {
    with_window(|state| {
        state.hover = None;
        state.invalidate();
    });
}

pub fn on_click(x: i32, y: i32) {
    // A click anywhere else finishes a rename first, so the click then works on up to date settings.
    end_editing();
    if let Some(effects) = with_window(|state| state.handle_click(x, y)) {
        run_effects(effects);
    }
}

/// Scrolls the page by the distance the trackpad or wheel reports.
pub fn on_scroll(delta: i32) {
    with_window(|state| {
        if state.is_confirming_reset {
            return;
        }
        let limit = (state.content_height - (HEIGHT - HEADER_HEIGHT)).max(0);
        state.scroll = (state.scroll - delta).clamp(0, limit);
        state.invalidate();
    });
    layout_fields();
}

/// Escape closes the confirm card if it is up, or else the window.
pub fn on_key(code: u16) {
    if code != KEY_ESCAPE {
        return;
    }
    let window = with_window(|state| {
        if state.is_confirming_reset {
            state.is_confirming_reset = false;
            state.invalidate();
            return None;
        }
        Some(state.window.clone())
    });
    match window {
        Some(Some(window)) => window.close(),
        Some(None) => layout_fields(),
        None => {}
    }
}

impl SettingsWindow {
    /// Saves a field's text as the device's rename and returns the new settings when that changed anything.
    /// Empty, or the device's own name, removes the rename, so the own name is used.
    fn save_rename(&mut self, index: usize, text: &str) -> Option<Settings> {
        let device = self.devices.get(index)?;
        let id = device.id.clone();
        self.invalidate();
        let name = text.trim();
        let name = if name == device.name { "" } else { name };
        if self.settings.renames.get(&id).map_or("", String::as_str) == name {
            return None;
        }
        if name.is_empty() {
            self.settings.renames.remove(&id);
        } else {
            self.settings.renames.insert(id, name.to_string());
        }
        Some(self.settings.clone())
    }

    /// Returns the name a row's field starts with: the rename, else the device's own name.
    fn name_in_use(&self, index: usize) -> Option<String> {
        let device = self.devices.get(index)?;
        Some(self.settings.renames.get(&device.id).cloned().unwrap_or_else(|| device.name.clone()))
    }
}

/// Starts renaming a row: its field shows the name in use, all selected, with the keyboard. Runs with WINDOW unborrowed.
fn begin_edit(index: usize) {
    let Some((window, field, name)) = with_window(|state| Some((state.window.clone(), state.fields.get(index)?.clone(), state.name_in_use(index)?))).flatten() else {
        return;
    };
    field.setStringValue(&NSString::from_str(&name));
    layout_fields();
    window.makeFirstResponder(Some(&field));
    if let Some(editor) = field.currentEditor() {
        // SAFETY: a nil sender is allowed.
        unsafe { editor.selectAll(None) };
    }
}

/// Saves a row's rename from ✓ or Return, ends the edit, and shows "Saved" on the row. Runs with WINDOW unborrowed.
fn commit_edit(index: usize, text: &str) {
    let settings = with_window(|state| {
        let settings = state.save_rename(index, text);
        state.editing = None;
        state.saved_row = Some(index);
        settings
    });
    let Some(settings) = settings else {
        return;
    };
    // The field hides here, which ends its edit; it sends nothing, so nothing saves twice.
    layout_fields();
    if let Some(settings) = settings {
        apply(settings);
    }
    start_saved_cue();
}

/// Ends a rename without saving: ✕, Escape, a click elsewhere, a page change. Runs with WINDOW unborrowed.
fn cancel_edit() {
    with_window(|state| {
        state.editing = None;
        state.invalidate();
    });
    layout_fields();
}

/// Returns which rename field a control is.
fn field_index(control: &NSControl) -> Option<usize> {
    usize::try_from(control.tag() - FIELD_TAG_BASE).ok()
}

/// Starts (or restarts) the 2 second "Saved" cue. Runs with WINDOW unborrowed.
fn start_saved_cue() {
    let timer = start_timer(RENAME_SAVED_SECONDS, sel!(renameSavedExpired:), false);
    with_window(|state| {
        if let Some(old) = state.saved_timer.replace(timer) {
            old.invalidate();
        }
    });
}

/// Return in a rename field: saves, as ✓ does. Returns true, as the key is handled.
pub fn on_rename_return(control: &NSControl, text_view: &NSTextView) -> bool {
    let Some(index) = field_index(control) else {
        return false;
    };
    commit_edit(index, &text_view.string().to_string());
    true
}

/// Escape in a rename field: cancels, as ✕ does, and saves nothing. Returns true, as the key is handled.
pub fn on_rename_cancel(control: &NSControl, _text_view: &NSTextView) -> bool {
    if field_index(control).is_none() {
        return false;
    }
    cancel_edit();
    true
}

/// Hides the "Saved" cue after its moment.
pub fn hide_rename_saved() {
    with_window(|state| {
        state.saved_row = None;
        state.saved_timer = None;
        state.invalidate();
    });
}

/// Builds the window and its state without showing it.
fn build(settings: Settings, devices: Vec<Device>) -> Retained<NSWindow> {
    let (window, view) = surface::create_window(WIDTH, HEIGHT, WINDOW_TITLE);
    let theme = current_theme();
    apply_title_bar(&window, &theme);
    let state = SettingsWindow {
        window: window.clone(),
        view,
        settings,
        devices,
        fields: Vec::new(),
        fonts: Fonts::new(),
        theme,
        page: 0,
        scroll: 0,
        content_height: 0,
        hits: Vec::new(),
        hover: None,
        is_confirming_reset: false,
        is_reset_done: false,
        done_timer: None,
        editing: None,
        saved_row: None,
        saved_timer: None,
    };
    WINDOW.with(|cell| *cell.borrow_mut() = Some(state));
    rebuild_fields();
    layout_fields();
    window
}

/// Opens the settings window, or brings it forward if it is open.
pub fn open_settings() {
    NSApplication::sharedApplication(main_thread()).activate();
    if let Some(window) = with_window(|state| state.window.clone()) {
        window.makeKeyAndOrderFront(None);
        return;
    }
    let Some((settings, devices)) = app::settings_and_devices() else {
        return;
    };
    let window = build(settings, devices);
    window.center();
    sync_dock_entry();
    window.makeKeyAndOrderFront(None);
    // macOS can refuse the activation above, and a window asked to come forward then stays hidden.
    window.orderFrontRegardless();
}

/**
 * Puts the app in the Dock while Settings is open, so it can be found again, and takes it out when the
 * window closes. The menu bar item keeps running until the user quits. Copied from NetSpeedGauge.
 */
fn sync_dock_entry() {
    let application = NSApplication::sharedApplication(main_thread());
    let is_open = WINDOW.with(|cell| cell.try_borrow().map(|state| state.is_some()).unwrap_or(false));
    let wanted = if is_open { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
    if application.activationPolicy() == wanted {
        return;
    }
    application.setActivationPolicy(wanted);
    if wanted == NSApplicationActivationPolicy::Accessory {
        application.deactivate();
    } else {
        // Changing to having a Dock entry drops the focus the new window has just asked for.
        // SAFETY: the selector exists on every macOS version.
        let _: () = unsafe { objc2::msg_send![&*application, activateIgnoringOtherApps: true] };
    }
}

/// Forgets the window once it closes, so the next open shows fresh rows. A rename being typed is dropped, unsaved.
pub fn on_closed() {
    end_editing();
    let closed = WINDOW.with(|cell| cell.try_borrow_mut().ok().and_then(|mut state| state.take()));
    if let Some(state) = closed {
        if let Some(timer) = &state.done_timer {
            timer.invalidate();
        }
        drop(state);
    }
    sync_dock_entry();
}

/// Hides the reset done cue a few seconds after a reset.
pub fn hide_reset_note() {
    with_window(|state| {
        state.is_reset_done = false;
        state.done_timer = None;
        state.invalidate();
    });
}

/// Follows the Mac switching between light and dark.
pub fn apply_theme() {
    let changed = with_window(|state| {
        state.theme = current_theme();
        state.invalidate();
        (state.window.clone(), state.theme)
    });
    if let Some((window, theme)) = changed {
        apply_title_bar(&window, &theme);
        refresh_placeholders();
    }
}

/// Updates the open window when devices or settings change: new devices appear without reopening.
pub fn on_devices_changed(settings: &Settings, devices: &[Device]) {
    let Some(is_new_set) = with_window(|state| state.devices.len() != devices.len() || state.devices.iter().zip(devices).any(|(old, new)| old.id != new.id)) else {
        return;
    };
    if is_new_set {
        // Finish a rename while the boxes still match the old devices, so it lands on the right one.
        end_editing();
    }
    with_window(|state| {
        state.settings = settings.clone();
        state.devices = devices.to_vec();
        state.invalidate();
    });
    if is_new_set {
        rebuild_fields();
        layout_fields();
    }
    refresh_placeholders();
}

/// Paints the settings window to a PNG without the screen. Used by capture mode and the capture example.
pub fn capture_to(path: &str) -> bool {
    // The state must be free while painting, since painting reads it.
    let Some(view) = with_window(|state| state.view.clone()) else {
        return false;
    };
    surface::capture_view(&view, path)
}

/// Builds the window from given settings and devices without showing it, for checks.
pub fn open_for_checks(settings: Settings, devices: Vec<Device>) {
    build(settings, devices);
}

/// Switches page, for checks.
pub fn show_page_for_checks(page: usize) {
    with_window(|state| {
        state.page = page.min(PAGES.len() - 1);
        state.scroll = 0;
        state.invalidate();
    });
    layout_fields();
}

/// Shows the open window in a chosen theme, for checks.
pub fn preview_theme(is_dark: bool) {
    let changed = with_window(|state| {
        state.theme = if is_dark { dark_theme() } else { light_theme() };
        state.invalidate();
        (state.window.clone(), state.theme)
    });
    if let Some((window, theme)) = changed {
        apply_title_bar(&window, &theme);
        refresh_placeholders();
    }
}

/// Clicks an area as a user would, for checks: "page:N", "percent", "poll:N", "menu_font:N", "hover:N", "click:N", "show:N", "ask_reset", "cancel_reset", "confirm_reset".
/// Returns false when no such area is showing.
pub fn click_for_checks(name: &str) -> bool {
    let (kind, number) = name.split_once(':').map_or((name, 0), |(kind, number)| (kind, number.parse().unwrap_or(0)));
    let wanted = match kind {
        "page" => Action::Page(number),
        "percent" => Action::TogglePercent,
        "poll" => Action::SetPoll(number as u32),
        "menu_font" => Action::SetMenuFont(number as u32),
        "hover" => Action::SetHover(number as u8),
        "click" => Action::SetClick(number as u8),
        "show" => Action::ToggleShown(number),
        "edit" => Action::EditName(number),
        "save" => Action::SaveName(number),
        "cancel_name" => Action::CancelName,
        "ask_reset" => Action::AskReset,
        "cancel_reset" => Action::CancelReset,
        "confirm_reset" => Action::ConfirmReset,
        _ => return false,
    };
    // Painting fills in where each area is.
    let Some(view) = with_window(|state| state.view.clone()) else {
        return false;
    };
    surface::paint_off_screen(&view);
    let position = with_window(|state| state.hits.iter().find(|hit| hit.action == wanted).map(|hit| (hit.x + hit.width / 2, hit.y + hit.height / 2))).flatten();
    if let Some((x, y)) = position {
        on_click(x, y);
    }
    position.is_some()
}

/// Opens a row's rename with its pencil and types a name without saving it, for checks.
pub fn type_rename_for_checks(index: usize, text: &str) -> bool {
    // Opens the row's field with its pencil first, as a user would.
    if with_window(|state| state.editing) != Some(Some(index)) {
        click_for_checks(&format!("edit:{index}"));
    }
    let Some(field) = with_window(|state| state.fields.get(index).cloned()).flatten() else {
        return false;
    };
    let Some(editor) = field.currentEditor() else {
        return false;
    };
    // Typed through the field editor as keys would be, replacing the selected name.
    // SAFETY: the field editor is an NSTextView, which answers both; a nil sender is allowed.
    unsafe {
        editor.selectAll(None);
        let _: () = objc2::msg_send![&*editor, insertText: &*NSString::from_str(text)];
    }
    true
}

/// Sends Return or Escape to the rename box being edited, through the same path a key press takes, for checks.
pub fn press_in_rename_for_checks(index: usize, is_return: bool) -> bool {
    let Some(field) = with_window(|state| state.fields.get(index).cloned()).flatten() else {
        return false;
    };
    let Some(editor) = field.currentEditor() else {
        return false;
    };
    let command = if is_return { sel!(insertNewline:) } else { sel!(cancelOperation:) };
    // SAFETY: doCommandBySelector: is NSTextView's own, which asks the box's delegate first.
    let _: () = unsafe { objc2::msg_send![&*editor, doCommandBySelector: command] };
    true
}

/// Clicks an empty spot of the page, as a user would to leave a rename, for checks.
pub fn leave_rename_for_checks() {
    on_click(WIDTH - 40, HEIGHT - 40);
}

/// Returns the row being renamed, for checks.
pub fn editing_row_for_checks() -> Option<usize> {
    with_window(|state| state.editing).flatten()
}

/// Paints the open window in both looks to `<base>_dark.png` and `<base>_light.png`, then restores its look, for checks.
pub fn capture_both_looks_for_checks(base: &str) -> bool {
    let restore = with_window(|state| state.theme.is_dark).unwrap_or(true);
    let mut saved = true;
    for (name, is_dark) in [("dark", true), ("light", false)] {
        preview_theme(is_dark);
        saved &= capture_to(&format!("{base}_{name}.png"));
    }
    preview_theme(restore);
    saved
}

/// Returns whether a rename box is being edited, for checks.
pub fn is_rename_focused_for_checks(index: usize) -> bool {
    with_window(|state| state.fields.get(index).cloned()).flatten().is_some_and(|field| field.currentEditor().is_some())
}

/// Presses Escape in the window, for checks.
pub fn press_escape_for_checks() {
    on_key(KEY_ESCAPE);
}

/// Returns whether the reset done cue is showing, for checks.
pub fn is_reset_done_for_checks() -> bool {
    with_window(|state| state.is_reset_done).unwrap_or(false)
}

/// Returns the settings the window shows, for checks.
pub fn settings_for_checks() -> Option<Settings> {
    with_window(|state| state.settings.clone())
}
