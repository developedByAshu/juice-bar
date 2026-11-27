//! The two flyouts that open from the tray icon, drawn like NetSpeedGauge's popup: a left click (or
//! hovering, when chosen) shows the devices flyout, a title line and one compact row per device;
//! a right click shows the menu, Settings and Quit only. Long names are cut and the full name,
//! connection and state show in a tooltip.

use crate::canvas::{create_font, mix, Align, Canvas};
use crate::messages::WM_APP_MENU_ACTION;
use crate::status_icon::{focus_status_icon, status_icon_rect};
use crate::theme::{apply_window_look, current_theme, on_theme_change, tone_color, Backdrop, Theme, ICON_FONT, UI_FONT, UI_FONT_STRONG};
use crate::util::{copy_into, scale, to_wide};
use juicebar_shared::device::{Connection, Device};
use juicebar_shared::names::short_name;
use juicebar_shared::tone::{tone_for_percent, Tone};
use juicebar_shared::settings::{MENU_FONT_POINTS_DEFAULT, MENU_FONT_POINTS_MAX, MENU_FONT_POINTS_MIN};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromRect, HFONT, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{TOOLTIPS_CLASSW, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_APPS, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_F10, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, IsWindow, LoadCursorW,
    PostMessageW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, TrackPopupMenu, IDC_ARROW, MF_STRING,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOW, SW_SHOWNOACTIVATE, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_ACTIVATE, WM_CHAR, WM_CONTEXTMENU,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

// The windows-sys crate does not export this message, so it is declared here.
const WM_MOUSELEAVE: u32 = 0x02A3;

// Sizes from NetSpeedGauge's popup menu (windows/src/popup_ui.rs), at 100% scaling.
const CLASS_NAME: &str = "JuiceBarFlyout";
const MIN_WIDTH: i32 = 164;
const HEADER_HEIGHT: i32 = 34;
const ROW_HEIGHT: i32 = 32;
const PADDING: i32 = 5;
const SIDE: i32 = 14;
const LABEL_LEFT: i32 = 40;
// The least space between a name and the right column, so the two never touch.
const PERCENT_GAP: i32 = 24;
const DOT_RADIUS: i32 = 4;
const HOVER_RADIUS: i32 = 5;
// The flyout is 80% of NetSpeedGauge's popup at the default text size (his call, 28 Nov).
const MENU_SIZE: f32 = 0.8;
// No gap: the flyout opens flush against the taskbar, as NetSpeedGauge's popup does.
const SCREEN_MARGIN: i32 = 0;
const TOOLTIP_WIDTH: i32 = 320;
// A click on the tray icon while the flyout is open first closes it; this stops that click reopening it.
const REOPEN_GUARD: Duration = Duration::from_millis(300);
const BOLT: &str = "\u{26A1}";
const ICON_SETTINGS: &str = "\u{E713}";
const ICON_QUIT: &str = "\u{E7E8}";
/// Wi-Fi with a cross, in place of the words "Not connected"; the words stay in the tooltip.
const ICON_NOT_CONNECTED: &str = "\u{EB5E}";
const NOT_CONNECTED_TEXT: &str = "Not connected";
const EMPTY_TEXT: &str = "No devices found";
const SHORTCUT_SETTINGS: &str = "Ctrl+,";
const SHORTCUT_QUIT: &str = "Ctrl+Q";
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

/// What sits in the % column of a device row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowEnd {
    /// The %, in its tone, with a bolt before it while charging.
    Percent { text: String, tone: Tone, is_charging: bool },
    /// The no-signal icon: the dongle is in but the device sleeps or is off.
    NotConnected,
    /// Nothing: no protocol recognised the device, and a click tries to detect it.
    Actions,
}

