//! The tray icon: the battery glyph in the tone of the lowest device, or that device's % in the
//! same tone when the user asks for the number.

use crate::canvas::{create_see_through_font, Align, Canvas};
use crate::glyph::glass_image;
use crate::messages::WM_APP_TRAY;
use crate::theme::{current_theme, font_family, tone_color, Theme};
use crate::util::{copy_into, to_wide};
use juicebar_shared::tone::Tone;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETFOCUS, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, NOTIFYICON_VERSION_4,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, FindWindowW, HICON, ICONINFO, SM_CXSMICON};

const ICON_ID: u32 = 1;
const BOLD: i32 = 600;

/// What the icon shows now, kept so it can be drawn again when the taskbar or scaling changes.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    tone: Tone,
    lowest_percent: Option<u8>,
    shows_percent: bool,
}

struct TrayState {
    window: HWND,
    icon: HICON,
    shown: Shown,
    tooltip: String,
}

thread_local! {
    static STATE: RefCell<Option<TrayState>> = const { RefCell::new(None) };
}

/// Returns the juice colour: the tone, or the taskbar's own text colour when plain.
pub fn glyph_color(tone: Tone, theme: &Theme) -> u32 {
    match tone {
        Tone::Plain => theme.taskbar_text,
        other => tone_color(other, theme),
    }
}

/// Returns the pixel size of a tray icon at the taskbar's scaling.
fn icon_size() -> (u32, u32) {
    let class = to_wide("Shell_TrayWnd");
    // SAFETY: plain lookups; a missing taskbar falls back to 100% scaling.
    unsafe {
        let taskbar = FindWindowW(class.as_ptr(), std::ptr::null());
        let dpi = if taskbar.is_null() { 96 } else { GetDpiForWindow(taskbar).max(96) };
        (GetSystemMetricsForDpi(SM_CXSMICON, dpi).max(16) as u32, dpi)
    }
}

/// Draws the icon's pixels, premultiplied, for a size.
pub fn icon_pixels(size: u32, dpi: u32, shown: (Tone, Option<u8>, bool), theme: &Theme) -> Vec<u32> {
    let (tone, lowest_percent, shows_percent) = shown;
    let color = glyph_color(tone, theme);
    match (shows_percent, lowest_percent) {
        (true, Some(percent)) => percent_pixels(size, dpi, percent, color),
        _ => glass_image(size, theme.taskbar_text, color, lowest_percent),
    }
}

/// Draws the lowest % as a bold number filling the icon.
fn percent_pixels(size: u32, dpi: u32, percent: u8, color: u32) -> Vec<u32> {
    let mut canvas = Canvas::new(size as i32, size as i32);
    let text = percent.to_string();
    let mut points = size as f32 * 0.78 * 72.0 / dpi as f32;
    loop {
        let font = create_see_through_font(font_family(), points, BOLD, dpi);
        let (width, height) = canvas.text_size(&text, font);
        if width <= size as i32 || points < 5.0 {
            canvas.see_through_text(size as i32 / 2, (size as i32 - height) / 2, &text, font, color, Align::Center);
            // SAFETY: the font was created above and is not used again.
            unsafe { DeleteObject(font) };
            break;
        }
        // SAFETY: as above.
        unsafe { DeleteObject(font) };
        points -= 0.5;
    }
    canvas.snapshot()
}

