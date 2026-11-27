//! Draws into the view being painted, the same way NetSpeedGauge does. Sizes match the Windows app's
//! 100% scale numbers, and AppKit handles Retina.

use juicebar_shared::glyph::{juice_polygon, BOX, GLASS, LINE_WIDTH, STRAW};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread, ClassType};
use objc2_app_kit::{
    NSBezierPath, NSBitmapImageRep, NSColor, NSCompositingOperation, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSImageSymbolConfiguration,
    NSLineCapStyle, NSLineJoinStyle,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSPoint, NSRect, NSSize, NSString};

pub const WEIGHT_REGULAR: f64 = 0.0;
pub const WEIGHT_MEDIUM: f64 = 0.23;
pub const WEIGHT_SEMIBOLD: f64 = 0.3;

// The Windows numbers are in points at 96 dots per inch, so they draw this much larger on a Mac.
const POINT_SCALE: f64 = 96.0 / 72.0;
// The line height the shared layout numbers were tuned for, as a share of the font size.
const LAYOUT_LINE_HEIGHT: f64 = 1.33;

/// Where a text position sits relative to the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A font, with how far to move its text so it sits where the layout expects.
pub struct Font {
    inner: Retained<NSFont>,
    pub box_height: i32,
    nudge: f64,
}

impl Font {
    fn wrap(inner: Retained<NSFont>, size: f64) -> Self {
        let line_height = inner.ascender() - inner.descender() + inner.leading();
        let layout_height = size * LAYOUT_LINE_HEIGHT;
        Self { inner, box_height: layout_height.round() as i32, nudge: (layout_height - line_height) / 2.0 }
    }

    /// Returns the system font at a size in Windows points.
    pub fn system(points: f64, weight: f64) -> Self {
        let size = points * POINT_SCALE;
        Self::wrap(NSFont::systemFontOfSize_weight(size, weight), size)
    }

    /// Returns the system font with digits all one width, so a column of numbers lines up.
    pub fn digits(points: f64, weight: f64) -> Self {
        let size = points * POINT_SCALE;
        Self::wrap(NSFont::monospacedDigitSystemFontOfSize_weight(size, weight), size)
    }

    /// Returns the AppKit font, for a real control that must match the drawn text.
    pub fn ns_font(&self) -> &NSFont {
        &self.inner
    }
}

