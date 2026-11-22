//! The flyout that opens from the tray icon, in the look of Windows 11's own tray flyouts: Acrylic
//! behind, rounded rows, Segoe UI Variable. One row per device, then Settings and Quit.

use crate::canvas::{create_font, Align, Canvas};
use crate::messages::WM_APP_MENU_ACTION;
use crate::status_icon::{focus_status_icon, status_icon_rect};
use crate::theme::{apply_window_look, background_opacity, current_theme, font_family, icon_font_family, on_theme_change, tone_color, Backdrop, Theme};
use crate::util::{scale, to_wide};
use juicebar_shared::device::{Connection, Device};
use juicebar_shared::tone::{tone_for_percent, Tone};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromRect, HFONT, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_APPS, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_F10, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, IsWindow, LoadCursorW,
    PostMessageW, RegisterClassW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, TrackPopupMenu, HTCLIENT, IDC_ARROW,
    IDC_HAND, MF_STRING, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_ACTIVATE,
    WM_CHAR, WM_CONTEXTMENU, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

// The windows-sys crate does not export this message, so it is declared here.
const WM_MOUSELEAVE: u32 = 0x02A3;

const CLASS_NAME: &str = "JuiceBarFlyout";
const WIDTH: i32 = 380;
const PADDING: i32 = 6;
const HEADER_HEIGHT: i32 = 44;
const DEVICE_ROW_HEIGHT: i32 = 40;
const FOOTER_ROW_HEIGHT: i32 = 36;
const DIVIDER_HEIGHT: i32 = 13;
const SIDE: i32 = 14;
const DOT_RADIUS: i32 = 4;
const NAME_LEFT: i32 = 32;
const PERCENT_WIDTH: i32 = 52;
const LABEL_GAP: i32 = 12;
const BUTTON_HEIGHT: i32 = 26;
const BUTTON_PAD: i32 = 10;
const BUTTON_GAP: i32 = 6;
const ROW_RADIUS: f32 = 4.0;
const SCREEN_MARGIN: i32 = 12;
// A click on the tray icon while the flyout is open first closes it; this stops that click reopening it.
const REOPEN_GUARD: Duration = Duration::from_millis(300);
const BOLT: &str = "\u{26A1}";
const ICON_SETTINGS: &str = "\u{E713}";
const ICON_QUIT: &str = "\u{E7E8}";
const REPORT_COMMAND: usize = 1;

/// What the user chose in the flyout. The app picks it up with `take_menu_action`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    OpenSettings,
    Quit,
    /// Report the device with this id.
    Report(String),
    /// Try every known protocol on the device with this id (the opt-in probe).
    Detect(String),
}

/// What sits at the right end of a device row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowEnd {
    /// The %, in its tone, with a bolt before it while charging.
    Percent { text: String, tone: Tone, is_charging: bool },
    /// Grey "Not connected": the dongle is in but the device sleeps or is off.
    NotConnected,
    /// Report and Try to detect, for a device no protocol recognised.
    Actions,
}

/// One device row, worked out from a device before any drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub connection: &'static str,
    pub end: RowEnd,
    /// Drawn in grey: no reading.
    pub is_dimmed: bool,
    /// The dot left of the name; None draws a hollow grey ring.
    pub dot: Option<Tone>,
}

/// Returns the label for how a device is connected.
pub fn connection_label(connection: Connection) -> &'static str {
    match connection {
        Connection::Dongle => "2.4 GHz",
        Connection::Bluetooth => "Bluetooth",
        Connection::Usb => "USB",
    }
}

/// Works out what a device's row shows.
pub fn build_device_row(device: &Device) -> DeviceRow {
    let end = match (device.battery, device.is_unrecognised()) {
        (Some(battery), _) => RowEnd::Percent { text: format!("{}%", battery.percent), tone: tone_for_percent(battery.percent), is_charging: battery.is_charging },
        (None, true) => RowEnd::Actions,
        (None, false) => RowEnd::NotConnected,
    };
    let dot = device.battery.map(|battery| tone_for_percent(battery.percent));
    DeviceRow {
        id: device.id.clone(),
        name: device.name.clone(),
        connection: connection_label(device.connection),
        is_dimmed: device.battery.is_none() && !device.is_unrecognised(),
        end,
        dot,
    }
}

