//! Light or dark, the accent colour, transparency effects and high contrast, read from Windows
//! and followed live: every window redraws the moment one of them changes, no restart.

use crate::canvas::{mix, Align, Canvas};
use crate::util::to_wide;
use juicebar_shared::tone::Tone;
use windows_sys::Win32::Graphics::Gdi::HFONT;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows_sys::Win32::Graphics::Gdi::{GetSysColor, COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};

const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const FALLBACK_ACCENT: u32 = 0x4C9AFF;
const MADE_WITH_LOVE: &str = "Made in India with ";
const ICON_HEART: &str = "\u{EB52}";
const HEART_COLOR: u32 = 0xE5484D;
const HEART_DROP: i32 = 3;

/// The fonts NetSpeedGauge draws with, so both apps read the same.
pub const UI_FONT: &str = "Segoe UI";
pub const UI_FONT_STRONG: &str = "Segoe UI Semibold";
pub const ICON_FONT: &str = "Segoe MDL2 Assets";
// Windows 11 build 22621 (22H2) is the first that draws Mica and Acrylic behind any window.
const FIRST_BACKDROP_BUILD: u32 = 22621;

/// The tone colours, the same sRGB values on Mac and Windows (CONTRACT.md).
pub const GREEN: u32 = 0x34C759;
pub const AMBER: u32 = 0xFF9F0A;
pub const RED: u32 = 0xFF453A;

/// Everything a window needs to draw itself in the user's current Windows look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Apps are dark (Settings, Personalisation, Colours, "Choose your app mode").
    pub is_dark: bool,
    /// The taskbar is dark ("Choose your default Windows mode"); the tray glyph follows this one.
    pub is_taskbar_dark: bool,
    pub is_high_contrast: bool,
    /// Mica and Acrylic show: transparency effects are on, contrast is normal and Windows can draw them.
    pub has_backdrop: bool,
    pub accent: u32,
    /// Window colour, used solid when there is no backdrop and as a light tint over it.
    pub background: u32,
    /// Raised surfaces: cards in Settings, the confirm card.
    pub card: u32,
    /// A row under the pointer or keyboard.
    pub hover: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    /// Text on the accent colour, for the main button.
    pub on_accent: u32,
    /// The text colour of the taskbar, for the plain glyph.
    pub taskbar_text: u32,
    /// The chosen page in the sidebar, as NetSpeedGauge draws it.
    pub selected: u32,
    /// Plain buttons, such as Reset and the step buttons.
    pub button: u32,
    pub button_hover: u32,
    /// The flyout's own colour, a step above the window.
    pub surface: u32,
    /// The accent as NetSpeedGauge shows it: switches that are on and the sidebar marker.
    pub highlight: u32,
}

static THEME_CHOICE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static LISTENERS: RefCell<Vec<Box<dyn Fn()>>> = RefCell::new(Vec::new());
}

/// Reads a whole-number setting from the registry.
fn read_dword(root: windows_sys::Win32::System::Registry::HKEY, path: &str, name: &str) -> Option<u32> {
    let (path, name) = (to_wide(path), to_wide(name));
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: the strings are null-terminated, and `value` and `size` are valid out pointers.
    let status = unsafe { RegGetValueW(root, path.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), std::ptr::from_mut(&mut value).cast(), &mut size) };
    (status == 0).then_some(value)
}

/// Reads a text setting from the registry.
fn read_text(root: windows_sys::Win32::System::Registry::HKEY, path: &str, name: &str) -> Option<String> {
    let (path, name) = (to_wide(path), to_wide(name));
    let mut buffer = [0u16; 64];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: the strings are null-terminated, and the buffer size passed matches the buffer.
    let status = unsafe { RegGetValueW(root, path.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size) };
    (status == 0).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
}

/// Returns the Windows build number, such as 26100.
pub fn windows_build() -> u32 {
    read_text(HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentBuildNumber").and_then(|text| text.parse().ok()).unwrap_or(0)
}

/// Returns the user's accent read the way NetSpeedGauge reads it, or a calm blue if it would be hard to see.
fn accent_color() -> u32 {
    let Some(stored) = read_dword(HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\DWM", "AccentColor") else {
        return FALLBACK_ACCENT;
    };
    let (red, green, blue) = (stored & 0xFF, (stored >> 8) & 0xFF, (stored >> 16) & 0xFF);
    let brightest = red.max(green).max(blue);
    let dullest = red.min(green).min(blue);
    if brightest < 90 || brightest - dullest < 40 {
        return FALLBACK_ACCENT;
    }
    (red << 16) | (green << 8) | blue
}

fn is_high_contrast() -> bool {
    let mut contrast: HIGHCONTRASTW = unsafe { std::mem::zeroed() };
    contrast.cbSize = std::mem::size_of::<HIGHCONTRASTW>() as u32;
    // SAFETY: `contrast` is sized as this query expects and outlives the call.
    let found = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, contrast.cbSize, std::ptr::from_mut(&mut contrast).cast(), 0) };
    found != 0 && contrast.dwFlags & HCF_HIGHCONTRASTON != 0
}

