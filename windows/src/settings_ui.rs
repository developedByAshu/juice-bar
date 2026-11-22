//! The settings window, in the look of Windows 11 Settings: Mica behind, a page list on the left,
//! and rounded cards on the right. Pages: General, Devices, About.

use crate::canvas::{create_font, mix, Align, Canvas};
use crate::confirm_sheet::{confirm_click, confirm_hover, confirm_key, dismiss_confirm, is_confirm_open, paint_confirm, show_confirm, wrap_text, SheetFonts};
use crate::glyph::glass_image;
use crate::menu_ui::connection_label;
use crate::messages::WM_APP_SETTINGS_CHANGED;
use crate::startup;
use crate::theme::{apply_window_look, background_opacity, current_theme, font_family, icon_font_family, on_theme_change, tone_color, Backdrop, Theme, GREEN};
use crate::util::{scale, to_wide};
use juicebar_shared::device::{Device, NameSource};
use juicebar_shared::settings::Settings;
use juicebar_shared::tone::tone_for_percent;
use std::cell::RefCell;
use std::rc::Rc;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect, SetBkColor, SetTextColor, HBRUSH, HDC, HFONT, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::EM_SETSEL;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowTextLengthW, GetWindowTextW, IsWindow, LoadCursorW, LoadIconW,
    PostMessageW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, CW_USEDEFAULT, ES_AUTOHSCROLL,
    IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WM_CHAR, WM_CLOSE, WM_CTLCOLOREDIT, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_GETDLGCODE, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
    WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE, DLGC_WANTALLKEYS,
};

const CLASS_NAME: &str = "JuiceBarSettings";
const WIDTH: i32 = 760;
const HEIGHT: i32 = 520;
const NAV_WIDTH: i32 = 200;
const NAV_ITEM_HEIGHT: i32 = 36;
const OUTER: i32 = 16;
const CONTENT_LEFT: i32 = NAV_WIDTH + 24;
const CONTENT_RIGHT_GAP: i32 = 24;
const TITLE_TOP: i32 = 12;
const CARDS_TOP: i32 = 68;
const CARD_HEIGHT: i32 = 68;
const DEVICE_CARD_HEIGHT: i32 = 76;
const CARD_GAP: i32 = 4;
const CARD_RADIUS: f32 = 6.0;
const CARD_PADDING: i32 = 18;
const BUTTON_HEIGHT: i32 = 32;
const TOGGLE_WIDTH: i32 = 40;
const TOGGLE_HEIGHT: i32 = 20;
const STEPPER_BUTTON: i32 = 32;
const STEPPER_VALUE_WIDTH: i32 = 64;
const SCROLL_STEP: i32 = 48;
const EDIT_SUBCLASS: usize = 1;
const REPO_URL: &str = "https://github.com/developedByAshu/juice-bar";
const LICENSE_URL: &str = "https://github.com/developedByAshu/juice-bar/blob/main/LICENSE";
const ICON_GENERAL: &str = "\u{E713}";
const ICON_DEVICES: &str = "\u{E772}";
const ICON_ABOUT: &str = "\u{E946}";
const ICON_HEART: &str = "\u{EB52}";
const HEART_COLOR: u32 = 0xE5484D;
const MIN_POLL_MINUTES: u32 = 1;
const MAX_POLL_MINUTES: u32 = 60;

/// The pages, in the order of the list on the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Devices,
    About,
}

const PAGES: [(Page, &str, &str); 3] = [(Page::General, ICON_GENERAL, "General"), (Page::Devices, ICON_DEVICES, "Devices"), (Page::About, ICON_ABOUT, "About")];

/// Something on a page that reacts to the pointer or keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Nav(Page),
    PollLess,
    PollMore,
    PercentToggle,
    StartupToggle,
    Reset,
    Rename(usize),
    ShowToggle(usize),
    ConfirmName(usize),
    Link(&'static str),
}

struct Fonts {
    title: HFONT,
    body: HFONT,
    strong: HFONT,
    caption: HFONT,
    icon: HFONT,
    big: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        let family = font_family();
        let display = if family == "Segoe UI" { "Segoe UI" } else { "Segoe UI Variable Display" };
        Self {
            title: create_font(display, 20.0, 600, dpi),
            body: create_font(family, 10.5, 400, dpi),
            strong: create_font(family, 10.5, 600, dpi),
            caption: create_font(family, 9.0, 400, dpi),
            icon: create_font(icon_font_family(), 12.0, 400, dpi),
            big: create_font(display, 14.0, 600, dpi),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: each font was created in new() and is released once here.
        unsafe {
            for font in [self.title, self.body, self.strong, self.caption, self.icon, self.big] {
                DeleteObject(font);
            }
        }
    }
}

struct Window {
    window: HWND,
    owner: HWND,
    settings: Rc<RefCell<Settings>>,
    devices: Rc<RefCell<Vec<Device>>>,
    page: Page,
    dpi: u32,
    fonts: Fonts,
    theme: Theme,
    hovered: Option<Control>,
    focused: Option<Control>,
    scroll: i32,
    /// The rename box and the device it renames, while open.
    edit: Option<(HWND, String)>,
    edit_brush: HBRUSH,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static LISTENING: RefCell<bool> = const { RefCell::new(false) };
}

