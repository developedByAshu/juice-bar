//! The settings window, laid out exactly like NetSpeedGauge's: a tinted sidebar with the app name,
//! the page list and the "Made in India" line, and the chosen page on the right in rounded rows
//! under section headings. Pages: General, Devices, About.

use crate::canvas::{create_font, mix, Align, Canvas};
use crate::glyph::glass_image;
use crate::menu_ui::{connection_label, fit_text};
use crate::messages::WM_APP_SETTINGS_CHANGED;
use crate::startup;
use crate::theme::{apply_window_look, current_theme, draw_made_with_love, on_theme_change, tone_color, Backdrop, Theme, GREEN, ICON_FONT, UI_FONT, UI_FONT_STRONG};
use crate::util::{scale, to_wide};
use juicebar_shared::device::{Device, NameSource};
use juicebar_shared::settings::{Settings, MENU_FONT_POINTS_MAX, MENU_FONT_POINTS_MIN};
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

// Every size below is NetSpeedGauge's (windows/src/settings_ui.rs), at 100% scaling, so the two windows match.
const CLASS_NAME: &str = "JuiceBarSettings";
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
const PRIVACY_LINE: i32 = 26;
const PRIVACY_HEADER_HEIGHT: i32 = 68;
const LOCK_TILE: i32 = 40;
const CARD_RADIUS: i32 = 6;
const NAV_TOP: i32 = 62;
const NAV_ROW: i32 = 40;
const SCROLL_STEP: i32 = 72;
const CONTROL_HEIGHT: i32 = 30;
const STEPPER_VALUE_WIDTH: i32 = 76;
const TOGGLE_WIDTH: i32 = 40;
const TOGGLE_HEIGHT: i32 = 20;
const DISABLED_FILL: f32 = 0.35;
const DISABLED_TEXT: f32 = 0.5;
// Devices rows start their name after the battery dot.
const DEVICE_TEXT_LEFT: i32 = 30;
const EDIT_SUBCLASS: usize = 1;
const REPO_URL: &str = "https://github.com/developedByAshu/juice-bar";
const LICENSE_URL: &str = "https://github.com/developedByAshu/juice-bar/blob/main/LICENSE";
const ICON_GENERAL: &str = "\u{E713}";
const ICON_DEVICES: &str = "\u{E772}";
const ICON_ABOUT: &str = "\u{E946}";
const ICON_MINUS: &str = "\u{E738}";
const ICON_PLUS: &str = "\u{E710}";
const ICON_CHECK: &str = "\u{E73E}";
const ICON_LOCK: &str = "\u{E72E}";
const MIN_POLL_MINUTES: u32 = 1;
const MAX_POLL_MINUTES: u32 = 60;
const APP_NAME: &str = "Juice Bar";
const APP_TAGLINE: &str = "Battery % of wireless mice and keyboards, in the tray.";
const DEVELOPER_NAME: &str = "Ashutosh Anand";
const DEVELOPER_ORIGIN: &str = "From India";
const RESET_LABEL: &str = "Reset all";
const RESET_CONFIRM_LABEL: &str = "Click to confirm";

/// What Juice Bar does and does not do with data, shown on the About page.
const PRIVACY_LINES: [&str; 4] = [
    "Juice Bar never connects to the internet on its own.",
    "It only reads battery levels from your wireless devices.",
    "Your settings and renames are stored only on this PC.",
    "Report sends only device ids, and only when you choose.",
];

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
    MenuTextLess,
    MenuTextMore,
    PercentToggle,
    StartupToggle,
    Reset,
    Rename(usize),
    ShowToggle(usize),
    ConfirmName(usize),
    Link(&'static str),
}

/// One row of the General page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    Poll,
    MenuText,
    Percent,
    Startup,
    Reset,
}