fn system_color(index: windows_sys::Win32::Graphics::Gdi::SYS_COLOR_INDEX) -> u32 {
    // SAFETY: plain lookup of a system colour.
    let bgr = unsafe { GetSysColor(index) };
    ((bgr & 0xFF) << 16) | (bgr & 0xFF00) | ((bgr >> 16) & 0xFF)
}

/// Returns the font family for all text: Segoe UI Variable on Windows 11, Segoe UI before it.
pub fn font_family() -> &'static str {
    let fonts = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
    if read_text(HKEY_LOCAL_MACHINE, fonts, "Segoe UI Variable (TrueType)").is_some() {
        "Segoe UI Variable Text"
    } else {
        "Segoe UI"
    }
}

/// Returns the font for icons: Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets before it.
pub fn icon_font_family() -> &'static str {
    let fonts = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
    if read_text(HKEY_LOCAL_MACHINE, fonts, "Segoe Fluent Icons (TrueType)").is_some() {
        "Segoe Fluent Icons"
    } else {
        "Segoe MDL2 Assets"
    }
}

/// Builds the theme from what Windows reports. Kept apart from the reads so tests can call it.
pub fn build_theme(is_dark: bool, is_taskbar_dark: bool, is_high_contrast: bool, transparency_on: bool, build: u32, accent: u32) -> Theme {
    let has_backdrop = transparency_on && !is_high_contrast && build >= FIRST_BACKDROP_BUILD;
    let taskbar_text = if is_taskbar_dark { 0xFFFFFF } else { 0x000000 };
    // The same colours as NetSpeedGauge's palette (NetSpeedGauge shared/src/palette.rs), so the apps match.
    if is_dark {
        Theme {
            is_dark,
            is_taskbar_dark,
            is_high_contrast,
            has_backdrop,
            accent,
            background: 0x202020,
            card: 0x2B2B2B,
            hover: 0xFFFFFF,
            border: 0x3A3A3A,
            text: 0xF5F5F5,
            muted: 0xA6A6A6,
            on_accent: 0x1B1B1B,
            taskbar_text,
            selected: 0x454545,
            button: 0x3A3A3A,
            button_hover: 0x454545,
            surface: 0x323232,
            highlight: mix(accent, 0xFFFFFF, 0.15),
        }
    } else {
        Theme {
            is_dark,
            is_taskbar_dark,
            is_high_contrast,
            has_backdrop,
            accent,
            background: 0xE8E8E8,
            card: 0xFFFFFF,
            hover: 0x000000,
            border: 0xD4D4D4,
            text: 0x1B1B1B,
            muted: 0x5F5F5F,
            on_accent: 0xFFFFFF,
            taskbar_text,
            selected: 0xFFFFFF,
            button: 0xE2E2E2,
            button_hover: 0xD6D6D6,
            surface: 0xFFFFFF,
            highlight: accent,
        }
    }
}

/// Sets the user's choice for Juice Bar's windows, as NetSpeedGauge's: 0 follows Windows, 1 is light, 2 is dark.
pub fn set_theme_choice(choice: u8) {
    THEME_CHOICE.store(choice, Ordering::Relaxed);
}

/// Returns true when Juice Bar's windows are dark: the user's choice, or Windows' app mode when it follows Windows.
fn is_window_dark() -> bool {
    match THEME_CHOICE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => read_dword(HKEY_CURRENT_USER, PERSONALIZE, "AppsUseLightTheme") != Some(1),
    }
}

/**
 * Returns the user's current look, read fresh from Windows. The window theme choice sets light or
 * dark for Juice Bar's own windows; the tray glyph keeps following the taskbar.
 */
pub fn current_theme() -> Theme {
    let is_dark = is_window_dark();
    let is_taskbar_dark = read_dword(HKEY_CURRENT_USER, PERSONALIZE, "SystemUsesLightTheme") != Some(1);
    let transparency_on = read_dword(HKEY_CURRENT_USER, PERSONALIZE, "EnableTransparency") != Some(0);
    let contrast = is_high_contrast();
    let mut theme = build_theme(is_dark, is_taskbar_dark, contrast, transparency_on, windows_build(), accent_color());
    // Solid colours, as NetSpeedGauge paints its windows, so the two apps look the same.
    theme.has_backdrop = false;
    if contrast {
        // High contrast: Windows' own colours only, so the user's chosen scheme is honoured exactly.
        theme.background = system_color(COLOR_WINDOW);
        theme.card = system_color(COLOR_BTNFACE);
        theme.text = system_color(COLOR_WINDOWTEXT);
        theme.muted = system_color(COLOR_GRAYTEXT);
        theme.border = system_color(COLOR_WINDOWTEXT);
        theme.hover = system_color(COLOR_HIGHLIGHT);
        theme.accent = system_color(COLOR_HIGHLIGHT);
        theme.on_accent = system_color(COLOR_HIGHLIGHTTEXT);
        theme.taskbar_text = system_color(COLOR_WINDOWTEXT);
        theme.selected = system_color(COLOR_HIGHLIGHT);
        theme.button = system_color(COLOR_BTNFACE);
        theme.button_hover = system_color(COLOR_HIGHLIGHT);
        theme.surface = system_color(COLOR_WINDOW);
        theme.highlight = system_color(COLOR_HIGHLIGHT);
        theme.is_dark = (theme.background & 0xFF) < 0x80;
    }
    theme
}