fn with_window<T>(action: impl FnOnce(&mut Window) -> T) -> Option<T> {
    WINDOW.with(|cell| cell.borrow_mut().as_mut().map(action))
}

fn invalidate() {
    with_window(|state| {
        // SAFETY: the window is alive while it is stored.
        unsafe { InvalidateRect(state.window, std::ptr::null(), 0) };
    });
}

/// Returns the devices in Settings order: as read, hidden ones included.
fn device_list(state: &Window) -> Vec<Device> {
    state.devices.borrow().clone()
}

/// Returns each control and its rectangle on the current page, scrolled.
fn layout(state: &Window, canvas: &Canvas) -> Vec<(Control, RECT)> {
    let dpi = state.dpi;
    let mut controls = Vec::new();
    for (index, (page, _, _)) in PAGES.iter().enumerate() {
        let top = scale(CARDS_TOP, dpi) + index as i32 * scale(NAV_ITEM_HEIGHT + 4, dpi);
        controls.push((Control::Nav(*page), RECT { left: scale(OUTER, dpi) - scale(4, dpi), top, right: scale(NAV_WIDTH, dpi), bottom: top + scale(NAV_ITEM_HEIGHT, dpi) }));
    }
    let left = scale(CONTENT_LEFT, dpi);
    let right = canvas.width() - scale(CONTENT_RIGHT_GAP, dpi);
    let padding = scale(CARD_PADDING, dpi);
    let card = |index: i32, height: i32| {
        let top = scale(CARDS_TOP, dpi) + index * (scale(height, dpi) + scale(CARD_GAP, dpi)) - state.scroll;
        (top, top + scale(height, dpi))
    };
    let middle_box = |top: i32, bottom: i32, width: i32, height: i32, right_edge: i32| {
        let box_top = top + (bottom - top - height) / 2;
        RECT { left: right_edge - width, top: box_top, right: right_edge, bottom: box_top + height }
    };
    match state.page {
        Page::General => {
            let (top, bottom) = card(0, CARD_HEIGHT);
            let more = middle_box(top, bottom, scale(STEPPER_BUTTON, dpi), scale(STEPPER_BUTTON, dpi), right - padding);
            let less_right = more.left - scale(STEPPER_VALUE_WIDTH, dpi);
            controls.push((Control::PollLess, RECT { left: less_right - scale(STEPPER_BUTTON, dpi), right: less_right, ..more }));
            controls.push((Control::PollMore, more));
            for (index, control) in [(1, Control::PercentToggle), (2, Control::StartupToggle)] {
                let (top, bottom) = card(index, CARD_HEIGHT);
                controls.push((control, middle_box(top, bottom, scale(TOGGLE_WIDTH, dpi), scale(TOGGLE_HEIGHT, dpi), right - padding)));
            }
            let (top, bottom) = card(3, CARD_HEIGHT);
            controls.push((Control::Reset, middle_box(top, bottom, scale(96, dpi), scale(BUTTON_HEIGHT, dpi), right - padding)));
        }
        Page::Devices => {
            let devices = device_list(state);
            let settings = state.settings.borrow();
            for (index, device) in devices.iter().enumerate() {
                let (top, bottom) = card(index as i32, DEVICE_CARD_HEIGHT);
                let toggle = middle_box(top, bottom, scale(TOGGLE_WIDTH, dpi), scale(TOGGLE_HEIGHT, dpi), right - padding);
                controls.push((Control::ShowToggle(index), toggle));
                let rename_right = toggle.left - scale(56, dpi);
                let rename = middle_box(top, bottom, scale(84, dpi), scale(BUTTON_HEIGHT, dpi), rename_right);
                controls.push((Control::Rename(index), rename));
                if needs_name_confirm(device, &settings) {
                    let confirm = middle_box(top, bottom, scale(124, dpi), scale(BUTTON_HEIGHT, dpi), rename.left - scale(8, dpi));
                    controls.push((Control::ConfirmName(index), confirm));
                }
            }
        }
        Page::About => {
            let top = scale(CARDS_TOP + 150, dpi);
            let links = [("GitHub", REPO_URL), ("Licence (GPL-3.0)", LICENSE_URL)];
            let mut x = left;
            for (label, url) in links {
                let width = canvas.text_width(label, state.fonts.body) + 2 * scale(14, dpi);
                controls.push((Control::Link(url), RECT { left: x, top, right: x + width, bottom: top + scale(BUTTON_HEIGHT, dpi) }));
                x += width + scale(8, dpi);
            }
        }
    }
    controls
}

/// Checks whether a device's name came from Bluetooth memory and still waits for the user's yes.
fn needs_name_confirm(device: &Device, settings: &Settings) -> bool {
    device.name_source == NameSource::BluetoothMemory && !settings.confirmed_bluetooth_names.contains_key(&device.id)
}

