//! Light or dark, the accent colour, transparency effects and high contrast, read from Windows
//! and followed live: every window redraws the moment one of them changes, no restart.

use crate::util::to_wide;
use juicebar_shared::tone::Tone;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows_sys::Win32::Graphics::Gdi::{GetSysColor, COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};

const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const FALLBACK_ACCENT: u32 = 0x0067C0;
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
}

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

/// Reads Windows' accent shades (light 3 to dark 3), each as 0xRRGGBB.
fn accent_palette() -> Option<[u32; 7]> {
    let (path, name) = (to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"), to_wide("AccentPalette"));
    let mut bytes = [0u8; 32];
    let mut size = bytes.len() as u32;
    // SAFETY: the strings are null-terminated, and the buffer size passed matches the buffer.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), RRF_RT_REG_BINARY, std::ptr::null_mut(), bytes.as_mut_ptr().cast(), &mut size) };
    if status != 0 || size < 28 {
        return None;
    }
    let mut shades = [0u32; 7];
    for (index, shade) in shades.iter_mut().enumerate() {
        let at = index * 4;
        *shade = (u32::from(bytes[at]) << 16) | (u32::from(bytes[at + 1]) << 8) | u32::from(bytes[at + 2]);
    }
    Some(shades)
}

/// Returns the accent as Windows itself uses it on controls: a light shade in dark mode, a dark one in light mode.
fn accent_color(is_dark: bool) -> u32 {
    if let Some(shades) = accent_palette() {
        return if is_dark { shades[1] } else { shades[4] };
    }
    read_dword(HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\DWM", "AccentColor").map_or(FALLBACK_ACCENT, |stored| {
        ((stored & 0xFF) << 16) | (stored & 0xFF00) | ((stored >> 16) & 0xFF)
    })
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
    if is_dark {
        Theme {
            is_dark,
            is_taskbar_dark,
            is_high_contrast,
            has_backdrop,
            accent,
            background: 0x202020,
            card: 0x2D2D2D,
            hover: 0xFFFFFF,
            border: 0x3D3D3D,
            text: 0xFFFFFF,
            muted: 0xA0A0A0,
            on_accent: 0x000000,
            taskbar_text,
        }
    } else {
        Theme {
            is_dark,
            is_taskbar_dark,
            is_high_contrast,
            has_backdrop,
            accent,
            background: 0xF3F3F3,
            card: 0xFFFFFF,
            hover: 0x000000,
            border: 0xE0E0E0,
            text: 0x1A1A1A,
            muted: 0x5D5D5D,
            on_accent: 0xFFFFFF,
            taskbar_text,
        }
    }
}

/// Returns the user's current look, read fresh from Windows.
pub fn current_theme() -> Theme {
    let is_dark = read_dword(HKEY_CURRENT_USER, PERSONALIZE, "AppsUseLightTheme") != Some(1);
    let is_taskbar_dark = read_dword(HKEY_CURRENT_USER, PERSONALIZE, "SystemUsesLightTheme") != Some(1);
    let transparency_on = read_dword(HKEY_CURRENT_USER, PERSONALIZE, "EnableTransparency") != Some(0);
    let contrast = is_high_contrast();
    let mut theme = build_theme(is_dark, is_taskbar_dark, contrast, transparency_on, windows_build(), accent_color(is_dark));
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
    if backdrop == Backdrop::Flyout {
        let border = ((theme.border & 0xFF) << 16) | (theme.border & 0xFF00) | ((theme.border >> 16) & 0xFF);
        set_attribute(window, DWMWA_BORDER_COLOR, border);
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

/// Returns how strongly a window paints its own colour: a light tint over a backdrop, solid without.
pub fn background_opacity(theme: &Theme) -> f32 {
    if theme.has_backdrop {
        0.0
    } else {
        1.0
    }
}