/// One device row, worked out from a device before any drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    pub id: String,
    /// The name as shown, cut to fit the menu.
    pub name: String,
    /// The full name, connection and state, shown on hover.
    pub tooltip: String,
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
    let connection = connection_label(device.connection);
    let base = format!("{} \u{00B7} {connection}", device.name);
    let tooltip = match end {
        RowEnd::NotConnected => format!("{base} \u{00B7} {NOT_CONNECTED_TEXT}"),
        RowEnd::Actions => format!("{base} \u{00B7} Click to try to detect, right-click to report it"),
        RowEnd::Percent { .. } => base,
    };
    DeviceRow {
        id: device.id.clone(),
        name: short_name(&device.name),
        tooltip,
        connection,
        is_dimmed: device.battery.is_none() && !device.is_unrecognised(),
        end,
        dot: device.battery.map(|battery| tone_for_percent(battery.percent)),
    }
}

/// Something in the flyout that reacts to the pointer or keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Device(usize),
    Settings,
    Quit,
}

struct Fonts {
    body: HFONT,
    strong: HFONT,
    small: HFONT,
    icon: HFONT,
}

impl Fonts {
    /// Rows use the menu text size (9 pt by default), the header and shortcuts 1 pt less, icons in step.
    fn new(dpi: u32) -> Self {
        let points = menu_points() as f32;
        Self {
            body: create_font(UI_FONT, points, 400, dpi),
            strong: create_font(UI_FONT_STRONG, points, 400, dpi),
            small: create_font(UI_FONT, points - 1.0, 400, dpi),
            icon: create_font(ICON_FONT, 11.0 * menu_scale(menu_points()), 400, dpi),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: each font was created in new() and is released once here.
        unsafe {
            for font in [self.body, self.strong, self.small, self.icon] {
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
    /// The tooltip window that shows each row's full name.
    tooltip: HWND,
    /// The battery card: the device rows only, without Settings and Quit.
    is_card: bool,
    /// Opened by hovering: it never takes focus and closes when the pointer leaves the icon.
    is_hover: bool,
}

thread_local! {
    static FLYOUT: RefCell<Option<Flyout>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<MenuAction>> = const { RefCell::new(None) };
    static LAST_CLOSED: RefCell<Option<Instant>> = const { RefCell::new(None) };
    static OWNER: RefCell<HWND> = const { RefCell::new(std::ptr::null_mut()) };
    static LISTENING: RefCell<bool> = const { RefCell::new(false) };
    static MENU_POINTS: Cell<u32> = const { Cell::new(MENU_FONT_POINTS_DEFAULT) };
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

/// Sets the menu text size in points, from Settings; the whole flyout's geometry follows it.
pub fn set_menu_text_points(points: u32) {
    MENU_POINTS.with(|cell| cell.set(points.clamp(MENU_FONT_POINTS_MIN, MENU_FONT_POINTS_MAX)));
}

fn menu_points() -> u32 {
    MENU_POINTS.with(Cell::get)
}

/// Returns how much of NetSpeedGauge's size the flyout uses: 80% at the default text size, growing with it.
pub fn menu_scale(points: u32) -> f32 {
    MENU_SIZE * points as f32 / MENU_FONT_POINTS_DEFAULT as f32
}

/// Scales one of NetSpeedGauge's lengths by the menu scale and the screen scaling.
fn unit(value: i32, dpi: u32) -> i32 {
    (value as f32 * menu_scale(menu_points()) * dpi as f32 / 96.0).round() as i32
}

/// Returns the devices flyout's height in pixels at 100% scaling for a number of device rows and a text size.
pub fn menu_height(device_rows: usize, points: u32) -> i32 {
    let total = HEADER_HEIGHT + PADDING + device_rows.max(1) as i32 * ROW_HEIGHT + PADDING;
    (total as f32 * menu_scale(points)).round() as i32
}

/// Returns the height at 100% scaling of the devices flyout, or of the right-click menu's two rows.
fn flyout_height(device_rows: usize, points: u32, is_card: bool) -> i32 {
    if is_card {
        return menu_height(device_rows, points);
    }
    ((PADDING * 2 + ROW_HEIGHT * 2) as f32 * menu_scale(points)).round() as i32
}

/// Returns the y that centres a line of text in a row.
fn centred(canvas: &Canvas, rect: &RECT, font: HFONT) -> i32 {
    rect.top + (rect.bottom - rect.top - canvas.text_height(font)) / 2
}

/// Returns the text of a row's % column, or None for the no-signal icon or an empty column.
fn percent_text(end: &RowEnd) -> Option<String> {
    match end {
        RowEnd::Percent { text, is_charging: true, .. } => Some(format!("{BOLT}{text}")),
        RowEnd::Percent { text, .. } => Some(text.clone()),
        _ => None,
    }
}

/// Returns the width of the label column and of the whole flyout: as narrow as its rows allow. The
/// devices flyout sizes to its names and readings, the right-click menu to Settings and its shortcuts.
fn column_widths(canvas: &Canvas, fonts: &Fonts, rows: &[DeviceRow], dpi: u32, is_card: bool) -> (i32, i32) {
    let (names, right_column) = if is_card {
        let names = rows.iter().map(|row| canvas.text_width(&row.name, fonts.body)).max().unwrap_or_else(|| canvas.text_width(EMPTY_TEXT, fonts.body));
        // The right column is as wide as its widest possible value, so a reading never pushes into the name.
        let widest_percent = format!("{BOLT}100%");
        (names, canvas.text_width(&widest_percent, fonts.strong).max(canvas.text_width(ICON_NOT_CONNECTED, fonts.icon)))
    } else {
        let shortcuts = canvas.text_width(SHORTCUT_SETTINGS, fonts.small).max(canvas.text_width(SHORTCUT_QUIT, fonts.small));
        (canvas.text_width("Settings", fonts.strong), shortcuts)
    };
    let labels = names;
    let width = unit(LABEL_LEFT, dpi) + labels + unit(PERCENT_GAP, dpi) + right_column + unit(SIDE, dpi);
    (names, width.max(unit(MIN_WIDTH, dpi)))
}

/// Returns the y position of a device row, under the title line.
fn row_top(row: usize, dpi: u32) -> i32 {
    unit(HEADER_HEIGHT + PADDING + row as i32 * ROW_HEIGHT, dpi)
}

/// Returns each target and its rectangle, at the flyout's scaling.
fn layout(flyout: &Flyout, width: i32) -> Vec<(Target, RECT)> {
    let dpi = flyout.dpi;
    let count = flyout.rows.len();
    let (left, right) = (unit(PADDING, dpi), width - unit(PADDING, dpi));
    let rect = |top: i32| RECT { left, top, right, bottom: top + unit(ROW_HEIGHT, dpi) };
    if flyout.is_card {
        return (0..count).map(|index| (Target::Device(index), rect(row_top(index, dpi)))).collect();
    }
    // The right-click menu: Settings and Quit only.
    vec![(Target::Settings, rect(unit(PADDING, dpi))), (Target::Quit, rect(unit(PADDING + ROW_HEIGHT, dpi)))]
}

/// Returns the flyout's client width, read from its window.
fn client_width(window: HWND) -> i32 {
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: plain lookup on a live window.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(window, &mut rect) };
    rect.right
}

/// Returns the target under a point.
fn hit_test(flyout: &Flyout, x: i32, y: i32) -> Option<Target> {
    let inside = |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
    layout(flyout, client_width(flyout.window)).iter().find(|(_, rect)| inside(rect)).map(|(target, _)| *target)
}

/// Cuts text to fit a width, ending it with an ellipsis.
pub fn fit_text(canvas: &Canvas, text: &str, font: HFONT, max_width: i32) -> String {
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

fn draw_focus_ring(canvas: &mut Canvas, theme: &Theme, rect: &RECT, dpi: u32) {
    let thickness = scale(2, dpi) as f32;
    canvas.round_outline(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, unit(HOVER_RADIUS + 1, dpi) as f32, thickness, theme.text, 1.0);
}

fn paint(flyout: &Flyout, canvas: &mut Canvas) {
    let theme = flyout.theme;
    let dpi = flyout.dpi;
    let fonts = &flyout.fonts;
    let width = canvas.width();
    canvas.clear(theme.surface, 1.0);
    if flyout.is_card {
        // A title line only, as on the Mac. No lowest %: the jar icon hints at it and every row shows its exact %.
        let divider_x = unit(PADDING, dpi);
        let header = RECT { left: 0, top: 0, right: width, bottom: unit(HEADER_HEIGHT, dpi) };
        canvas.text(unit(SIDE, dpi), centred(canvas, &header, fonts.small), "Juice Bar", fonts.small, theme.muted, Align::Left);
        canvas.round_rect(divider_x, unit(HEADER_HEIGHT, dpi), width - divider_x * 2, scale(1, dpi).max(1), 0.0, theme.border, 1.0);
        if flyout.rows.is_empty() {
            let top = row_top(0, dpi);
            canvas.text(unit(SIDE, dpi), centred(canvas, &RECT { left: 0, top, right: width, bottom: top + unit(ROW_HEIGHT, dpi) }, fonts.body), EMPTY_TEXT, fonts.body, theme.muted, Align::Left);
        }
    }

    let targets = layout(flyout, width);
    let (name_column, _) = column_widths(canvas, fonts, &flyout.rows, dpi, flyout.is_card);
    let draw_wash = |canvas: &mut Canvas, rect: &RECT| {
        let hover = mix(theme.surface, theme.text, 0.09);
        canvas.round_rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, unit(HOVER_RADIUS, dpi) as f32, hover, 1.0);
    };
    for (target, rect) in &targets {
        let is_lit = flyout.hovered == Some(*target) || flyout.focused == Some(*target);
        if is_lit {
            draw_wash(canvas, rect);
        }
        if flyout.focused == Some(*target) {
            draw_focus_ring(canvas, &theme, rect, dpi);
        }
        let label_x = unit(LABEL_LEFT, dpi);
        match target {
            Target::Device(index) => {
                let row = &flyout.rows[*index];
                let middle = rect.top + (rect.bottom - rect.top) / 2;
                let dot_x = unit(SIDE + DOT_RADIUS + 2, dpi);
                let radius = unit(DOT_RADIUS, dpi);
                match row.dot {
                    Some(tone) => canvas.circle(dot_x, middle, radius, tone_color(tone, &theme)),
                    None => canvas.round_outline(dot_x - radius, middle - radius, radius * 2, radius * 2, radius as f32, 1.5, theme.muted, 1.0),
                }
                let name_color = if row.is_dimmed { theme.muted } else { theme.text };
                let name = fit_text(canvas, &row.name, fonts.body, name_column);
                canvas.text(label_x, centred(canvas, rect, fonts.body), &name, fonts.body, name_color, Align::Left);
                let right = width - unit(SIDE, dpi);
                match &row.end {
                    RowEnd::Percent { tone, .. } => {
                        let text = percent_text(&row.end).unwrap_or_default();
                        canvas.text(right, centred(canvas, rect, fonts.strong), &text, fonts.strong, tone_color(*tone, &theme), Align::Right);
                    }
                    RowEnd::NotConnected => canvas.text(right, centred(canvas, rect, fonts.icon), ICON_NOT_CONNECTED, fonts.icon, theme.muted, Align::Right),
                    RowEnd::Actions => {}
                }
            }
            Target::Settings | Target::Quit => {
                let (icon, label) = if *target == Target::Settings { (ICON_SETTINGS, "Settings") } else { (ICON_QUIT, "Quit") };
                let icon_color = if is_lit { theme.text } else { theme.muted };
                canvas.text(unit(SIDE, dpi), centred(canvas, rect, fonts.icon), icon, fonts.icon, icon_color, Align::Left);
                let font = if is_lit { fonts.strong } else { fonts.body };
                canvas.text(label_x, centred(canvas, rect, font), label, font, theme.text, Align::Left);
                // The shortcut ends on the same right edge as every % and icon in the devices flyout.
                let shortcut = if *target == Target::Settings { SHORTCUT_SETTINGS } else { SHORTCUT_QUIT };
                canvas.text(width - unit(SIDE, dpi), centred(canvas, rect, fonts.small), shortcut, fonts.small, theme.muted, Align::Right);
            }
        }
    }
}

impl Theme {
    /// Returns the colour for link-like button text: the accent, lifted in dark mode so it reads.
    pub fn accent_text(&self) -> u32 {
        if self.is_high_contrast {
            return self.text;
        }
        if self.is_dark { mix(self.accent, 0xFFFFFF, 0.45) } else { mix(self.accent, 0x000000, 0.15) }
    }
}

/// Builds the tooltip that shows each device row's full name, connection and state on hover.
fn create_row_tooltips(window: HWND, rows: &[DeviceRow], dpi: u32) -> HWND {
    // SAFETY: the tooltip is owned by the flyout and dies with it; every pointer passed outlives its call.
    unsafe {
        let tooltip = CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            std::ptr::null(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
            0,
            0,
            0,
            0,
            window,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        SendMessageW(tooltip, TTM_SETMAXTIPWIDTH, 0, scale(TOOLTIP_WIDTH, dpi) as isize);
        let width = client_width(window);
        for (index, row) in rows.iter().enumerate() {
            let top = row_top(index, dpi);
            let mut text: Vec<u16> = vec![0; row.tooltip.encode_utf16().count() + 1];
            copy_into(&mut text, &row.tooltip);
            let mut info: TTTOOLINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
            info.uFlags = TTF_SUBCLASS;
            info.hwnd = window;
            info.uId = index + 1;
            info.rect = RECT { left: 0, top, right: width, bottom: top + unit(ROW_HEIGHT, dpi) };
            info.lpszText = text.as_mut_ptr();
            SendMessageW(tooltip, TTM_ADDTOOLW, 0, std::ptr::from_ref(&info) as isize);
        }
        tooltip
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

/// Runs what a target does when clicked or picked with Enter. An unknown device tries to detect.
fn activate(target: Target) {
    let rows = FLYOUT.with(|cell| cell.borrow().as_ref().map(|flyout| flyout.rows.clone())).unwrap_or_default();
    match target {
        Target::Settings => choose(MenuAction::OpenSettings),
        Target::Quit => choose(MenuAction::Quit),
        Target::Device(index) if rows[index].end == RowEnd::Actions => choose(MenuAction::Detect(rows[index].id.clone())),
        Target::Device(index) => show_row_menu(index, None),
    }
}

/// Shows the right-click menu of a known device row: Report, for a wrong name or %.
fn show_row_menu(index: usize, at: Option<POINT>) {
    let Some((window, id)) = FLYOUT.with(|cell| cell.borrow().as_ref().map(|flyout| (flyout.window, flyout.rows.get(index).map(|row| row.id.clone())))) else {
        return;
    };
    let Some(id) = id else { return };
    let label = to_wide("Report\u{2026}");
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
    layout(flyout, client_width(flyout.window)).into_iter().map(|(target, _)| target).collect()
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
        WM_LBUTTONUP => {
            let (x, y) = point_of(lparam);
            if let Some(target) = with_flyout(|flyout| hit_test(flyout, x, y)).flatten() {
                // A known device's row does nothing on a left click; an unknown one tries to detect.
                if !matches!(target, Target::Device(_)) || with_flyout(|flyout| matches!(target, Target::Device(index) if flyout.rows[index].end == RowEnd::Actions)) == Some(true) {
                    activate(target);
                }
            }
            0
        }
        WM_RBUTTONUP => {
            let (x, y) = point_of(lparam);
            if let Some(Target::Device(index)) = with_flyout(|flyout| hit_test(flyout, x, y)).flatten() {
                show_row_menu(index, None);
            }
            0
        }
        WM_CONTEXTMENU => {
            if let Some(Some(Target::Device(index))) = with_flyout(|flyout| flyout.focused) {
                let rect = with_flyout(|flyout| layout(flyout, client_width(flyout.window)).into_iter().find(|(target, _)| *target == Target::Device(index)).map(|(_, rect)| rect)).flatten();
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

/// Returns the flyout's position, size and scaling for its rows, next to the anchor.
fn flyout_bounds(anchor: RECT, rows: &[DeviceRow], is_card: bool) -> (i32, i32, i32, i32, u32) {
    // SAFETY: plain lookups with out pointers that outlive the calls.
    unsafe {
        let monitor = MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let dpi = dpi_x.max(96);
        let (_, width) = column_widths(&Canvas::new(1, 1), &Fonts::new(dpi), rows, dpi, is_card);
        let height = scale(flyout_height(rows.len(), menu_points(), is_card), dpi);
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

/// Opens the right-click menu next to the tray icon: Settings and Quit only. A second call closes it.
pub fn show_menu() {
    open_flyout(&[], false, false);
}

/// Opens the devices flyout: the title line and one row per device. Opened by hovering, it never takes
/// focus and gives way to an open menu.
pub fn show_card(devices: &[Device], is_hover: bool) {
    if is_hover && is_menu_open() {
        return;
    }
    open_flyout(devices, true, is_hover);
}

/// Closes the card that hovering opened, once the pointer leaves the icon. A clicked card stays.
pub fn close_hover_card() {
    if with_flyout(|flyout| flyout.is_hover) == Some(true) {
        close_menu();
    }
}

fn open_flyout(devices: &[Device], is_card: bool, is_hover: bool) {
    if is_menu_open() {
        // The same flyout asked for again closes it. A different one replaces it: a click on the icon
        // while its hover card shows, or a right-click for the menu while the battery card is open.
        let is_same = with_flyout(|flyout| flyout.is_card == is_card && !flyout.is_hover) == Some(true);
        close_menu();
        if is_same || is_hover {
            return;
        }
        LAST_CLOSED.with(|cell| *cell.borrow_mut() = None);
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
        let (x, y, width, height, dpi) = flyout_bounds(anchor, &rows, is_card);
        let class_name = to_wide(CLASS_NAME);
        let title = to_wide("Juice Bar");
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | if is_hover { WS_EX_NOACTIVATE } else { 0 },
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
        let tooltip = create_row_tooltips(window, &rows, dpi);
        FLYOUT.with(|cell| {
            *cell.borrow_mut() = Some(Flyout { window, owner, rows, dpi, fonts: Fonts::new(dpi), theme, hovered: None, focused: None, is_tracking_mouse: false, was_active: false, tooltip, is_card, is_hover });
        });
        if is_hover {
            ShowWindow(window, SW_SHOWNOACTIVATE);
        } else {
            ShowWindow(window, SW_SHOW);
            SetForegroundWindow(window);
        }
    }
}

/// Shows new readings in the open devices flyout, resizing it when the rows changed. The right-click menu has no rows to refresh.
pub fn refresh_menu(devices: &[Device]) {
    if with_flyout(|flyout| flyout.is_card) != Some(true) {
        return;
    }
    let rows: Vec<DeviceRow> = devices.iter().map(build_device_row).collect();
    let open = with_flyout(|flyout| {
        flyout.hovered = None;
        flyout.focused = None;
        (flyout.window, flyout.tooltip, flyout.is_card)
    });
    let Some((window, old_tooltip, is_card)) = open else { return };
    // SAFETY: the flyout window is alive while it is stored; the old tooltip is replaced once here.
    unsafe {
        // New names or readings can change the width as well as the height.
        let (x, y, width, height, dpi) = flyout_bounds(anchor_rect(), &rows, is_card);
        SetWindowPos(window, std::ptr::null_mut(), x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        DestroyWindow(old_tooltip);
        let tooltip = create_row_tooltips(window, &rows, dpi);
        with_flyout(|flyout| {
            flyout.rows = rows;
            flyout.tooltip = tooltip;
        });
        InvalidateRect(window, std::ptr::null(), 0);
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