fn link_label(url: &str) -> &'static str {
    if url == REPO_URL {
        "GitHub"
    } else {
        "Licence (GPL-3.0)"
    }
}

fn draw_card(canvas: &mut Canvas, theme: &Theme, left: i32, top: i32, right: i32, bottom: i32) {
    let (fill, opacity) = if theme.has_backdrop { (0xFFFFFF, if theme.is_dark { 0.05 } else { 0.7 }) } else { (theme.card, 1.0) };
    canvas.round_rect(left, top, right - left, bottom - top, CARD_RADIUS, fill, opacity);
    canvas.round_outline(left, top, right - left, bottom - top, CARD_RADIUS, 1.0, 0x000000, if theme.is_dark { 0.2 } else { 0.06 });
}

fn draw_button(canvas: &mut Canvas, theme: &Theme, rect: &RECT, label: &str, font: HFONT, is_hovered: bool, is_accent: bool) {
    let (fill, text) = if is_accent {
        (theme.accent, theme.on_accent)
    } else if theme.is_dark {
        (0x3A3A3A, theme.text)
    } else {
        (0xFBFBFB, theme.text)
    };
    let fill = if is_hovered { mix(fill, if theme.is_dark { 0xFFFFFF } else { 0x000000 }, 0.06) } else { fill };
    canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, 4.0, fill, 1.0);
    if !is_accent {
        canvas.round_outline(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, 4.0, 1.0, theme.border, 1.0);
    }
    let height = canvas.text_height(font);
    canvas.text((rect.left + rect.right) / 2, rect.top + (rect.bottom - rect.top - height) / 2, label, font, text, Align::Center);
}

fn draw_toggle(canvas: &mut Canvas, theme: &Theme, rect: &RECT, is_on: bool, is_hovered: bool, font: HFONT, dpi: u32) {
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    let knob = if is_hovered { scale(14, dpi) } else { scale(12, dpi) };
    let knob_top = rect.top + (height - knob) / 2;
    if is_on {
        canvas.round_rect(rect.left, rect.top, width, height, height as f32 / 2.0, theme.accent, 1.0);
        canvas.round_rect(rect.right - scale(4, dpi) - knob, knob_top, knob, knob, knob as f32 / 2.0, theme.on_accent, 1.0);
    } else {
        canvas.round_outline(rect.left, rect.top, width, height, height as f32 / 2.0, 1.0, theme.muted, 1.0);
        canvas.round_rect(rect.left + scale(4, dpi), knob_top, knob, knob, knob as f32 / 2.0, theme.muted, 1.0);
    }
    let label = if is_on { "On" } else { "Off" };
    let text_height = canvas.text_height(font);
    canvas.text(rect.left - scale(12, dpi), rect.top + (height - text_height) / 2, label, font, theme.text, Align::Right);
}

fn draw_focus(canvas: &mut Canvas, theme: &Theme, rect: &RECT, dpi: u32) {
    let ring = scale(2, dpi);
    canvas.round_outline(rect.left - ring - 1, rect.top - ring - 1, rect.right - rect.left + 2 * ring + 2, rect.bottom - rect.top + 2 * ring + 2, 6.0, ring as f32, theme.text, 1.0);
}

/// Draws a card's title and its one-line description, left of its control.
fn draw_card_text(canvas: &mut Canvas, theme: &Theme, fonts: &Fonts, left: i32, top: i32, bottom: i32, title: &str, detail: &str, max_width: i32) {
    let title_height = canvas.text_height(fonts.body);
    let detail_height = canvas.text_height(fonts.caption);
    let block = title_height + if detail.is_empty() { 0 } else { detail_height + 2 };
    let y = top + (bottom - top - block) / 2;
    canvas.text(left, y, title, fonts.body, theme.text, Align::Left);
    if !detail.is_empty() {
        let line = wrap_text(canvas, detail, fonts.caption, max_width).into_iter().next().unwrap_or_default();
        canvas.text(left, y + title_height + 2, &line, fonts.caption, theme.muted, Align::Left);
    }
}