/// Makes a Windows icon from premultiplied pixels. Icons take straight alpha, so it is undone here.
fn make_icon(size: u32, pixels: &[u32]) -> HICON {
    // SAFETY: the bitmaps are created, filled within their bounds, handed to CreateIconIndirect, and freed.
    unsafe {
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size as i32;
        info.bmiHeader.biHeight = -(size as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(std::ptr::null_mut(), &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        let target = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (size * size) as usize);
        for (out, &pixel) in target.iter_mut().zip(pixels) {
            *out = unpremultiply(pixel);
        }
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, std::ptr::null());
        let icon_info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

/// Turns a premultiplied pixel into straight alpha.
pub fn unpremultiply(pixel: u32) -> u32 {
    let alpha = pixel >> 24;
    if alpha == 0 {
        return 0;
    }
    let channel = |shift: u32| ((((pixel >> shift) & 0xFF) * 255 + alpha / 2) / alpha).min(255) << shift;
    (alpha << 24) | channel(16) | channel(8) | channel(0)
}

fn notify_data(window: HWND) -> NOTIFYICONDATAW {
    // SAFETY: an all-zero NOTIFYICONDATAW is a valid empty value.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = ICON_ID;
    data
}

/// Draws the icon for what it shows now and hands it to the tray.
fn push(state: &mut TrayState, command: u32) {
    let (size, dpi) = icon_size();
    let shown = state.shown;
    let pixels = icon_pixels(size, dpi, (shown.tone, shown.lowest_percent, shown.shows_percent), &current_theme());
    let icon = make_icon(size, &pixels);
    let mut data = notify_data(state.window);
    // No NIF_SHOWTIP: Windows shows no tooltip of its own, so resting on the icon shows nothing unless
    // the battery card is chosen. The text still goes in, for screen readers.
    data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
    data.uCallbackMessage = WM_APP_TRAY;
    data.hIcon = icon;
    copy_into(&mut data.szTip, &state.tooltip);
    // SAFETY: `data` is filled for this call. The old icon is freed only after the tray took the new one.
    unsafe {
        Shell_NotifyIconW(command, &data);
        if command == NIM_ADD {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
        if !state.icon.is_null() {
            DestroyIcon(state.icon);
        }
    }
    state.icon = icon;
}

/// Puts the icon in the tray, plain and empty until the first scan. The window gets its clicks.
pub fn create_status_icon(window: HWND) {
    let mut state = TrayState {
        window,
        icon: std::ptr::null_mut(),
        shown: Shown { tone: Tone::Plain, lowest_percent: None, shows_percent: false },
        tooltip: "Juice Bar".to_string(),
    };
    push(&mut state, NIM_ADD);
    STATE.with(|cell| *cell.borrow_mut() = Some(state));
}

/// Shows a tone and, when asked, the lowest %. Nothing is redrawn when nothing changed.
pub fn update_status_icon(tone: Tone, lowest_percent: Option<u8>, shows_percent: bool) {
    let shown = Shown { tone, lowest_percent, shows_percent };
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if state.shown != shown {
                state.shown = shown;
                push(state, NIM_MODIFY);
            }
        }
    });
}

/// Sets the text shown when the pointer rests on the icon.
pub fn set_tooltip(text: &str) {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            if state.tooltip != text {
                state.tooltip = text.to_string();
                push(state, NIM_MODIFY);
            }
        }
    });
}

/// Draws the icon again, for a taskbar colour or scaling change.
pub fn redraw_status_icon() {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            push(state, NIM_MODIFY);
        }
    });
}

/// Puts the icon back after Explorer restarted, which empties the tray.
pub fn restore_status_icon() {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            push(state, NIM_ADD);
        }
    });
}

/// Hands keyboard focus back to the icon, as Windows asks after a flyout closes from the keyboard.
pub fn focus_status_icon() {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            let data = notify_data(state.window);
            // SAFETY: `data` names our icon.
            unsafe { Shell_NotifyIconW(NIM_SETFOCUS, &data) };
        }
    });
}

/// Returns where the icon sits on screen, for placing the flyout next to it.
pub fn status_icon_rect() -> Option<RECT> {
    STATE.with(|cell| {
        let state = cell.borrow();
        let state = state.as_ref()?;
        // SAFETY: an all-zero identifier is valid; the fields set name our icon.
        let mut identifier: NOTIFYICONIDENTIFIER = unsafe { std::mem::zeroed() };
        identifier.cbSize = std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32;
        identifier.hWnd = state.window;
        identifier.uID = ICON_ID;
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: both structures outlive the call.
        let status = unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) };
        (status == 0).then_some(rect)
    })
}

/// Takes the icon out of the tray, on quit.
pub fn remove_status_icon() {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().take() {
            let data = notify_data(state.window);
            // SAFETY: `data` names our icon; the icon handle is freed once here.
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &data);
                if !state.icon.is_null() {
                    DestroyIcon(state.icon);
                }
            }
        }
    });
}