/// Something in the flyout that reacts to the pointer or keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Device(usize),
    Report(usize),
    Detect(usize),
    Settings,
    Quit,
}

struct Fonts {
    title: HFONT,
    body: HFONT,
    strong: HFONT,
    small: HFONT,
    icon: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        let family = font_family();
        Self {
            title: create_font(family, 11.0, 600, dpi),
            body: create_font(family, 10.5, 400, dpi),
            strong: create_font(family, 10.5, 600, dpi),
            small: create_font(family, 9.0, 400, dpi),
            icon: create_font(icon_font_family(), 11.0, 400, dpi),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: each font was created in new() and is released once here.
        unsafe {
            for font in [self.title, self.body, self.strong, self.small, self.icon] {
                DeleteObject(font);
            }
        }
    }
}

struct Flyout {
    window: HWND,
    owner: HWND,
    rows: Vec<DeviceRow>,
    dpi: u32,
    fonts: Fonts,
    theme: Theme,
    hovered: Option<Target>,
    /// Set by the keyboard; shows a focus ring as Windows does for keyboard use only.
    focused: Option<Target>,
    is_tracking_mouse: bool,
    /// Set once Windows made the flyout active; only then does losing activation close it.
    was_active: bool,
}

thread_local! {
    static FLYOUT: RefCell<Option<Flyout>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<MenuAction>> = const { RefCell::new(None) };
    static LAST_CLOSED: RefCell<Option<Instant>> = const { RefCell::new(None) };
    static OWNER: RefCell<HWND> = const { RefCell::new(std::ptr::null_mut()) };
    static LISTENING: RefCell<bool> = const { RefCell::new(false) };
}

/// Sets the window that receives the flyout's actions as WM_APP_MENU_ACTION.
pub fn set_menu_owner(owner: HWND) {
    OWNER.with(|cell| *cell.borrow_mut() = owner);
}

/// Returns the action the user chose, once.
pub fn take_menu_action() -> Option<MenuAction> {
    PENDING.with(|cell| cell.borrow_mut().take())
}

/// Returns true while the flyout is open.
pub fn is_menu_open() -> bool {
    FLYOUT.with(|cell| cell.borrow().is_some())
}

/// Returns the flyout's height in pixels at 100% scaling for a number of device rows.
pub fn menu_height(device_rows: usize) -> i32 {
    let rows = device_rows.max(1) as i32;
    PADDING + HEADER_HEIGHT + rows * DEVICE_ROW_HEIGHT + DIVIDER_HEIGHT + 2 * FOOTER_ROW_HEIGHT + PADDING
}

/// Returns each target and its rectangle, at the flyout's scaling.
fn layout(flyout: &Flyout, canvas: Option<&Canvas>) -> Vec<(Target, RECT)> {
    let dpi = flyout.dpi;
    let width = scale(WIDTH, dpi);
    let mut targets = Vec::new();
    let mut top = scale(PADDING + HEADER_HEIGHT, dpi);
    let row_left = scale(PADDING, dpi);
    let row_right = width - scale(PADDING, dpi);
    for (index, row) in flyout.rows.iter().enumerate() {
        let bottom = top + scale(DEVICE_ROW_HEIGHT, dpi);
        targets.push((Target::Device(index), RECT { left: row_left, top, right: row_right, bottom }));
        if row.end == RowEnd::Actions {
            let measure = |text: &str| canvas.map_or(scale(70, dpi), |canvas| canvas.text_width(text, flyout.fonts.small)) + 2 * scale(BUTTON_PAD, dpi);
            let button_top = top + (scale(DEVICE_ROW_HEIGHT, dpi) - scale(BUTTON_HEIGHT, dpi)) / 2;
            let button_bottom = button_top + scale(BUTTON_HEIGHT, dpi);
            let detect_right = width - scale(SIDE, dpi);
            let detect_left = detect_right - measure("Try to detect");
            let report_right = detect_left - scale(BUTTON_GAP, dpi);
            let report_left = report_right - measure("Report");
            targets.push((Target::Report(index), RECT { left: report_left, top: button_top, right: report_right, bottom: button_bottom }));
            targets.push((Target::Detect(index), RECT { left: detect_left, top: button_top, right: detect_right, bottom: button_bottom }));
        }
        top = bottom;
    }
    if flyout.rows.is_empty() {
        top += scale(DEVICE_ROW_HEIGHT, dpi);
    }
    top += scale(DIVIDER_HEIGHT, dpi);
    for target in [Target::Settings, Target::Quit] {
        let bottom = top + scale(FOOTER_ROW_HEIGHT, dpi);
        targets.push((target, RECT { left: row_left, top, right: row_right, bottom }));
        top = bottom;
    }
    targets
}