fn paint(state: &Window, canvas: &mut Canvas) {
    let theme = state.theme;
    let dpi = state.dpi;
    let fonts = &state.fonts;
    canvas.clear(theme.background, background_opacity(&theme));
    let controls = layout(state, canvas);
    let rect_of = |wanted: Control| controls.iter().find(|(control, _)| *control == wanted).map(|(_, rect)| *rect);
    let is_hovered = |control: Control| state.hovered == Some(control);
    let show_focus = |control: Control| state.focused == Some(control);
    let left = scale(CONTENT_LEFT, dpi);
    let right = canvas.width() - scale(CONTENT_RIGHT_GAP, dpi);
    let padding = scale(CARD_PADDING, dpi);
    let settings = state.settings.borrow().clone();

    // Page content first, so the page list and title stay on top when the cards scroll.
    match state.page {
        Page::General => {
            let rows = [
                ("Check batteries every", "Juice Bar sleeps in between, so it costs no power"),
                ("Show the lowest % in the icon", "Shows the number instead of the juice glass"),
                ("Launch at login", "Starts Juice Bar when you sign in to Windows"),
                ("Reset all settings", "Renames, hidden devices and choices go back to how they were"),
            ];
            for (index, (title, detail)) in rows.iter().enumerate() {
                let top = scale(CARDS_TOP, dpi) + index as i32 * (scale(CARD_HEIGHT, dpi) + scale(CARD_GAP, dpi));
                let bottom = top + scale(CARD_HEIGHT, dpi);
                draw_card(canvas, &theme, left, top, right, bottom);
                draw_card_text(canvas, &theme, fonts, left + padding, top, bottom, title, detail, right - left - padding * 2 - scale(170, dpi));
            }
            if let (Some(less), Some(more)) = (rect_of(Control::PollLess), rect_of(Control::PollMore)) {
                draw_button(canvas, &theme, &less, "\u{2212}", fonts.body, is_hovered(Control::PollLess), false);
                draw_button(canvas, &theme, &more, "+", fonts.body, is_hovered(Control::PollMore), false);
                let value = format!("{} min", settings.poll_minutes);
                let height = canvas.text_height(fonts.body);
                canvas.text((less.right + more.left) / 2, less.top + (less.bottom - less.top - height) / 2, &value, fonts.body, theme.text, Align::Center);
                for control in [Control::PollLess, Control::PollMore] {
                    if show_focus(control) {
                        draw_focus(canvas, &theme, &rect_of(control).unwrap_or(less), dpi);
                    }
                }
            }
            for (control, is_on) in [(Control::PercentToggle, settings.shows_percent_in_icon), (Control::StartupToggle, startup::is_enabled())] {
                if let Some(rect) = rect_of(control) {
                    draw_toggle(canvas, &theme, &rect, is_on, is_hovered(control), fonts.body, dpi);
                    if show_focus(control) {
                        draw_focus(canvas, &theme, &rect, dpi);
                    }
                }
            }
            if let Some(rect) = rect_of(Control::Reset) {
                draw_button(canvas, &theme, &rect, "Reset", fonts.body, is_hovered(Control::Reset), false);
                if show_focus(Control::Reset) {
                    draw_focus(canvas, &theme, &rect, dpi);
                }
            }
        }
        Page::Devices => {
            let devices = device_list(state);
            if devices.is_empty() {
                canvas.text(left, scale(CARDS_TOP, dpi), "No wireless mouse or keyboard found yet. Plug in a dongle and it shows up here.", fonts.body, theme.muted, Align::Left);
            }
            for (index, device) in devices.iter().enumerate() {
                let top = scale(CARDS_TOP, dpi) + index as i32 * (scale(DEVICE_CARD_HEIGHT, dpi) + scale(CARD_GAP, dpi)) - state.scroll;
                let bottom = top + scale(DEVICE_CARD_HEIGHT, dpi);
                if bottom < scale(CARDS_TOP, dpi) || top > canvas.height() {
                    continue;
                }
                draw_card(canvas, &theme, left, top, right, bottom);
                let reading = match device.battery {
                    Some(battery) => format!("{}%{}", battery.percent, if battery.is_charging { " charging" } else { "" }),
                    None if device.is_unrecognised() => "Not recognised yet".to_string(),
                    None => "Not connected".to_string(),
                };
                let mut detail = format!("{}  \u{00B7}  {reading}", connection_label(device.connection));
                if needs_name_confirm(device, &settings) {
                    detail.push_str("  \u{00B7}  name from Bluetooth");
                }
                let is_hidden = settings.hidden.contains(&device.id);
                let text_right = rect_of(Control::ConfirmName(index)).or(rect_of(Control::Rename(index))).map_or(right, |rect| rect.left) - scale(12, dpi);
                let dot_x = left + padding + scale(4, dpi);
                let middle = (top + bottom) / 2;
                match device.battery {
                    Some(battery) => canvas.circle(dot_x, middle, scale(4, dpi), tone_color(tone_for_percent(battery.percent), &theme)),
                    None => canvas.round_outline(dot_x - scale(4, dpi), middle - scale(4, dpi), scale(8, dpi), scale(8, dpi), scale(4, dpi) as f32, 1.5, theme.muted, 1.0),
                }
                let is_renaming = state.edit.as_ref().is_some_and(|(_, id)| id == &device.id);
                let title = if is_renaming { String::new() } else { device.name.clone() };
                draw_card_text(canvas, &theme, fonts, left + padding + scale(18, dpi), top, bottom, &title, &detail, text_right - left - padding - scale(18, dpi));
                if let Some(rect) = rect_of(Control::Rename(index)) {
                    draw_button(canvas, &theme, &rect, "Rename", fonts.body, is_hovered(Control::Rename(index)), false);
                }
                if let Some(rect) = rect_of(Control::ConfirmName(index)) {
                    draw_button(canvas, &theme, &rect, "Keep this name", fonts.body, is_hovered(Control::ConfirmName(index)), true);
                }
                if let Some(rect) = rect_of(Control::ShowToggle(index)) {
                    draw_toggle(canvas, &theme, &rect, !is_hidden, is_hovered(Control::ShowToggle(index)), fonts.caption, dpi);
                    let caption_height = canvas.text_height(fonts.caption);
                    let _ = caption_height;
                }
                for control in [Control::Rename(index), Control::ConfirmName(index), Control::ShowToggle(index)] {
                    if show_focus(control) {
                        if let Some(rect) = rect_of(control) {
                            draw_focus(canvas, &theme, &rect, dpi);
                        }
                    }
                }
            }
        }
        Page::About => {
            let top = scale(CARDS_TOP, dpi);
            let icon_size = scale(64, dpi) as u32;
            canvas.draw_pixels(left, top, icon_size as i32, &glass_image(icon_size, theme.text, GREEN, Some(75)));
            let text_left = left + icon_size as i32 + scale(16, dpi);
            canvas.text(text_left, top + scale(4, dpi), "Juice Bar", fonts.big, theme.text, Align::Left);
            canvas.text(text_left, top + scale(32, dpi), &format!("Version {}", juicebar_shared::APP_VERSION), fonts.body, theme.muted, Align::Left);
            let description = "Battery % of wireless mice and keyboards, in the tray. Free and open source.";
            canvas.text(left, top + scale(96, dpi), description, fonts.body, theme.text, Align::Left);
            canvas.text(left, top + scale(120, dpi), "No account, no telemetry. Report sends only device ids, and only when you choose.", fonts.caption, theme.muted, Align::Left);
            for (control, rect) in &controls {
                if let Control::Link(url) = control {
                    draw_button(canvas, &theme, rect, link_label(url), fonts.body, is_hovered(*control), false);
                    if show_focus(*control) {
                        draw_focus(canvas, &theme, rect, dpi);
                    }
                }
            }
            let love = "Made in India with ";
            let love_y = top + scale(214, dpi);
            canvas.text(left, love_y, love, fonts.caption, theme.muted, Align::Left);
            let love_width = canvas.text_width(love, fonts.caption);
            canvas.text(left + love_width, love_y + scale(2, dpi), ICON_HEART, fonts.caption_icon(dpi), HEART_COLOR, Align::Left);
        }
    }

    // A clean band behind the title, so scrolled cards pass under it.
    if state.page == Page::Devices && state.scroll > 0 {
        canvas.round_rect(left - scale(8, dpi), 0, right - left + scale(16, dpi), scale(CARDS_TOP - 4, dpi), 0.0, theme.background, if theme.has_backdrop { 0.85 } else { 1.0 });
    }
    let title = PAGES.iter().find(|(page, _, _)| *page == state.page).map_or("", |(_, _, title)| title);
    canvas.text(left, scale(TITLE_TOP, dpi), title, fonts.title, theme.text, Align::Left);

    for (page, icon, label) in PAGES {
        let Some(rect) = rect_of(Control::Nav(page)) else { continue };
        let is_selected = state.page == page;
        if is_selected || is_hovered(Control::Nav(page)) {
            let opacity = if theme.is_dark { if is_selected { 0.06 } else { 0.04 } } else if is_selected { 0.05 } else { 0.03 };
            canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, 4.0, theme.hover, opacity);
        }
        if is_selected {
            let bar_height = scale(16, dpi);
            canvas.round_rect(rect.left, rect.top + (rect.bottom - rect.top - bar_height) / 2, scale(3, dpi), bar_height, 1.5, theme.accent_text(), 1.0);
        }
        if show_focus(Control::Nav(page)) {
            draw_focus(canvas, &theme, &rect, dpi);
        }
        let middle = (rect.top + rect.bottom) / 2;
        canvas.text(rect.left + scale(14, dpi), middle - canvas.text_height(fonts.icon) / 2, icon, fonts.icon, theme.text, Align::Left);
        canvas.text(rect.left + scale(44, dpi), middle - canvas.text_height(fonts.body) / 2, label, fonts.body, theme.text, Align::Left);
    }

    if is_confirm_open() {
        let sheet_fonts = SheetFonts { title: fonts.big, body: fonts.body };
        paint_confirm(canvas, &theme, &sheet_fonts, dpi, true);
    }
}