/// Returns the colour for a tone on text or a dot. Plain is the window's text colour.
pub fn tone_color(tone: Tone, theme: &Theme) -> u32 {
    match tone {
        Tone::Green => GREEN,
        Tone::Amber => AMBER,
        Tone::Red => RED,
        Tone::Plain => theme.text,
    }
}

/// Registers a function to run whenever the look changes. Every window registers one redraw.
pub fn on_theme_change(callback: impl Fn() + 'static) {
    LISTENERS.with(|listeners| listeners.borrow_mut().push(Box::new(callback)));
}

/// Tells every listener the look changed. The main window calls this on Windows' change messages.
pub fn notify_theme_change() {
    // Taken out while running, so a listener that registers another cannot hit a busy list.
    let listeners = LISTENERS.with(|listeners| std::mem::take(&mut *listeners.borrow_mut()));
    for listener in &listeners {
        listener();
    }
    LISTENERS.with(|stored| {
        let mut stored = stored.borrow_mut();
        let added = std::mem::take(&mut *stored);
        *stored = listeners;
        stored.extend(added);
    });
}

fn set_attribute(window: HWND, attribute: i32, value: u32) {
    // SAFETY: `value` outlives the call. Older Windows ignores attributes it does not know.
    unsafe { DwmSetWindowAttribute(window, attribute as u32, std::ptr::from_ref(&value).cast(), std::mem::size_of::<u32>() as u32) };
}

/// Which backdrop a window wants when the user allows one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    /// Mica, for the settings window.
    Window,
    /// Acrylic, for the flyout, as Windows' own tray flyouts use.
    Flyout,
}

/**
 * Gives a window its Windows 11 frame: rounded corners, dark or light title, and Mica or Acrylic
 * behind see-through pixels. Without a backdrop the window paints its colour solid instead.
 */
pub fn apply_window_look(window: HWND, theme: &Theme, backdrop: Backdrop) {
    set_attribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(theme.is_dark));
    set_attribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND as u32);
    let windows_color = |color: u32| ((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF);
    if backdrop == Backdrop::Flyout {
        set_attribute(window, DWMWA_BORDER_COLOR, windows_color(theme.border));
    } else {
        // The title bar takes the window's colour, as NetSpeedGauge's does, so the two read as one surface.
        set_attribute(window, DWMWA_CAPTION_COLOR, windows_color(theme.background));
        set_attribute(window, DWMWA_TEXT_COLOR, windows_color(theme.text));
    }
    let kind = match (theme.has_backdrop, backdrop) {
        (false, _) => DWMSBT_NONE,
        (true, Backdrop::Window) => DWMSBT_MAINWINDOW,
        (true, Backdrop::Flyout) => DWMSBT_TRANSIENTWINDOW,
    };
    set_attribute(window, DWMWA_SYSTEMBACKDROP_TYPE, kind as u32);
    let extend = if theme.has_backdrop { -1 } else { 0 };
    let margins = MARGINS { cxLeftWidth: extend, cxRightWidth: extend, cyTopHeight: extend, cyBottomHeight: extend };
    // SAFETY: `margins` outlives the call.
    unsafe { DwmExtendFrameIntoClientArea(window, &margins) };
}

/// Draws the "Made in India with love" line, centred, with a real heart icon. Copied from NetSpeedGauge.
pub fn draw_made_with_love(canvas: &mut Canvas, center_x: i32, y: i32, font: HFONT, icon_font: HFONT, color: u32) {
    let text_width = canvas.text_width(MADE_WITH_LOVE, font);
    let heart_width = canvas.text_width(ICON_HEART, icon_font);
    let left = center_x - (text_width + heart_width) / 2;
    canvas.text(left, y, MADE_WITH_LOVE, font, color, Align::Left);
    canvas.text(left + text_width, y + HEART_DROP, ICON_HEART, icon_font, HEART_COLOR, Align::Left);
}

/// Returns how strongly a window paints its own colour: a light tint over a backdrop, solid without.
pub fn background_opacity(theme: &Theme) -> f32 {
    if theme.has_backdrop {
        0.0
    } else {
        1.0
    }
}