/// Returns the target under a point; buttons win over the row they sit in.
fn hit_test(flyout: &Flyout, x: i32, y: i32) -> Option<Target> {
    let targets = layout(flyout, None);
    let inside = |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
    targets.iter().rev().find(|(_, rect)| inside(rect)).map(|(target, _)| *target)
}

/// Cuts text to fit a width, ending it with an ellipsis.
fn fit_text(canvas: &Canvas, text: &str, font: HFONT, max_width: i32) -> String {
    if canvas.text_width(text, font) <= max_width {
        return text.to_string();
    }
    let mut characters: Vec<char> = text.chars().collect();
    while !characters.is_empty() {
        characters.pop();
        let candidate: String = characters.iter().collect::<String>().trim_end().to_string() + "\u{2026}";
        if canvas.text_width(&candidate, font) <= max_width {
            return candidate;
        }
    }
    String::new()
}

/// Draws the hover or pressed wash on a rectangle, as Windows 11 menus do.
fn draw_wash(canvas: &mut Canvas, theme: &Theme, rect: &RECT, strength: f32) {
    let opacity = if theme.is_high_contrast { 1.0 } else if theme.is_dark { 0.06 * strength } else { 0.04 * strength };
    canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, ROW_RADIUS, theme.hover, opacity);
}

fn draw_focus_ring(canvas: &mut Canvas, theme: &Theme, rect: &RECT, dpi: u32) {
    let thickness = scale(2, dpi) as f32;
    canvas.round_outline(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, ROW_RADIUS + 1.0, thickness, theme.text, 1.0);
}