/// Blends two 0xRRGGBB colours. An `amount` of 0 gives `from`, and 1 gives `to`.
pub fn mix(from: u32, to: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let start = ((from >> shift) & 0xFF) as f32;
        let end = ((to >> shift) & 0xFF) as f32;
        ((start + (end - start) * amount).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Converts 0xRRGGBB to a colour.
pub fn color(rgb: u32) -> Retained<NSColor> {
    let channel = |shift: u32| f64::from((rgb >> shift) & 0xFF) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(channel(16), channel(8), channel(0), 1.0)
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> NSRect {
    NSRect::new(NSPoint::new(f64::from(x), f64::from(y)), NSSize::new(f64::from(width), f64::from(height)))
}

fn fill_path(x: i32, y: i32, width: i32, height: i32, radius: f32, fill: &NSColor) {
    let radius = f64::from(radius).min(f64::from(width) / 2.0).min(f64::from(height) / 2.0).max(0.0);
    fill.set();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(x, y, width, height), radius, radius).fill();
}

/// Fills a rounded rectangle.
pub fn round_rect(x: i32, y: i32, width: i32, height: i32, radius: f32, rgb: u32) {
    fill_path(x, y, width, height, radius, &color(rgb));
}

/// Fills a rounded rectangle part way see-through, greying out what is under it.
pub fn fade_round_rect(x: i32, y: i32, width: i32, height: i32, radius: f32, rgb: u32, amount: f32) {
    fill_path(x, y, width, height, radius, &color(rgb).colorWithAlphaComponent(f64::from(amount)));
}

fn attributes(font: &Font, rgb: u32) -> Retained<NSDictionary<AnyObject, AnyObject>> {
    let fill = color(rgb);
    // SAFETY: the keys are AppKit's own constants, and every object outlives the call that copies them.
    unsafe {
        let keys: [*const AnyObject; 2] = [std::ptr::from_ref(NSFontAttributeName).cast(), std::ptr::from_ref(NSForegroundColorAttributeName).cast()];
        let values: [*const AnyObject; 2] = [std::ptr::from_ref(&*font.inner).cast(), std::ptr::from_ref(&*fill).cast()];
        msg_send![
            NSDictionary::<AnyObject, AnyObject>::class(),
            dictionaryWithObjects: values.as_ptr(),
            forKeys: keys.as_ptr(),
            count: 2usize
        ]
    }
}

/// Returns a text in a font and colour, for a real control's hint.
pub fn attributed(text: &str, font: &Font, rgb: u32) -> Retained<NSAttributedString> {
    let string = NSString::from_str(text);
    let attributes = attributes(font, rgb);
    // SAFETY: initWithString:attributes: is the designated initialiser, and both objects are valid.
    unsafe { msg_send![NSAttributedString::alloc(), initWithString: &*string, attributes: &*attributes] }
}

/// Returns how wide a text draws.
pub fn text_width(text: &str, font: &Font) -> i32 {
    let string = NSString::from_str(text);
    let attributes = attributes(font, 0);
    // SAFETY: both objects are valid for the call.
    let size: NSSize = unsafe { msg_send![&*string, sizeWithAttributes: &*attributes] };
    size.width.ceil() as i32
}

/// Draws one line of text. `y` is the top of the line, and `x` the point `align` names.
pub fn text(x: i32, y: i32, text: &str, font: &Font, rgb: u32, align: Align) {
    let string = NSString::from_str(text);
    let attributes = attributes(font, rgb);
    // SAFETY: both objects are valid for the call.
    let size: NSSize = unsafe { msg_send![&*string, sizeWithAttributes: &*attributes] };
    let left = match align {
        Align::Left => f64::from(x),
        Align::Center => f64::from(x) - size.width / 2.0,
        Align::Right => f64::from(x) - size.width,
    };
    let point = NSPoint::new(left, f64::from(y) + font.nudge);
    // SAFETY: both objects are valid, and this only runs while a view is being painted.
    let _: () = unsafe { msg_send![&*string, drawAtPoint: point, withAttributes: &*attributes] };
}

fn symbol_image(name: &str, points: f64, rgb: u32) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
    let fill = color(rgb);
    let sized = NSImageSymbolConfiguration::configurationWithPointSize_weight(points * POINT_SCALE, WEIGHT_REGULAR);
    let tinted = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&fill);
    let config = sized.configurationByApplyingConfiguration(&tinted);
    image.imageWithSymbolConfiguration(&config)
}

/// Returns how wide an SF Symbol draws at a size in Windows points.
pub fn symbol_width(name: &str, points: f64) -> i32 {
    symbol_image(name, points, 0).map_or(0, |image| image.size().width.round() as i32)
}

/// Draws one line of text whose ink, not its advance, ends exactly at `right`, so a column of
/// different glyphs (a %, a Q, a comma) lines up to the pixel. `y` is the top of the line.
pub fn text_ink_right(right: i32, y: i32, text: &str, font: &Font, rgb: u32) {
    let string = NSString::from_str(text);
    let attributes = attributes(font, rgb);
    // Line fragment origin plus device metrics: the bounds of the drawn glyphs themselves.
    let options: usize = 1 | 8;
    let huge = NSSize::new(10_000.0, 10_000.0);
    // SAFETY: both objects are valid, and a nil context is allowed.
    let ink: NSRect = unsafe { msg_send![&*string, boundingRectWithSize: huge, options: options, attributes: &*attributes, context: std::ptr::null::<AnyObject>()] };
    let left = f64::from(right) - (ink.origin.x + ink.size.width);
    let point = NSPoint::new(left, f64::from(y) + font.nudge);
    // SAFETY: both objects are valid, and this only runs while a view is being painted.
    let _: () = unsafe { msg_send![&*string, drawAtPoint: point, withAttributes: &*attributes] };
}

