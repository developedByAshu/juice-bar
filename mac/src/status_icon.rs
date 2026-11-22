//! The menu bar item: an upright battery (unlike the Mac's own sideways one), filled to the lowest
//! device's level and coloured by its tone. Plain tone draws as a template image, so it follows the
//! menu bar's light, dark and tinted looks for free.

use crate::delegate::main_thread;
use crate::theme::tone_color;
use block2::RcBlock;
use juicebar_shared::tone::Tone;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSBezierPath, NSColor, NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const ICON_SIZE: NSSize = NSSize { width: 12.0, height: 18.0 };
const BODY_WIDTH: f64 = 9.0;
const BODY_HEIGHT: f64 = 14.0;
const CAP_WIDTH: f64 = 4.0;
const CAP_HEIGHT: f64 = 1.6;
const LINE_WIDTH: f64 = 1.2;
const CORNER: f64 = 2.2;
const INSET: f64 = 1.9;

/// Creates the menu bar item with an empty battery until the first reading.
pub fn create_status_icon() -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(main_thread()) {
        button.setToolTip(Some(&NSString::from_str("Juice Bar")));
    }
    update_status_icon(&item, Tone::Plain, None, false);
    item
}

/// Redraws the battery for the lowest level, and shows that % next to it when asked.
pub fn update_status_icon(item: &NSStatusItem, tone: Tone, lowest_percent: Option<u8>, shows_percent: bool) {
    let Some(button) = item.button(main_thread()) else {
        return;
    };
    button.setImage(Some(&battery_image(tone, lowest_percent)));
    let title = match (shows_percent, lowest_percent) {
        (true, Some(percent)) => format!(" {percent}%"),
        _ => String::new(),
    };
    button.setTitle(&NSString::from_str(&title));
    let label = lowest_percent.map_or_else(|| "Juice Bar".to_string(), |percent| format!("Juice Bar, lowest battery {percent}%"));
    button.setToolTip(Some(&NSString::from_str(&label)));
}

/// Draws the battery glyph. The drawing runs at display time, so system colours resolve for the current look.
fn battery_image(tone: Tone, lowest_percent: Option<u8>) -> Retained<NSImage> {
    let fill_fraction = lowest_percent.map_or(0.0, |percent| (f64::from(percent) / 100.0).clamp(0.12, 1.0));
    let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
        let left = (ICON_SIZE.width - BODY_WIDTH) / 2.0;
        let bottom = 1.0;
        let body = NSRect::new(NSPoint::new(left, bottom), NSSize::new(BODY_WIDTH, BODY_HEIGHT));
        let cap = NSRect::new(NSPoint::new((ICON_SIZE.width - CAP_WIDTH) / 2.0, bottom + BODY_HEIGHT + 0.4), NSSize::new(CAP_WIDTH, CAP_HEIGHT));
        let outline_color = if tone == Tone::Plain { NSColor::blackColor() } else { NSColor::labelColor() };
        outline_color.setStroke();
        outline_color.setFill();
        let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(body, CORNER, CORNER);
        outline.setLineWidth(LINE_WIDTH);
        outline.stroke();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(cap, 0.8, 0.8).fill();
        if fill_fraction > 0.0 {
            let inner_height = (BODY_HEIGHT - INSET * 2.0) * fill_fraction;
            let inner = NSRect::new(NSPoint::new(left + INSET, bottom + INSET), NSSize::new(BODY_WIDTH - INSET * 2.0, inner_height));
            let fill_color = if tone == Tone::Plain { NSColor::blackColor() } else { tone_color(tone) };
            fill_color.setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inner, 1.0, 1.0).fill();
        }
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(ICON_SIZE, false, &handler);
    image.setTemplate(tone == Tone::Plain);
    image
}
