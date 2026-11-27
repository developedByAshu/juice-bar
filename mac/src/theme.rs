//! The colours Juice Bar draws with. The menu bar icon uses the system tone colours, so it adapts to
//! light, dark and contrast. The settings window uses NetSpeedGauge's palette, so both apps look alike.

use crate::canvas::{self, mix, Align, Font};
use crate::delegate::main_thread;
use juicebar_shared::tone::Tone;
use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSColor, NSColorSpace, NSWindow};

const FALLBACK_ACCENT: u32 = 0x4C9AFF;
const MADE_WITH_LOVE: &str = "Made in India with ";
const ICON_HEART: &str = "heart.fill";
const HEART_COLOR: u32 = 0xE5484D;
const HEART_POINTS: f64 = 8.0;
const HEART_BOX: i32 = 14;

/// Returns the system colour for a tone. Plain is the normal text colour.
pub fn tone_color(tone: Tone) -> Retained<NSColor> {
    match tone {
        Tone::Green => NSColor::systemGreenColor(),
        Tone::Plain => NSColor::labelColor(),
        Tone::Amber => NSColor::systemOrangeColor(),
        Tone::Red => NSColor::systemRedColor(),
    }
}

/// Checks whether the menu bar currently draws in dark mode.
pub fn current_theme_is_dark() -> bool {
    let appearance = NSApplication::sharedApplication(main_thread()).effectiveAppearance();
    appearance.name().to_string().contains("Dark")
}

/// The colours for one theme, the same values as NetSpeedGauge's palette.
#[derive(Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub background: u32,
    pub card: u32,
    pub selected: u32,
    pub segment_track_on_card: u32,
    pub segment_selected: u32,
    pub border: u32,
    pub button: u32,
    pub button_hover: u32,
    pub text: u32,
    pub muted: u32,
    /// The accent: toggles, the selected page marker and the privacy ticks.
    pub accent: u32,
    /// The glass's juice and the reset done cue.
    pub good: u32,
    pub danger: u32,
}

/// Returns the dark colours built around an accent colour.
pub fn dark_palette(accent: u32) -> Theme {
    Theme {
        is_dark: true,
        background: 0x202020,
        card: 0x2B2B2B,
        selected: 0x454545,
        segment_track_on_card: 0x232323,
        segment_selected: 0x454545,
        border: 0x3A3A3A,
        button: 0x3A3A3A,
        button_hover: 0x454545,
        text: 0xF5F5F5,
        muted: 0xA6A6A6,
        accent: mix(accent, 0xFFFFFF, 0.15),
        good: 0x30D158,
        danger: 0xE5484D,
    }
}

/// Returns the light colours built around an accent colour.
pub fn light_palette(accent: u32) -> Theme {
    Theme {
        is_dark: false,
        // A small step between greys is far harder to see near white than near black, so these steps are bigger than dark's.
        background: 0xE8E8E8,
        card: 0xFFFFFF,
        selected: 0xFFFFFF,
        segment_track_on_card: 0xE2E2E2,
        segment_selected: 0xFFFFFF,
        border: 0xD4D4D4,
        button: 0xE2E2E2,
        button_hover: 0xD6D6D6,
        text: 0x1B1B1B,
        muted: 0x5F5F5F,
        accent,
        good: 0x248A3D,
        danger: 0xD93A3F,
    }
}

/// Returns the Mac's accent colour as 0xRRGGBB, or a calm blue if it would be hard to see.
fn accent_color() -> u32 {
    let Some(accent) = NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
        return FALLBACK_ACCENT;
    };
    let channel = |value: f64| (value * 255.0).round() as u32;
    let (red, green, blue) = (channel(accent.redComponent()), channel(accent.greenComponent()), channel(accent.blueComponent()));
    let brightest = red.max(green).max(blue);
    let dullest = red.min(green).min(blue);
    if brightest < 90 || brightest - dullest < 40 {
        return FALLBACK_ACCENT;
    }
    (red << 16) | (green << 8) | blue
}

pub fn dark_theme() -> Theme {
    dark_palette(accent_color())
}

pub fn light_theme() -> Theme {
    light_palette(accent_color())
}

/// Returns the colours for the Mac's current light or dark look.
pub fn current_theme() -> Theme {
    if current_theme_is_dark() {
        dark_theme()
    } else {
        light_theme()
    }
}

/// Draws the "Made in India with love" line, centred.
pub fn draw_made_with_love(center_x: i32, y: i32, font: &Font, color: u32) {
    let text_width = canvas::text_width(MADE_WITH_LOVE, font);
    let left = center_x - (text_width + HEART_BOX) / 2;
    canvas::text(left, y, MADE_WITH_LOVE, font, color, Align::Left);
    canvas::symbol(left + text_width + HEART_BOX / 2, y + font.box_height / 2, ICON_HEART, HEART_POINTS, HEART_COLOR);
}

/// Returns the Mac's own light or dark look for a theme.
pub fn appearance_for(theme: &Theme) -> Option<Retained<NSAppearance>> {
    // SAFETY: these are AppKit's own constants.
    let name = unsafe {
        if theme.is_dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    NSAppearance::appearanceNamed(name)
}

/// Makes a title bar match its window, so the two read as one surface.
pub fn apply_title_bar(window: &NSWindow, theme: &Theme) {
    window.setAppearance(appearance_for(theme).as_deref());
    window.setBackgroundColor(Some(&canvas::color(theme.background)));
    window.setTitlebarAppearsTransparent(true);
}