thread_local! {
    static CLEAR_SPACE: std::cell::RefCell<std::collections::HashMap<String, f64>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Returns the clear space, in points, between a symbol's last inked column and its image's right side.
/// A symbol's image pads the glyph a little, and its alignment box includes that padding.
fn clear_space_right(image: &NSImage) -> f64 {
    let size = image.size();
    let key = format!("{:?}{}x{}", image.name(), size.width, size.height);
    if let Some(space) = CLEAR_SPACE.with(|cache| cache.borrow().get(&key).copied()) {
        return space;
    }
    let space = image
        .TIFFRepresentation()
        .and_then(|data| NSBitmapImageRep::imageRepWithData(&data))
        .and_then(|bitmap| {
            let (wide, high) = (bitmap.pixelsWide(), bitmap.pixelsHigh());
            let inked = (0..wide).rev().find(|&x| (0..high).any(|y| bitmap.colorAtX_y(x, y).is_some_and(|pixel| pixel.alphaComponent() > 0.3)))?;
            Some((wide - 1 - inked) as f64 * size.width / wide as f64)
        })
        .unwrap_or(0.0);
    CLEAR_SPACE.with(|cache| cache.borrow_mut().insert(key, space));
    space
}

/// Draws an SF Symbol whose right side is at `right`, centred on `center_y`.
pub fn symbol_right(right: i32, center_y: i32, name: &str, points: f64, rgb: u32) {
    let Some(image) = symbol_image(name, points, rgb) else {
        return;
    };
    let size = image.size();
    let glyph_right = size.width - clear_space_right(&image);
    let origin = NSPoint::new(f64::from(right) - glyph_right, f64::from(center_y) - size.height / 2.0);
    // SAFETY: this only runs while a view is being painted, and no drawing hints are passed.
    unsafe {
        image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(NSRect::new(origin, size), NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0, true, None);
    }
}

/// Draws an SF Symbol centred on a point, at a size in Windows points.
pub fn symbol(center_x: i32, center_y: i32, name: &str, points: f64, rgb: u32) {
    let Some(image) = symbol_image(name, points, rgb) else {
        return;
    };
    let size = image.size();
    let origin = NSPoint::new(f64::from(center_x) - size.width / 2.0, f64::from(center_y) - size.height / 2.0);
    // SAFETY: this only runs while a view is being painted, and no drawing hints are passed.
    unsafe {
        image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(NSRect::new(origin, size), NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0, true, None);
    }
}

/// Draws the Juice Bar glass from the shared geometry, `height` points tall with its top left at (x, y).
pub fn glass(x: i32, y: i32, height: i32, ink: u32, juice: u32, percent: Option<u8>) {
    let scale = f64::from(height) / BOX.1;
    // The glyph box has y going up and this view has y going down, so each point flips.
    let path_of = |points: &[(f64, f64)], is_closed: bool| {
        let path = NSBezierPath::bezierPath();
        for (index, (point_x, point_y)) in points.iter().enumerate() {
            let point = NSPoint::new(f64::from(x) + point_x * scale, f64::from(y) + (BOX.1 - point_y) * scale);
            if index == 0 {
                path.moveToPoint(point);
            } else {
                path.lineToPoint(point);
            }
        }
        if is_closed {
            path.closePath();
        }
        path.setLineWidth(LINE_WIDTH * scale);
        path.setLineJoinStyle(NSLineJoinStyle::Round);
        path.setLineCapStyle(NSLineCapStyle::Round);
        path
    };
    if let Some(polygon) = juice_polygon(percent) {
        color(juice).setFill();
        path_of(&polygon, true).fill();
    }
    color(ink).setStroke();
    path_of(&GLASS, true).stroke();
    path_of(&STRAW, false).stroke();
}
