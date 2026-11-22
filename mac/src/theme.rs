//! The colours Juice Bar draws with. Menus and windows follow the system appearance on their own;
//! only the tone colours are ours, and they are the system ones, so they adapt to light, dark and contrast.

use juicebar_shared::tone::Tone;
use objc2::rc::Retained;
use objc2_app_kit::NSColor;

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
    let appearance = objc2_app_kit::NSApplication::sharedApplication(crate::delegate::main_thread()).effectiveAppearance();
    let name = appearance.name().to_string();
    name.contains("Dark")
}