fn paint(flyout: &Flyout, canvas: &mut Canvas) {
    let theme = flyout.theme;
    let dpi = flyout.dpi;
    let fonts = &flyout.fonts;
    let width = canvas.width();
    canvas.clear(theme.background, background_opacity(&theme));
    let text_color = |dimmed: bool| if dimmed { theme.muted } else { theme.text };
    let title_height = canvas.text_height(fonts.title);
    canvas.text(scale(PADDING + SIDE, dpi), scale(PADDING, dpi) + (scale(HEADER_HEIGHT, dpi) - title_height) / 2, "Juice Bar", fonts.title, theme.text, Align::Left);
    let targets = layout(flyout, Some(canvas));
    let rect_of = |wanted: Target| targets.iter().find(|(target, _)| *target == wanted).map(|(_, rect)| *rect);
    let is_lit = |target: Target| flyout.hovered == Some(target) || flyout.focused == Some(target);

    for (index, row) in flyout.rows.iter().enumerate() {
        let Some(rect) = rect_of(Target::Device(index)) else { continue };
        let row_lit = is_lit(Target::Device(index));
        if row_lit {
            draw_wash(canvas, &theme, &rect, 1.0);
        }
        if flyout.focused == Some(Target::Device(index)) {
            draw_focus_ring(canvas, &theme, &rect, dpi);
        }
        let on_wash = row_lit && theme.is_high_contrast;
        let middle = rect.top + (rect.bottom - rect.top) / 2;
        let dot_x = rect.left + scale(SIDE + DOT_RADIUS, dpi) - scale(PADDING, dpi) + scale(4, dpi);
        match row.dot {
            Some(tone) => canvas.circle(dot_x, middle, scale(DOT_RADIUS, dpi), tone_color(tone, &theme)),
            None => {
                let radius = scale(DOT_RADIUS, dpi);
                canvas.round_outline(dot_x - radius, middle - radius, radius * 2, radius * 2, radius as f32, scale(1, dpi).max(1) as f32 * 1.5, theme.muted, 1.0);
            }
        }
        let right = width - scale(SIDE, dpi);
        let (end_left, label_right) = match &row.end {
            RowEnd::Actions => {
                let report = rect_of(Target::Report(index)).unwrap_or(rect);
                (report.left, report.left - scale(LABEL_GAP, dpi))
            }
            _ => (right - scale(PERCENT_WIDTH, dpi), right - scale(PERCENT_WIDTH, dpi) - scale(LABEL_GAP, dpi)),
        };
        let label_width = canvas.text_width(row.connection, fonts.small);
        let small_height = canvas.text_height(fonts.small);
        let body_height = canvas.text_height(fonts.body);
        let name_left = rect.left + scale(NAME_LEFT, dpi) - scale(PADDING, dpi) + scale(4, dpi);
        let name_max = label_right - label_width - scale(LABEL_GAP, dpi) - name_left;
        let name = fit_text(canvas, &row.name, fonts.body, name_max);
        let name_color = if on_wash { theme.on_accent } else { text_color(row.is_dimmed) };
        canvas.text(name_left, middle - body_height / 2, &name, fonts.body, name_color, Align::Left);
        canvas.text(label_right, middle - small_height / 2, row.connection, fonts.small, if on_wash { theme.on_accent } else { theme.muted }, Align::Right);
        match &row.end {
            RowEnd::Percent { text, tone, is_charging } => {
                let strong_height = canvas.text_height(fonts.strong);
                canvas.text(right, middle - strong_height / 2, text, fonts.strong, tone_color(*tone, &theme), Align::Right);
                if *is_charging {
                    let text_width = canvas.text_width(text, fonts.strong);
                    canvas.text(right - text_width - scale(2, dpi), middle - strong_height / 2, BOLT, fonts.strong, tone_color(*tone, &theme), Align::Right);
                }
            }
            RowEnd::NotConnected => {
                canvas.text(right, middle - small_height / 2, "Not connected", fonts.small, theme.muted, Align::Right);
            }
            RowEnd::Actions => {
                for (target, label) in [(Target::Report(index), "Report"), (Target::Detect(index), "Try to detect")] {
                    let Some(button) = rect_of(target) else { continue };
                    let lit = is_lit(target);
                    let fill = if theme.is_dark { 0xFFFFFF } else { 0x000000 };
                    let opacity = if lit { 0.12 } else { 0.06 };
                    canvas.round_rect(button.left, button.top, button.right - button.left, button.bottom - button.top, ROW_RADIUS, fill, opacity);
                    if flyout.focused == Some(target) {
                        draw_focus_ring(canvas, &theme, &button, dpi);
                    }
                    let center = button.left + (button.right - button.left) / 2;
                    canvas.text(center, button.top + (button.bottom - button.top - small_height) / 2, label, fonts.small, theme.accent_text(), Align::Center);
                }
            }
        }
        let _ = end_left;
    }
    if flyout.rows.is_empty() {
        let top = scale(PADDING + HEADER_HEIGHT, dpi);
        let body_height = canvas.text_height(fonts.body);
        canvas.text(scale(PADDING + SIDE, dpi), top + (scale(DEVICE_ROW_HEIGHT, dpi) - body_height) / 2, "No wireless mouse or keyboard found", fonts.body, theme.muted, Align::Left);
    }

    if let Some(settings) = rect_of(Target::Settings) {
        let divider_y = settings.top - scale(DIVIDER_HEIGHT, dpi) / 2 - 1;
        canvas.round_rect(scale(PADDING + SIDE, dpi) - scale(4, dpi), divider_y, width - 2 * (scale(PADDING + SIDE, dpi) - scale(4, dpi)), scale(1, dpi).max(1), 0.0, theme.border, 1.0);
    }
    for (target, icon, label, shortcut) in [(Target::Settings, ICON_SETTINGS, "Settings...", "Ctrl+,"), (Target::Quit, ICON_QUIT, "Quit Juice Bar", "Ctrl+Q")] {
        let Some(rect) = rect_of(target) else { continue };
        let lit = is_lit(target);
        if lit {
            draw_wash(canvas, &theme, &rect, 1.0);
        }
        if flyout.focused == Some(target) {
            draw_focus_ring(canvas, &theme, &rect, dpi);
        }
        let color = if lit && theme.is_high_contrast { theme.on_accent } else { theme.text };
        let middle = rect.top + (rect.bottom - rect.top) / 2;
        let icon_height = canvas.text_height(fonts.icon);
        let body_height = canvas.text_height(fonts.body);
        let small_height = canvas.text_height(fonts.small);
        canvas.text(rect.left + scale(SIDE - PADDING + 6, dpi), middle - icon_height / 2, icon, fonts.icon, color, Align::Left);
        canvas.text(rect.left + scale(NAME_LEFT - PADDING + 4, dpi) + scale(6, dpi), middle - body_height / 2, label, fonts.body, color, Align::Left);
        canvas.text(width - scale(SIDE, dpi), middle - small_height / 2, shortcut, fonts.small, if lit && theme.is_high_contrast { theme.on_accent } else { theme.muted }, Align::Right);
    }
}