impl Fonts {
    /// The heart in the About line, from the icon font at caption size.
    fn caption_icon(&self, _dpi: u32) -> HFONT {
        self.icon
    }
}

/// Saves the choices and tells the app to show them.
fn save_and_announce(state: &Window) {
    state.settings.borrow().save();
    // SAFETY: posting to the app's main window.
    unsafe { PostMessageW(state.owner, WM_APP_SETTINGS_CHANGED, 0, 0) };
}

/// Runs what a control does when clicked or picked from the keyboard.
fn activate(control: Control) {
    let devices = with_window(|state| device_list(state)).unwrap_or_default();
    match control {
        Control::Nav(page) => {
            close_rename(true);
            with_window(|state| {
                state.page = page;
                state.scroll = 0;
            });
        }
        Control::PollLess | Control::PollMore => {
            with_window(|state| {
                {
                    let mut settings = state.settings.borrow_mut();
                    let minutes = settings.poll_minutes;
                    settings.poll_minutes = if control == Control::PollLess { minutes.saturating_sub(1).max(MIN_POLL_MINUTES) } else { (minutes + 1).min(MAX_POLL_MINUTES) };
                }
                save_and_announce(state);
            });
        }
        Control::PercentToggle => {
            with_window(|state| {
                {
                    let mut settings = state.settings.borrow_mut();
                    settings.shows_percent_in_icon = !settings.shows_percent_in_icon;
                }
                save_and_announce(state);
            });
        }
        Control::StartupToggle => {
            startup::set_enabled(!startup::is_enabled());
        }
        Control::Reset => {
            show_confirm(
                "Reset all settings?",
                "Renames, hidden devices, confirmed Bluetooth names and the check interval go back to how they were at install. Launch at login stays as it is.",
                || {
                    with_window(|state| {
                        *state.settings.borrow_mut() = Settings::default();
                        save_and_announce(state);
                    });
                    invalidate();
                },
            );
        }
        Control::Rename(index) => {
            if let Some(device) = devices.get(index) {
                open_rename(index, device);
            }
        }
        Control::ShowToggle(index) => {
            if let Some(device) = devices.get(index) {
                with_window(|state| {
                    {
                        let mut settings = state.settings.borrow_mut();
                        if let Some(position) = settings.hidden.iter().position(|id| id == &device.id) {
                            settings.hidden.remove(position);
                        } else {
                            settings.hidden.push(device.id.clone());
                        }
                    }
                    save_and_announce(state);
                });
            }
        }
        Control::ConfirmName(index) => {
            if let Some(device) = devices.get(index) {
                with_window(|state| {
                    state.settings.borrow_mut().confirmed_bluetooth_names.insert(device.id.clone(), device.name.clone());
                    save_and_announce(state);
                });
            }
        }
        Control::Link(url) => crate::app::open_url(url),
    }
    invalidate();
}