/// One piece of a page, top to bottom, as NetSpeedGauge builds its pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Section(&'static str),
    Note(&'static str),
    Setting(Setting),
    Device(usize),
    NoDevices,
    AboutCard,
    Privacy,
    Developer,
    Link { label: &'static str, hint: &'static str, url: &'static str },
}

struct Fonts {
    body: HFONT,
    strong: HFONT,
    small: HFONT,
    title: HFONT,
    icon: HFONT,
    small_icon: HFONT,
    heart: HFONT,
    lock: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        Self {
            body: create_font(UI_FONT, 10.0, 400, dpi),
            strong: create_font(UI_FONT_STRONG, 10.0, 400, dpi),
            small: create_font(UI_FONT, 9.0, 400, dpi),
            title: create_font(UI_FONT_STRONG, 16.0, 400, dpi),
            icon: create_font(ICON_FONT, 11.0, 400, dpi),
            small_icon: create_font(ICON_FONT, 8.0, 400, dpi),
            heart: create_font(ICON_FONT, 9.0, 400, dpi),
            lock: create_font(ICON_FONT, 15.0, 400, dpi),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: each font was created in new() and is released once here.
        unsafe {
            for font in [self.body, self.strong, self.small, self.title, self.icon, self.small_icon, self.heart, self.lock] {
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
    /// Reset all was clicked once; the next click on it resets, any other click cancels.
    is_confirming_reset: bool,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static LISTENING: RefCell<bool> = const { RefCell::new(false) };
}

/// Runs an action on the open window. A nested call while the window is busy is skipped, never a crash.
fn with_window<T>(action: impl FnOnce(&mut Window) -> T) -> Option<T> {
    WINDOW.with(|cell| cell.try_borrow_mut().ok().and_then(|mut state| state.as_mut().map(action)))
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

/// Returns the pieces of the current page, top to bottom.
fn page_blocks(state: &Window) -> Vec<Block> {
    match state.page {
        Page::General => vec![
            Block::Section("Battery checks"),
            Block::Setting(Setting::Poll),
            Block::Section("Tray icon"),
            Block::Setting(Setting::Percent),
            Block::Setting(Setting::MenuText),
            Block::Section("Startup"),
            Block::Setting(Setting::Startup),
            Block::Section("Reset"),
            Block::Setting(Setting::Reset),
        ],
        Page::Devices => {
            let count = state.devices.borrow().len();
            let mut blocks = vec![Block::Section("Your devices"), Block::Note("Switch a device off to hide it from the menu.\nRename gives it a name you will recognise.")];
            if count == 0 {
                blocks.push(Block::NoDevices);
            }
            blocks.extend((0..count).map(Block::Device));
            blocks
        }
        Page::About => vec![
            Block::AboutCard,
            Block::Section("Privacy"),
            Block::Privacy,
            Block::Section("Know your developer"),
            Block::Developer,
            Block::Section("Links"),
            Block::Link { label: "Source code", hint: "github.com/developedByAshu/juice-bar", url: REPO_URL },
            Block::Link { label: "Licence", hint: "GPL-3.0, free and open source", url: LICENSE_URL },
        ],
    }
}

/// Returns how much height a piece takes, the gap below it included.
fn block_height(block: Block, dpi: u32) -> i32 {
    match block {
        Block::Section(_) => scale(SECTION_GAP + 8, dpi),
        Block::Note(_) => scale(NOTE_HEIGHT, dpi),
        Block::AboutCard => scale(ABOUT_CARD_HEIGHT + ROW_GAP, dpi),
        Block::Privacy => scale(PRIVACY_HEADER_HEIGHT, dpi) + scale(PRIVACY_LINE, dpi) * PRIVACY_LINES.len() as i32 + scale(20, dpi) + scale(ROW_GAP, dpi),
        _ => scale(ROW_HEIGHT + ROW_GAP, dpi),
    }
}

/// Returns each piece of the current page with its top, scrolled.
fn placed_blocks(state: &Window) -> Vec<(Block, i32)> {
    let mut y = scale(HEADER_HEIGHT, state.dpi) - state.scroll;
    page_blocks(state)
        .into_iter()
        .map(|block| {
            let top = y;
            y += block_height(block, state.dpi);
            (block, top)
        })
        .collect()
}

/// Returns the height of the whole current page, from the header down.
fn content_height(state: &Window) -> i32 {
    page_blocks(state).into_iter().map(|block| block_height(block, state.dpi)).sum::<i32>() + scale(MARGIN, state.dpi)
}

/// Returns the left edge and width of the rows on the right, as NetSpeedGauge places them.
fn content_columns(dpi: u32) -> (i32, i32) {
    (scale(SIDEBAR_WIDTH + MARGIN - 4, dpi), scale(WIDTH - SIDEBAR_WIDTH - MARGIN * 2 + 4, dpi))
}

/// Returns a plain button of NetSpeedGauge's size whose right edge is `right`, in a row at `top`.
fn button_rect(canvas: &Canvas, font: HFONT, label: &str, right: i32, top: i32, dpi: u32) -> RECT {
    let width = canvas.text_width(label, font) + scale(28, dpi);
    let button_top = top + scale(10, dpi);
    RECT { left: right - width, top: button_top, right, bottom: button_top + scale(CONTROL_HEIGHT, dpi) }
}

/// Returns the switch's rectangle in a row whose right padding ends at `right`.
fn toggle_rect(right: i32, top: i32, dpi: u32) -> RECT {
    let (width, height) = (scale(TOGGLE_WIDTH, dpi), scale(TOGGLE_HEIGHT, dpi));
    let toggle_top = top + (scale(ROW_HEIGHT, dpi) - height) / 2;
    RECT { left: right - width, top: toggle_top, right, bottom: toggle_top + height }
}

/// Returns each control and its rectangle on the current page, scrolled.
fn layout(state: &Window, canvas: &Canvas) -> Vec<(Control, RECT)> {
    let dpi = state.dpi;
    let mut controls = Vec::new();
    let sidebar = scale(SIDEBAR_WIDTH, dpi);
    for (index, (page, _, _)) in PAGES.iter().enumerate() {
        let top = scale(NAV_TOP + index as i32 * NAV_ROW, dpi);
        controls.push((Control::Nav(*page), RECT { left: scale(8, dpi), top, right: sidebar - scale(8, dpi), bottom: top + scale(NAV_ROW - 4, dpi) }));
    }
    let (left, width) = content_columns(dpi);
    let right = left + width - scale(14, dpi);
    let row = |top: i32| RECT { left, top, right: left + width, bottom: top + scale(ROW_HEIGHT, dpi) };
    let settings = state.settings.borrow();
    let devices = device_list(state);
    for (block, top) in placed_blocks(state) {
        match block {
            Block::Setting(setting @ (Setting::Poll | Setting::MenuText)) => {
                let (less, more) = if setting == Setting::Poll { (Control::PollLess, Control::PollMore) } else { (Control::MenuTextLess, Control::MenuTextMore) };
                let size = scale(CONTROL_HEIGHT, dpi);
                let button_top = top + scale(10, dpi);
                let plus_x = right - size;
                let minus_x = plus_x - scale(STEPPER_VALUE_WIDTH, dpi) - size;
                controls.push((less, RECT { left: minus_x, top: button_top, right: minus_x + size, bottom: button_top + size }));
                controls.push((more, RECT { left: plus_x, top: button_top, right: plus_x + size, bottom: button_top + size }));
            }
            // As in NetSpeedGauge, a switch row reacts anywhere on the row.
            Block::Setting(Setting::Percent) => controls.push((Control::PercentToggle, row(top))),
            Block::Setting(Setting::Startup) => controls.push((Control::StartupToggle, row(top))),
            Block::Setting(Setting::Reset) => {
                let label = if state.is_confirming_reset { RESET_CONFIRM_LABEL } else { RESET_LABEL };
                controls.push((Control::Reset, button_rect(canvas, state.fonts.body, label, right, top, dpi)));
            }
            Block::Device(index) => {
                let toggle = toggle_rect(right, top, dpi);
                controls.push((Control::ShowToggle(index), toggle));
                let rename = button_rect(canvas, state.fonts.body, "Rename", toggle.left - scale(14, dpi), top, dpi);
                controls.push((Control::Rename(index), rename));
                if devices.get(index).is_some_and(|device| needs_name_confirm(device, &settings)) {
                    controls.push((Control::ConfirmName(index), button_rect(canvas, state.fonts.body, "Keep this name", rename.left - scale(8, dpi), top, dpi)));
                }
            }
            Block::Link { url, .. } => controls.push((Control::Link(url), button_rect(canvas, state.fonts.body, "Open", right, top, dpi))),
            _ => {}
        }
    }
    controls
}

/// Checks whether a device's name came from Bluetooth memory and still waits for the user's yes.
fn needs_name_confirm(device: &Device, settings: &Settings) -> bool {
    device.name_source == NameSource::BluetoothMemory && !settings.confirmed_bluetooth_names.contains_key(&device.id)
}

/// Paints the window: the page first, then the fixed header and sidebar over anything scrolled under them.
fn paint(state: &Window, canvas: &mut Canvas) {
    let theme = state.theme;
    let dpi = state.dpi;
    canvas.clear(theme.background, 1.0);
    let controls = layout(state, canvas);
    draw_page(state, canvas, &controls);
    let sidebar = scale(SIDEBAR_WIDTH, dpi);
    canvas.round_rect(sidebar, 0, canvas.width() - sidebar, scale(HEADER_HEIGHT, dpi), 0.0, theme.background, 1.0);
    let title = PAGES.iter().find(|(page, _, _)| *page == state.page).map_or("", |(_, _, title)| title);
    canvas.text(sidebar + scale(MARGIN - 4, dpi), scale(20, dpi), title, state.fonts.title, theme.text, Align::Left);
    draw_sidebar(state, canvas, &controls);
    draw_scrollbar(state, canvas);
}

fn draw_sidebar(state: &Window, canvas: &mut Canvas, controls: &[(Control, RECT)]) {
    let theme = state.theme;
    let dpi = state.dpi;
    let fonts = &state.fonts;
    let sidebar = scale(SIDEBAR_WIDTH, dpi);
    canvas.round_rect(0, 0, sidebar, canvas.height(), 0.0, mix(theme.background, theme.card, 0.45), 1.0);
    canvas.text(scale(18, dpi), scale(22, dpi), APP_NAME, fonts.strong, theme.muted, Align::Left);
    for (page, icon, label) in PAGES {
        let Some(rect) = controls.iter().find(|(control, _)| *control == Control::Nav(page)).map(|(_, rect)| *rect) else { continue };
        let (x, y, width, height) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
        let is_selected = state.page == page;
        if is_selected || state.hovered == Some(Control::Nav(page)) {
            let fill = if is_selected { theme.selected } else { mix(theme.background, theme.selected, 0.5) };
            canvas.round_rect(x, y, width, height, scale(CARD_RADIUS, dpi) as f32, fill, 1.0);
        }
        if is_selected {
            canvas.round_rect(x, y + scale(10, dpi), scale(3, dpi), height - scale(20, dpi), scale(2, dpi) as f32, theme.highlight, 1.0);
        }
        if state.focused == Some(Control::Nav(page)) {
            draw_focus(canvas, &theme, &rect, dpi);
        }
        let color = if is_selected { theme.text } else { theme.muted };
        canvas.text(x + scale(12, dpi), y + scale(10, dpi), icon, fonts.icon, color, Align::Left);
        let font = if is_selected { fonts.strong } else { fonts.body };
        canvas.text(x + scale(38, dpi), y + scale(9, dpi), label, font, theme.text, Align::Left);
    }
    draw_made_with_love(canvas, sidebar / 2, canvas.height() - scale(34, dpi), fonts.small, fonts.heart, theme.muted);
}

fn draw_scrollbar(state: &Window, canvas: &mut Canvas) {
    let dpi = state.dpi;
    let view = canvas.height() - scale(HEADER_HEIGHT, dpi);
    let content = content_height(state);
    if content <= view {
        return;
    }
    let track_x = canvas.width() - scale(7, dpi);
    let thumb_height = (view * view / content).max(scale(30, dpi));
    let thumb_y = scale(HEADER_HEIGHT, dpi) + (view - thumb_height) * state.scroll / (content - view).max(1);
    let color = mix(state.theme.background, state.theme.muted, 0.5);
    canvas.round_rect(track_x, thumb_y, scale(3, dpi), thumb_height, scale(2, dpi) as f32, color, 1.0);
}

fn row_card(canvas: &mut Canvas, theme: &Theme, left: i32, top: i32, width: i32, height: i32, dpi: u32) {
    canvas.round_rect(left, top, width, height, scale(CARD_RADIUS, dpi) as f32, theme.card, 1.0);
}

/// Draws a row's label, with its hint under it when there is one, cut to fit before the controls.
fn row_label(canvas: &mut Canvas, state: &Window, x: i32, top: i32, label: &str, hint: &str, max_width: i32) {
    let (theme, dpi, fonts) = (state.theme, state.dpi, &state.fonts);
    if hint.is_empty() {
        canvas.text(x, top + scale(15, dpi), &fit_text(canvas, label, fonts.body, max_width), fonts.body, theme.text, Align::Left);
    } else {
        canvas.text(x, top + scale(7, dpi), &fit_text(canvas, label, fonts.body, max_width), fonts.body, theme.text, Align::Left);
        canvas.text(x, top + scale(27, dpi), &fit_text(canvas, hint, fonts.small, max_width), fonts.small, theme.muted, Align::Left);
    }
}

/// Draws a plain or accent button the way NetSpeedGauge does.
fn draw_button(canvas: &mut Canvas, state: &Window, rect: &RECT, label: &str, control: Control, is_accent: bool) {
    let (theme, dpi) = (state.theme, state.dpi);
    let is_hovered = state.hovered == Some(control);
    let (fill, text) = if is_accent {
        (if is_hovered { mix(theme.highlight, theme.text, 0.12) } else { theme.highlight }, theme.on_accent)
    } else {
        (if is_hovered { theme.button_hover } else { theme.button }, theme.text)
    };
    canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, scale(CARD_RADIUS, dpi) as f32, fill, 1.0);
    canvas.text((rect.left + rect.right) / 2, rect.top + scale(5, dpi), label, state.fonts.body, text, Align::Center);
    if state.focused == Some(control) {
        draw_focus(canvas, &theme, rect, dpi);
    }
}

/// Draws NetSpeedGauge's switch: filled in the accent when on, an outline when off.
fn draw_toggle(canvas: &mut Canvas, theme: &Theme, rect: &RECT, is_on: bool, dpi: u32) {
    let (x, top, width, height) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
    if is_on {
        canvas.round_rect(x, top, width, height, height as f32 / 2.0, theme.highlight, 1.0);
    } else {
        canvas.round_rect(x, top, width, height, height as f32 / 2.0, theme.muted, 1.0);
        canvas.round_rect(x + 1, top + 1, width - 2, height - 2, (height - 2) as f32 / 2.0, theme.card, 1.0);
    }
    let knob = scale(12, dpi);
    let knob_x = if is_on { x + width - knob - scale(4, dpi) } else { x + scale(4, dpi) };
    let knob_color = if is_on { if theme.is_dark { 0x1B1B1B } else { 0xFFFFFF } } else { theme.muted };
    canvas.round_rect(knob_x, top + (height - knob) / 2, knob, knob, knob as f32 / 2.0, knob_color, 1.0);
}

fn draw_focus(canvas: &mut Canvas, theme: &Theme, rect: &RECT, dpi: u32) {
    let ring = scale(2, dpi);
    canvas.round_outline(rect.left - ring - 1, rect.top - ring - 1, rect.right - rect.left + 2 * ring + 2, rect.bottom - rect.top + 2 * ring + 2, scale(CARD_RADIUS + 2, dpi) as f32, ring as f32, theme.text, 1.0);
}

/// Draws NetSpeedGauge's minus and plus buttons with the value between them.
fn draw_stepper(canvas: &mut Canvas, state: &Window, controls: (Control, Control), less: &RECT, more: &RECT, value: &str, can_less: bool, can_more: bool) {
    let (theme, dpi) = (state.theme, state.dpi);
    for (rect, icon, control, is_enabled) in [(less, ICON_MINUS, controls.0, can_less), (more, ICON_PLUS, controls.1, can_more)] {
        let (size, radius) = (rect.right - rect.left, scale(CARD_RADIUS, dpi) as f32);
        let center = rect.left + size / 2;
        if is_enabled {
            let fill = if state.hovered == Some(control) { theme.button_hover } else { theme.button };
            canvas.round_rect(rect.left, rect.top, size, size, radius, fill, 1.0);
            canvas.text(center, rect.top + scale(9, dpi), icon, state.fonts.small_icon, theme.text, Align::Center);
        } else {
            // At the end of its range the button looks switched off, as in NetSpeedGauge.
            canvas.round_rect(rect.left, rect.top, size, size, radius, mix(theme.card, theme.button, DISABLED_FILL), 1.0);
            canvas.text(center, rect.top + scale(9, dpi), icon, state.fonts.small_icon, mix(theme.card, theme.muted, DISABLED_TEXT), Align::Center);
        }
        if state.focused == Some(control) {
            draw_focus(canvas, &theme, rect, dpi);
        }
    }
    canvas.text((less.right + more.left) / 2, less.top + scale(5, dpi), value, state.fonts.body, theme.text, Align::Center);
}

/// Draws every piece of the current page, top to bottom.
fn draw_page(state: &Window, canvas: &mut Canvas, controls: &[(Control, RECT)]) {
    let theme = state.theme;
    let dpi = state.dpi;
    let fonts = &state.fonts;
    let rect_of = |wanted: Control| controls.iter().find(|(control, _)| *control == wanted).map(|(_, rect)| *rect);
    let (left, width) = content_columns(dpi);
    let right = left + width - scale(14, dpi);
    let row_height = scale(ROW_HEIGHT, dpi);
    let text_x = left + scale(14, dpi);
    let settings = state.settings.borrow().clone();
    let devices = device_list(state);
    for (block, top) in placed_blocks(state) {
        if top > canvas.height() || top + block_height(block, dpi) < scale(HEADER_HEIGHT, dpi) - scale(ROW_HEIGHT, dpi) {
            continue;
        }
        match block {
            Block::Section(title) => canvas.text(left + scale(2, dpi), top + scale(14, dpi), title, fonts.strong, theme.text, Align::Left),
            Block::Note(text) => {
                let (first, second) = text.split_once('\n').unwrap_or((text, ""));
                canvas.text(left + scale(2, dpi), top + scale(2, dpi), first, fonts.small, theme.muted, Align::Left);
                canvas.text(left + scale(2, dpi), top + scale(19, dpi), second, fonts.small, theme.muted, Align::Left);
            }
            Block::Setting(setting) => {
                row_card(canvas, &theme, left, top, width, row_height, dpi);
                // The label and hint run up to the row's control, with NetSpeedGauge's 14 px gap.
                let control_left = match setting {
                    Setting::Poll => rect_of(Control::PollLess).map_or(right, |rect| rect.left),
                    Setting::MenuText => rect_of(Control::MenuTextLess).map_or(right, |rect| rect.left),
                    Setting::Reset => rect_of(Control::Reset).map_or(right, |rect| rect.left),
                    Setting::Percent | Setting::Startup => toggle_rect(right, top, dpi).left,
                };
                let label_room = control_left - scale(14, dpi) - text_x;
                match setting {
                    Setting::Poll => {
                        row_label(canvas, state, text_x, top, "Check batteries every", "Juice Bar sleeps in between, so it costs no power", label_room);
                        if let (Some(less), Some(more)) = (rect_of(Control::PollLess), rect_of(Control::PollMore)) {
                            let minutes = settings.poll_minutes;
                            draw_stepper(canvas, state, (Control::PollLess, Control::PollMore), &less, &more, &format!("{minutes} min"), minutes > MIN_POLL_MINUTES, minutes < MAX_POLL_MINUTES);
                        }
                    }
                    Setting::MenuText => {
                        row_label(canvas, state, text_x, top, "Menu text size", "The tray menu grows and shrinks with it", label_room);
                        if let (Some(less), Some(more)) = (rect_of(Control::MenuTextLess), rect_of(Control::MenuTextMore)) {
                            let points = settings.menu_font_points;
                            draw_stepper(canvas, state, (Control::MenuTextLess, Control::MenuTextMore), &less, &more, &format!("{points} pt"), points > MENU_FONT_POINTS_MIN, points < MENU_FONT_POINTS_MAX);
                        }
                    }
                    Setting::Percent | Setting::Startup => {
                        let (label, hint, is_on, control) = if setting == Setting::Percent {
                            ("Show the lowest % in the icon", "Shows the number instead of the juice glass", settings.shows_percent_in_icon, Control::PercentToggle)
                        } else {
                            ("Start with Windows", "Opens Juice Bar when you sign in", startup::is_enabled(), Control::StartupToggle)
                        };
                        row_label(canvas, state, text_x, top, label, hint, label_room);
                        let toggle = toggle_rect(right, top, dpi);
                        draw_toggle(canvas, &theme, &toggle, is_on, dpi);
                        if state.focused == Some(control) {
                            draw_focus(canvas, &theme, &toggle, dpi);
                        }
                    }
                    Setting::Reset => {
                        row_label(canvas, state, text_x, top, "Every setting", "Renames, hidden devices and the check interval", label_room);
                        if let Some(rect) = rect_of(Control::Reset) {
                            let label = if state.is_confirming_reset { RESET_CONFIRM_LABEL } else { RESET_LABEL };
                            draw_button(canvas, state, &rect, label, Control::Reset, false);
                        }
                    }
                }
            }
            Block::NoDevices => {
                row_card(canvas, &theme, left, top, width, row_height, dpi);
                row_label(canvas, state, text_x, top, "No wireless mouse or keyboard found yet", "Plug in a dongle and it shows up here", width - scale(28, dpi));
            }
            Block::Device(index) => {
                let Some(device) = devices.get(index) else { continue };
                row_card(canvas, &theme, left, top, width, row_height, dpi);
                let reading = match device.battery {
                    Some(battery) => format!("{}%{}", battery.percent, if battery.is_charging { " charging" } else { "" }),
                    None if device.is_unrecognised() => "Not recognised yet".to_string(),
                    None => "Not connected".to_string(),
                };
                let mut hint = format!("{}  \u{00B7}  {reading}", connection_label(device.connection));
                if needs_name_confirm(device, &settings) {
                    hint.push_str("  \u{00B7}  name from Bluetooth");
                }
                let dot_x = text_x + scale(4, dpi);
                let middle = top + row_height / 2;
                match device.battery {
                    Some(battery) => canvas.circle(dot_x, middle, scale(4, dpi), tone_color(tone_for_percent(battery.percent), &theme)),
                    None => canvas.round_outline(dot_x - scale(4, dpi), middle - scale(4, dpi), scale(8, dpi), scale(8, dpi), scale(4, dpi) as f32, 1.5, theme.muted, 1.0),
                }
                let name_x = left + scale(DEVICE_TEXT_LEFT, dpi);
                let first_control = rect_of(Control::ConfirmName(index)).or(rect_of(Control::Rename(index))).map_or(right, |rect| rect.left);
                let is_renaming = state.edit.as_ref().is_some_and(|(_, id)| id == &device.id);
                let label = if is_renaming { "" } else { device.name.as_str() };
                row_label(canvas, state, name_x, top, label, &hint, first_control - scale(12, dpi) - name_x);
                if let Some(rect) = rect_of(Control::Rename(index)) {
                    draw_button(canvas, state, &rect, "Rename", Control::Rename(index), false);
                }
                if let Some(rect) = rect_of(Control::ConfirmName(index)) {
                    draw_button(canvas, state, &rect, "Keep this name", Control::ConfirmName(index), true);
                }
                if let Some(rect) = rect_of(Control::ShowToggle(index)) {
                    draw_toggle(canvas, &theme, &rect, !settings.hidden.contains(&device.id), dpi);
                    if state.focused == Some(Control::ShowToggle(index)) {
                        draw_focus(canvas, &theme, &rect, dpi);
                    }
                }
            }
            Block::AboutCard => {
                let height = scale(ABOUT_CARD_HEIGHT, dpi);
                row_card(canvas, &theme, left, top, width, height, dpi);
                let icon_size = scale(56, dpi);
                let x = left + scale(16, dpi);
                canvas.text(x, top + scale(14, dpi), APP_NAME, fonts.title, theme.text, Align::Left);
                canvas.text(x, top + scale(46, dpi), APP_TAGLINE, fonts.body, theme.muted, Align::Left);
                canvas.text(x, top + scale(68, dpi), &format!("Version {}", juicebar_shared::APP_VERSION), fonts.small, theme.muted, Align::Left);
                let icon_x = left + width - scale(16, dpi) - icon_size;
                canvas.draw_pixels(icon_x, top + (height - icon_size) / 2, icon_size, &glass_image(icon_size as u32, theme.text, GREEN, Some(75)));
            }
            Block::Privacy => draw_privacy(canvas, state, left, top, width),
            Block::Developer => {
                row_card(canvas, &theme, left, top, width, row_height, dpi);
                row_label(canvas, state, text_x, top, DEVELOPER_NAME, DEVELOPER_ORIGIN, width - scale(28, dpi));
            }
            Block::Link { label, hint, url } => {
                row_card(canvas, &theme, left, top, width, row_height, dpi);
                row_label(canvas, state, text_x, top, label, hint, width - scale(120, dpi));
                if let Some(rect) = rect_of(Control::Link(url)) {
                    draw_button(canvas, state, &rect, "Open", Control::Link(url), false);
                }
            }
        }
    }
}

/// Draws the privacy card: a lock on a tinted tile, its promise, a dividing line and the four promises.
fn draw_privacy(canvas: &mut Canvas, state: &Window, left: i32, top: i32, width: i32) {
    let (theme, dpi, fonts) = (state.theme, state.dpi, &state.fonts);
    let header = scale(PRIVACY_HEADER_HEIGHT, dpi);
    let line_height = scale(PRIVACY_LINE, dpi);
    let height = header + line_height * PRIVACY_LINES.len() as i32 + scale(20, dpi);
    row_card(canvas, &theme, left, top, width, height, dpi);
    let tile = scale(LOCK_TILE, dpi);
    let (tile_x, tile_y) = (left + scale(14, dpi), top + scale(14, dpi));
    canvas.round_rect(tile_x, tile_y, tile, tile, scale(CARD_RADIUS + 2, dpi) as f32, mix(theme.card, theme.highlight, 0.18), 1.0);
    let lock_y = tile_y + (tile - canvas.text_height(fonts.lock)) / 2;
    canvas.text(tile_x + tile / 2, lock_y, ICON_LOCK, fonts.lock, theme.highlight, Align::Center);
    let text_x = tile_x + tile + scale(12, dpi);
    canvas.text(text_x, tile_y + scale(1, dpi), "Your data stays with you", fonts.strong, theme.text, Align::Left);
    canvas.text(text_x, tile_y + scale(21, dpi), "Private and stored only on this PC", fonts.small, theme.muted, Align::Left);
    canvas.round_rect(left + scale(14, dpi), top + header, width - scale(28, dpi), 1, 0.0, theme.border, 1.0);
    for (position, line) in PRIVACY_LINES.iter().enumerate() {
        let line_y = top + header + scale(10, dpi) + position as i32 * line_height;
        canvas.text(left + scale(14, dpi), line_y + scale(5, dpi), ICON_CHECK, fonts.small_icon, theme.highlight, Align::Left);
        canvas.text(left + scale(36, dpi), line_y, line, fonts.body, theme.text, Align::Left);
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
    // Any other click cancels a Reset all that waits for its second click, as in NetSpeedGauge.
    let was_confirming = with_window(|state| std::mem::replace(&mut state.is_confirming_reset, false)).unwrap_or(false);
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
        Control::MenuTextLess | Control::MenuTextMore => {
            with_window(|state| {
                {
                    let mut settings = state.settings.borrow_mut();
                    let points = settings.menu_font_points;
                    settings.menu_font_points = if control == Control::MenuTextLess { points.saturating_sub(1).max(MENU_FONT_POINTS_MIN) } else { (points + 1).min(MENU_FONT_POINTS_MAX) };
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
            with_window(|state| {
                if was_confirming {
                    // Start with Windows stays as it is: it lives in Windows' own startup list.
                    *state.settings.borrow_mut() = Settings::default();
                    save_and_announce(state);
                } else {
                    state.is_confirming_reset = true;
                }
            });
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
        let Some(row_top) = placed_blocks(state).into_iter().find(|(block, _)| *block == Block::Device(index)).map(|(_, top)| top) else { return };
        // SAFETY: the window is alive while it is stored; the edit box is its child and dies with it.
        unsafe {
            let (content_left, _) = content_columns(dpi);
            let left = content_left + scale(DEVICE_TEXT_LEFT, dpi);
            let top = row_top + scale(5, dpi);
            let width = scale(200, dpi);
            let height = scale(20, dpi);
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
            let on_page_area = matches!(control, Control::Nav(_)) || (x >= scale(SIDEBAR_WIDTH, state.dpi) && y >= scale(HEADER_HEIGHT, state.dpi));
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
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: the window is alive while it is stored.
        unsafe { GetClientRect(state.window, &mut client) };
        let visible = client.bottom - scale(HEADER_HEIGHT, state.dpi);
        state.scroll = (state.scroll + delta).clamp(0, (content_height(state) - visible).max(0));
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
            match hit_test(x, y) {
                Some(control) => activate(control),
                None => {
                    // A click on empty space cancels a Reset all that waits for its second click.
                    if with_window(|state| std::mem::replace(&mut state.is_confirming_reset, false)) == Some(true) {
                        invalidate();
                    }
                }
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
                is_confirming_reset: false,
            });
        });
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
    }
}

/// Puts Reset all in its "Click to confirm" state, for screenshots of it.
pub fn show_reset_confirm() {
    with_window(|state| state.is_confirming_reset = true);
    invalidate();
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