impl Theme {
    /// Returns the colour for link-like button text: the accent, lifted in dark mode so it reads.
    pub fn accent_text(&self) -> u32 {
        if self.is_high_contrast {
            return self.text;
        }
        let mix = |amount: f32| {
            let channel = |shift: u32| {
                let start = ((self.accent >> shift) & 0xFF) as f32;
                let end = if self.is_dark { 255.0 } else { 0.0 };
                ((start + (end - start) * amount).round() as u32) << shift
            };
            channel(16) | channel(8) | channel(0)
        };
        if self.is_dark { mix(0.45) } else { mix(0.15) }
    }
}

/// Sends an action to the owner window and closes the flyout.
fn choose(action: MenuAction) {
    PENDING.with(|cell| *cell.borrow_mut() = Some(action));
    let owner = OWNER.with(|cell| *cell.borrow());
    close_menu();
    // SAFETY: posting to our own window; a closed owner simply drops the message.
    unsafe { PostMessageW(owner, WM_APP_MENU_ACTION, 0, 0) };
}

/// Runs what a target does when clicked or picked with Enter.
fn activate(target: Target) {
    let rows = FLYOUT.with(|cell| cell.borrow().as_ref().map(|flyout| flyout.rows.clone())).unwrap_or_default();
    match target {
        Target::Settings => choose(MenuAction::OpenSettings),
        Target::Quit => choose(MenuAction::Quit),
        Target::Report(index) => choose(MenuAction::Report(rows[index].id.clone())),
        Target::Detect(index) => choose(MenuAction::Detect(rows[index].id.clone())),
        Target::Device(index) if rows[index].end == RowEnd::Actions => choose(MenuAction::Report(rows[index].id.clone())),
        Target::Device(index) => show_row_menu(index, None),
    }
}

/// Shows the right-click menu of a known device row: Report, for a wrong name or %.
fn show_row_menu(index: usize, at: Option<POINT>) {
    let Some((window, id)) = FLYOUT.with(|cell| cell.borrow().as_ref().map(|flyout| (flyout.window, flyout.rows.get(index).map(|row| row.id.clone())))) else {
        return;
    };
    let Some(id) = id else { return };
    let label = to_wide("Report wrong name or %");
    // SAFETY: the menu is created, shown modally on our window, and destroyed here.
    let chosen = unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, REPORT_COMMAND, label.as_ptr());
        let mut point = at.unwrap_or(POINT { x: 0, y: 0 });
        if at.is_none() {
            GetCursorPos(&mut point);
        }
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, point.x, point.y, 0, window, std::ptr::null());
        DestroyMenu(menu);
        chosen
    };
    if chosen as usize == REPORT_COMMAND {
        choose(MenuAction::Report(id));
    }
}