/// Opens a text box over a device's name for renaming. Enter or leaving the box saves; Esc cancels.
fn open_rename(index: usize, device: &Device) {
    close_rename(true);
    with_window(|state| {
        let dpi = state.dpi;
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: the window is alive while it is stored; the edit box is its child and dies with it.
        unsafe {
            GetClientRect(state.window, &mut client);
            let left = scale(CONTENT_LEFT + CARD_PADDING + 18, dpi);
            let top = scale(CARDS_TOP, dpi) + index as i32 * (scale(DEVICE_CARD_HEIGHT, dpi) + scale(CARD_GAP, dpi)) - state.scroll + scale(12, dpi);
            let width = scale(260, dpi);
            let height = scale(28, dpi);
            let class = to_wide("EDIT");
            let text = to_wide(&device.name);
            let edit = CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
                left,
                top,
                width,
                height,
                state.window,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            SendMessageW(edit, WM_SETFONT, state.fonts.body as usize, 1);
            SendMessageW(edit, EM_SETSEL, 0, -1);
            SetWindowSubclass(edit, Some(edit_proc), EDIT_SUBCLASS, 0);
            SetFocus(edit);
            state.edit = Some((edit, device.id.clone()));
        }
    });
    invalidate();
}

/// Closes the rename box, saving its text when `keep` is true. An empty name removes the rename.
fn close_rename(keep: bool) {
    let Some(Some((edit, id))) = with_window(|state| state.edit.take()) else { return };
    // SAFETY: the edit box is a live child of the settings window until destroyed here.
    let text = unsafe {
        let length = GetWindowTextLengthW(edit);
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(edit, buffer.as_mut_ptr(), buffer.len() as i32);
        RemoveWindowSubclass(edit, Some(edit_proc), EDIT_SUBCLASS);
        DestroyWindow(edit);
        String::from_utf16_lossy(&buffer[..copied as usize])
    };
    if keep {
        with_window(|state| {
            {
                let mut settings = state.settings.borrow_mut();
                let name = text.trim();
                if name.is_empty() {
                    settings.renames.remove(&id);
                } else {
                    settings.renames.insert(id.clone(), name.to_string());
                }
            }
            // The new name shows at once, without waiting for the read that follows.
            if let Some(device) = state.devices.borrow_mut().iter_mut().find(|device| device.id == id) {
                if !text.trim().is_empty() {
                    device.name = text.trim().to_string();
                    device.name_source = NameSource::UserRename;
                }
            }
            save_and_announce(state);
        });
    }
    invalidate();
}