fn focusable(flyout: &Flyout) -> Vec<Target> {
    layout(flyout, None).into_iter().map(|(target, _)| target).collect()
}

fn move_focus(step: i32) {
    FLYOUT.with(|cell| {
        if let Some(flyout) = cell.borrow_mut().as_mut() {
            let order = focusable(flyout);
            let current = flyout.focused.and_then(|target| order.iter().position(|item| *item == target));
            let next = match current {
                None if step > 0 => 0,
                None => order.len() - 1,
                Some(position) => (position as i32 + step).rem_euclid(order.len() as i32) as usize,
            };
            flyout.focused = Some(order[next]);
            flyout.hovered = None;
            // SAFETY: the flyout window is alive while it is stored.
            unsafe { InvalidateRect(flyout.window, std::ptr::null(), 0) };
        }
    });
}

fn set_hovered(target: Option<Target>) {
    FLYOUT.with(|cell| {
        if let Some(flyout) = cell.borrow_mut().as_mut() {
            if flyout.hovered != target {
                flyout.hovered = target;
                // SAFETY: the flyout window is alive while it is stored.
                unsafe { InvalidateRect(flyout.window, std::ptr::null(), 0) };
            }
        }
    });
}

fn with_flyout<T>(action: impl FnOnce(&mut Flyout) -> T) -> Option<T> {
    FLYOUT.with(|cell| cell.borrow_mut().as_mut().map(action))
}