unsafe extern "system" fn edit_proc(edit: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match message {
        WM_GETDLGCODE => DLGC_WANTALLKEYS as LRESULT,
        WM_KEYDOWN if wparam as u16 == VK_RETURN => {
            close_rename(true);
            0
        }
        WM_KEYDOWN if wparam as u16 == VK_ESCAPE => {
            close_rename(false);
            0
        }
        // Enter and Esc beep in a single-line box unless their characters are swallowed too.
        WM_CHAR if wparam == 13 || wparam == 27 => 0,
        WM_KILLFOCUS => {
            let result = DefSubclassProc(edit, message, wparam, lparam);
            close_rename(true);
            result
        }
        _ => DefSubclassProc(edit, message, wparam, lparam),
    }
}

fn hit_test(x: i32, y: i32) -> Option<Control> {
    with_window(|state| {
        let canvas = Canvas::new(1, 1);
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: the window is alive while it is stored.
        unsafe { GetClientRect(state.window, &mut client) };
        let sized = Canvas::new(client.right, 1);
        let _ = canvas;
        let controls = layout(state, &sized);
        let inside = |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        controls.into_iter().find(|(control, rect)| {
            let on_page_area = matches!(control, Control::Nav(_)) || y >= scale(CARDS_TOP - 4, state.dpi);
            on_page_area && inside(rect)
        }).map(|(control, _)| control)
    })
    .flatten()
}

fn move_focus(step: i32) {
    with_window(|state| {
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: the window is alive while it is stored.
        unsafe { GetClientRect(state.window, &mut client) };
        let order: Vec<Control> = layout(state, &Canvas::new(client.right, 1)).into_iter().map(|(control, _)| control).collect();
        let current = state.focused.and_then(|focused| order.iter().position(|control| *control == focused));
        let next = match current {
            None if step > 0 => 0,
            None => order.len() - 1,
            Some(position) => (position as i32 + step).rem_euclid(order.len() as i32) as usize,
        };
        state.focused = Some(order[next]);
    });
    invalidate();
}

/// Paints with a canvas sized to the client area; the confirm card needs it for hit testing too.
fn client_canvas(window: HWND) -> Canvas {
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: plain lookup on a live window.
    unsafe { GetClientRect(window, &mut client) };
    Canvas::new(client.right, client.bottom)
}

fn scroll_by(delta: i32) {
    with_window(|state| {
        if state.page != Page::Devices {
            return;
        }
        let count = state.devices.borrow().len() as i32;
        let dpi = state.dpi;
        let content = count * (scale(DEVICE_CARD_HEIGHT, dpi) + scale(CARD_GAP, dpi));
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: the window is alive while it is stored.
        unsafe { GetClientRect(state.window, &mut client) };
        let visible = client.bottom - scale(CARDS_TOP, dpi) - scale(OUTER, dpi);
        state.scroll = (state.scroll + delta).clamp(0, (content - visible).max(0));
    });
    close_rename(true);
    invalidate();
}