fn is_key_down(key: u16) -> bool {
    // SAFETY: plain key state lookup.
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

unsafe extern "system" fn flyout_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let point_of = |lparam: LPARAM| ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32);
    match message {
        WM_PAINT => {
            let mut paint_info: PAINTSTRUCT = std::mem::zeroed();
            let target = BeginPaint(window, &mut paint_info);
            with_flyout(|flyout| {
                let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(window, &mut rect);
                let mut canvas = Canvas::new(rect.right, rect.bottom);
                paint(flyout, &mut canvas);
                canvas.present(target);
            });
            EndPaint(window, &paint_info);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let (x, y) = point_of(lparam);
            let should_track = with_flyout(|flyout| {
                let first = !flyout.is_tracking_mouse;
                flyout.is_tracking_mouse = true;
                flyout.focused = None;
                first
            });
            if should_track == Some(true) {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: window, dwHoverTime: 0 };
                TrackMouseEvent(&mut track);
            }
            let target = with_flyout(|flyout| hit_test(flyout, x, y)).flatten();
            set_hovered(target);
            0
        }
        WM_MOUSELEAVE => {
            with_flyout(|flyout| flyout.is_tracking_mouse = false);
            set_hovered(None);
            0
        }
        WM_SETCURSOR => {
            let over_button = with_flyout(|flyout| matches!(flyout.hovered, Some(Target::Report(_) | Target::Detect(_)))).unwrap_or(false);
            if (lparam & 0xFFFF) as u32 == HTCLIENT {
                SetCursor(LoadCursorW(std::ptr::null_mut(), if over_button { IDC_HAND } else { IDC_ARROW }));
                return 1;
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        WM_LBUTTONUP => {
            let (x, y) = point_of(lparam);
            if let Some(target) = with_flyout(|flyout| hit_test(flyout, x, y)).flatten() {
                if !matches!(target, Target::Device(_)) || with_flyout(|flyout| matches!(target, Target::Device(index) if flyout.rows[index].end == RowEnd::Actions)) == Some(true) {
                    activate(target);
                }
            }
            0
        }
        WM_RBUTTONUP => {
            let (x, y) = point_of(lparam);
            if let Some(Target::Device(index)) = with_flyout(|flyout| hit_test(flyout, x, y)).flatten() {
                if with_flyout(|flyout| flyout.rows[index].end != RowEnd::Actions) == Some(true) {
                    show_row_menu(index, None);
                }
            }
            0
        }
        WM_CONTEXTMENU => {
            if let Some(Some(Target::Device(index))) = with_flyout(|flyout| flyout.focused) {
                let rect = with_flyout(|flyout| layout(flyout, None).into_iter().find(|(target, _)| *target == Target::Device(index)).map(|(_, rect)| rect)).flatten();
                if let Some(rect) = rect {
                    let mut point = POINT { x: rect.left + 40, y: rect.bottom };
                    windows_sys::Win32::Graphics::Gdi::ClientToScreen(window, &mut point);
                    show_row_menu(index, Some(point));
                }
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let key = wparam as u16;
            match key {
                VK_ESCAPE => {
                    close_menu();
                    focus_status_icon();
                }
                VK_DOWN => move_focus(1),
                VK_UP => move_focus(-1),
                VK_TAB => move_focus(if is_key_down(VK_SHIFT) { -1 } else { 1 }),
                VK_RETURN | VK_SPACE => {
                    if let Some(Some(target)) = with_flyout(|flyout| flyout.focused) {
                        activate(target);
                    }
                }
                VK_APPS => {
                    PostMessageW(window, WM_CONTEXTMENU, window as usize, -1);
                }
                VK_F10 if is_key_down(VK_SHIFT) => {
                    PostMessageW(window, WM_CONTEXTMENU, window as usize, -1);
                }
                _ if is_key_down(VK_CONTROL) && key == u16::from(b'Q') => choose(MenuAction::Quit),
                _ if is_key_down(VK_CONTROL) && key == 0xBC => choose(MenuAction::OpenSettings),
                _ => return DefWindowProcW(window, message, wparam, lparam),
            }
            0
        }
        WM_CHAR => 0,
        WM_ACTIVATE => {
            if wparam & 0xFFFF == 0 {
                if with_flyout(|flyout| flyout.was_active) == Some(true) {
                    close_menu();
                }
            } else {
                with_flyout(|flyout| flyout.was_active = true);
            }
            0
        }
        WM_DPICHANGED => {
            let dpi = (wparam & 0xFFFF) as u32;
            with_flyout(|flyout| {
                flyout.dpi = dpi;
                flyout.fonts = Fonts::new(dpi);
            });
            let suggested = &*(lparam as *const RECT);
            SetWindowPos(window, std::ptr::null_mut(), suggested.left, suggested.top, suggested.right - suggested.left, suggested.bottom - suggested.top, SWP_NOZORDER | SWP_NOACTIVATE);
            0
        }
        WM_DESTROY => 0,
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

fn register_class() {
    let class_name = to_wide(CLASS_NAME);
    // SAFETY: the class name outlives the call; registering twice only fails harmlessly.
    unsafe {
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(flyout_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&class);
    }
}

/// Returns where the flyout goes: next to the tray icon, inside the screen's free area, away from the taskbar.
pub fn place_flyout(anchor: RECT, monitor: RECT, work: RECT, width: i32, height: i32, margin: i32) -> (i32, i32) {
    let center_x = anchor.left + (anchor.right - anchor.left) / 2;
    let center_y = anchor.top + (anchor.bottom - anchor.top) / 2;
    let clamp_x = |x: i32| x.clamp(work.left + margin, (work.right - margin - width).max(work.left + margin));
    let clamp_y = |y: i32| y.clamp(work.top + margin, (work.bottom - margin - height).max(work.top + margin));
    if work.top > monitor.top {
        (clamp_x(center_x - width / 2), work.top + margin)
    } else if work.left > monitor.left {
        (work.left + margin, clamp_y(center_y - height / 2))
    } else if work.right < monitor.right {
        (work.right - margin - width, clamp_y(center_y - height / 2))
    } else {
        (clamp_x(center_x - width / 2), work.bottom - margin - height)
    }
}

/// Returns the flyout's position, size and scaling for a number of rows, next to the anchor.
fn flyout_bounds(anchor: RECT, row_count: usize) -> (i32, i32, i32, i32, u32) {
    // SAFETY: plain lookups with out pointers that outlive the calls.
    unsafe {
        let monitor = MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let dpi = dpi_x.max(96);
        let width = scale(WIDTH, dpi);
        let height = scale(menu_height(row_count), dpi);
        let (x, y) = place_flyout(anchor, info.rcMonitor, info.rcWork, width, height, scale(SCREEN_MARGIN, dpi));
        (x, y, width, height, dpi)
    }
}

fn anchor_rect() -> RECT {
    status_icon_rect().unwrap_or_else(|| {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `point` outlives the call.
        unsafe { GetCursorPos(&mut point) };
        RECT { left: point.x, top: point.y, right: point.x + 1, bottom: point.y + 1 }
    })
}

/// Opens the flyout next to the tray icon with these devices. A second call while open closes it.
pub fn show_menu(devices: &[Device]) {
    if is_menu_open() {
        close_menu();
        return;
    }
    let just_closed = LAST_CLOSED.with(|cell| cell.borrow().is_some_and(|closed| closed.elapsed() < REOPEN_GUARD));
    if just_closed {
        return;
    }
    register_class();
    listen_for_theme();
    let rows: Vec<DeviceRow> = devices.iter().map(build_device_row).collect();
    let anchor = anchor_rect();
    // SAFETY: Win32 window creation on the UI thread; every pointer passed outlives its call.
    unsafe {
        let (x, y, width, height, dpi) = flyout_bounds(anchor, rows.len());
        let class_name = to_wide(CLASS_NAME);
        let title = to_wide("Juice Bar");
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        let theme = current_theme();
        apply_window_look(window, &theme, Backdrop::Flyout);
        let owner = OWNER.with(|cell| *cell.borrow());
        FLYOUT.with(|cell| {
            *cell.borrow_mut() = Some(Flyout { window, owner, rows, dpi, fonts: Fonts::new(dpi), theme, hovered: None, focused: None, is_tracking_mouse: false, was_active: false });
        });
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
    }
}

/// Shows new readings in the open flyout, resizing it when the number of rows changed.
pub fn refresh_menu(devices: &[Device]) {
    let rows: Vec<DeviceRow> = devices.iter().map(build_device_row).collect();
    let row_count = rows.len();
    let resized = with_flyout(|flyout| {
        let resized = flyout.rows.len().max(1) != row_count.max(1);
        flyout.rows = rows;
        flyout.hovered = None;
        flyout.focused = None;
        (flyout.window, resized)
    });
    if let Some((window, resized)) = resized {
        // SAFETY: the flyout window is alive while it is stored.
        unsafe {
            if resized {
                let (x, y, width, height, _) = flyout_bounds(anchor_rect(), row_count);
                SetWindowPos(window, std::ptr::null_mut(), x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            InvalidateRect(window, std::ptr::null(), 0);
        }
    }
}

/// Closes the flyout if it is open.
pub fn close_menu() {
    let flyout = FLYOUT.with(|cell| cell.borrow_mut().take());
    if let Some(flyout) = flyout {
        LAST_CLOSED.with(|cell| *cell.borrow_mut() = Some(Instant::now()));
        // SAFETY: the window was created by show_menu and is destroyed once here.
        unsafe {
            if IsWindow(flyout.window) != 0 {
                DestroyWindow(flyout.window);
            }
        }
        let _ = flyout.owner;
    }
}

/// Redraws the open flyout in the new look whenever Windows' look changes.
fn listen_for_theme() {
    let already = LISTENING.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), true));
    if already {
        return;
    }
    on_theme_change(|| {
        with_flyout(|flyout| {
            flyout.theme = current_theme();
            apply_window_look(flyout.window, &flyout.theme, Backdrop::Flyout);
            // SAFETY: the flyout window is alive while it is stored.
            unsafe { InvalidateRect(flyout.window, std::ptr::null(), 0) };
        });
    });
}