unsafe extern "system" fn settings_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let point_of = |lparam: LPARAM| ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32);
    match message {
        WM_PAINT => {
            let mut paint_info: PAINTSTRUCT = std::mem::zeroed();
            let target = BeginPaint(window, &mut paint_info);
            let mut canvas = client_canvas(window);
            with_window(|state| paint(state, &mut canvas));
            canvas.present(target);
            EndPaint(window, &paint_info);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let (x, y) = point_of(lparam);
            if is_confirm_open() {
                let canvas = client_canvas(window);
                let changed = with_window(|state| confirm_hover(&canvas, &SheetFonts { title: state.fonts.big, body: state.fonts.body }, state.dpi, x, y)).unwrap_or(false);
                if changed {
                    invalidate();
                }
                return 0;
            }
            let control = hit_test(x, y);
            let changed = with_window(|state| {
                let changed = state.hovered != control;
                state.hovered = control;
                changed
            });
            if changed == Some(true) {
                invalidate();
            }
            0
        }
        WM_LBUTTONDOWN => {
            with_window(|state| state.focused = None);
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = point_of(lparam);
            if is_confirm_open() {
                let canvas = client_canvas(window);
                if let Some((dpi, title, body)) = with_window(|state| (state.dpi, state.fonts.big, state.fonts.body)) {
                    confirm_click(&canvas, &SheetFonts { title, body }, dpi, x, y);
                }
                invalidate();
                return 0;
            }
            if let Some(control) = hit_test(x, y) {
                activate(control);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as i16 as i32;
            let dpi = with_window(|state| state.dpi).unwrap_or(96);
            scroll_by(-delta.signum() * scale(SCROLL_STEP, dpi));
            0
        }
        WM_KEYDOWN => {
            let key = wparam as u16;
            if is_confirm_open() {
                confirm_key(key);
                invalidate();
                return 0;
            }
            let shift = GetKeyState(i32::from(VK_SHIFT)) < 0;
            match key {
                VK_TAB => move_focus(if shift { -1 } else { 1 }),
                VK_DOWN => move_focus(1),
                VK_UP => move_focus(-1),
                VK_LEFT | VK_RIGHT => {
                    if let Some(Some(focused @ (Control::PollLess | Control::PollMore))) = with_window(|state| state.focused) {
                        let _ = focused;
                        activate(if key == VK_LEFT { Control::PollLess } else { Control::PollMore });
                    }
                }
                VK_RETURN | VK_SPACE => {
                    if let Some(Some(control)) = with_window(|state| state.focused) {
                        activate(control);
                    }
                }
                VK_ESCAPE => {
                    DestroyWindow(window);
                }
                _ => return DefWindowProcW(window, message, wparam, lparam),
            }
            0
        }
        WM_CTLCOLOREDIT => {
            let device_context = wparam as HDC;
            let colors = with_window(|state| (state.theme, state.edit_brush));
            if let Some((theme, brush)) = colors {
                let swap = |color: u32| ((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF);
                SetTextColor(device_context, swap(theme.text));
                SetBkColor(device_context, swap(theme.card));
                return brush as LRESULT;
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        WM_DPICHANGED => {
            let dpi = (wparam & 0xFFFF) as u32;
            close_rename(true);
            with_window(|state| {
                state.dpi = dpi;
                state.fonts = Fonts::new(dpi);
            });
            let suggested = &*(lparam as *const RECT);
            SetWindowPos(window, std::ptr::null_mut(), suggested.left, suggested.top, suggested.right - suggested.left, suggested.bottom - suggested.top, SWP_NOZORDER | SWP_NOACTIVATE);
            invalidate();
            0
        }
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            dismiss_confirm();
            if let Some(state) = WINDOW.with(|cell| cell.borrow_mut().take()) {
                DeleteObject(state.edit_brush);
            }
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

fn register_class() {
    let class_name = to_wide(CLASS_NAME);
    // SAFETY: the class name outlives the call; registering twice only fails harmlessly.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(settings_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: module,
            // The icon built into the program; a small number in place of a name picks a numbered resource.
            hIcon: LoadIconW(module, std::ptr::without_provenance(1)),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class);
    }
}

fn listen_for_theme() {
    let already = LISTENING.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), true));
    if already {
        return;
    }
    on_theme_change(|| {
        with_window(|state| {
            state.theme = current_theme();
            apply_window_look(state.window, &state.theme, Backdrop::Window);
            // SAFETY: the brush was made for the old theme and is replaced once here.
            unsafe {
                DeleteObject(state.edit_brush);
                state.edit_brush = CreateSolidBrush(swap_rgb(state.theme.card));
            }
        });
        invalidate();
    });
}

fn swap_rgb(color: u32) -> u32 {
    ((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF)
}

/// Opens the settings window on General, or brings it to the front if it is open.
pub fn open_settings(owner: HWND, settings: Rc<RefCell<Settings>>, devices: Rc<RefCell<Vec<Device>>>) {
    open_settings_on(owner, settings, devices, Page::General);
}

/// Opens the settings window on a page, or brings it to the front on that page.
pub fn open_settings_on(owner: HWND, settings: Rc<RefCell<Settings>>, devices: Rc<RefCell<Vec<Device>>>, page: Page) {
    let existing = with_window(|state| {
        state.page = page;
        state.window
    });
    if let Some(window) = existing {
        // SAFETY: the window is alive while it is stored.
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
        invalidate();
        return;
    }
    register_class();
    listen_for_theme();
    let class_name = to_wide(CLASS_NAME);
    let title = to_wide("Juice Bar settings");
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    // SAFETY: Win32 window creation on the UI thread; every pointer passed outlives its call.
    unsafe {
        let dpi = GetDpiForSystem().max(96);
        let mut frame = RECT { left: 0, top: 0, right: scale(WIDTH, dpi), bottom: scale(HEIGHT, dpi) };
        AdjustWindowRectExForDpi(&mut frame, style, 0, WS_EX_APPWINDOW, dpi);
        let window = CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            frame.right - frame.left,
            frame.bottom - frame.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        let dpi = GetDpiForWindow(window).max(96);
        let theme = current_theme();
        apply_window_look(window, &theme, Backdrop::Window);
        WINDOW.with(|cell| {
            *cell.borrow_mut() = Some(Window {
                window,
                owner,
                settings,
                devices,
                page,
                dpi,
                fonts: Fonts::new(dpi),
                theme,
                hovered: None,
                focused: None,
                scroll: 0,
                edit: None,
                edit_brush: CreateSolidBrush(swap_rgb(theme.card)),
            });
        });
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
    }
}

/// Opens the Reset confirm card, for screenshots and tests of the card.
pub fn show_reset_confirm() {
    activate(Control::Reset);
}

/// Redraws the open window after a read, so the Devices page shows fresh readings.
pub fn refresh_settings() {
    let is_renaming = with_window(|state| state.edit.is_some()).unwrap_or(false);
    if !is_renaming {
        invalidate();
    }
}

/// Closes the settings window if it is open.
pub fn close_settings() {
    if let Some(window) = with_window(|state| state.window) {
        // SAFETY: the window is alive while it is stored; WM_DESTROY clears the state.
        unsafe {
            if IsWindow(window) != 0 {
                DestroyWindow(window);
            }
        }
    }
}
