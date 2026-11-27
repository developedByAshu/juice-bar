//! The menu bar item: the Juice Bar glass (shared geometry in `juicebar_shared::glyph`), filled to the
//! lowest device's level in its tone. Plain tone draws as a template image, so it follows the menu
//! bar's light, dark and tinted looks for free.

use crate::delegate::{delegate, main_thread};
use crate::theme::tone_color;
use block2::RcBlock;
use juicebar_shared::glyph::{juice_polygon, BOX, GLASS, LINE_WIDTH, STRAW};
use juicebar_shared::tone::Tone;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSEvent, NSEventMask, NSTrackingArea, NSTrackingAreaOptions, NSView, NSBezierPath, NSColor, NSImage, NSLineCapStyle, NSLineJoinStyle, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// Point size of the menu bar image; the glyph box is scaled to fit.
const ICON_SIZE: NSSize = NSSize { width: 15.0, height: 17.0 };

/// Creates the menu bar item with an empty glass until the first reading.
pub fn create_status_icon() -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    update_status_icon(&item, Tone::Plain, None, false);
    item
}

define_class!(
    // Lies over the button and watches the pointer for the hover card. It takes no clicks, so the button
    // keeps every click, as NetSpeedGauge's readout view does.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarIconHoverView"]
    pub struct IconHoverView;

    impl IconHoverView {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            crate::app::on_icon_enter();
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            crate::app::on_icon_exit();
        }
    }
);

/// Sends the button's clicks (left and right) to the delegate while no menu is attached, and watches the
/// pointer resting on the icon for the hover card.
pub fn watch_clicks_and_hover(item: &NSStatusItem) {
    let Some(button) = item.button(main_thread()) else {
        return;
    };
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app and answers iconClicked:.
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(sel!(iconClicked:)));
    }
    button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
    let this = IconHoverView::alloc(main_thread()).set_ivars(());
    // SAFETY: initWithFrame: is NSView's designated initialiser.
    let view: Retained<IconHoverView> = unsafe { msg_send![super(this), initWithFrame: button.bounds()] };
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    let options = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
    let owner: &AnyObject = view.as_ref();
    // SAFETY: the view owns the area, and AppKit keeps its owner reference only while the area is attached.
    let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), NSRect::ZERO, options, Some(owner), None) };
    view.addTrackingArea(&area);
    button.addSubview(&view);
}

/// Redraws the glass for the lowest level, and shows that % next to it when asked.
pub fn update_status_icon(item: &NSStatusItem, tone: Tone, lowest_percent: Option<u8>, shows_percent: bool) {
    let Some(button) = item.button(main_thread()) else {
        return;
    };
    button.setImage(Some(&glass_image(ICON_SIZE, tone, lowest_percent)));
    let title = match (shows_percent, lowest_percent) {
        (true, Some(percent)) => format!(" {percent}%"),
        _ => String::new(),
    };
    button.setTitle(&NSString::from_str(&title));
    let label = lowest_percent.map_or_else(|| "Juice Bar".to_string(), |percent| format!("Juice Bar, lowest battery {percent}%"));
    button.setToolTip(Some(&NSString::from_str(&label)));
}

/// Returns a path through points scaled from the glyph box into `size`.
fn scaled_path(points: &[(f64, f64)], size: NSSize, is_closed: bool) -> Retained<NSBezierPath> {
    let (scale_x, scale_y) = (size.width / BOX.0, size.height / BOX.1);
    let path = NSBezierPath::bezierPath();
    for (index, (x, y)) in points.iter().enumerate() {
        let point = NSPoint::new(x * scale_x, y * scale_y);
        if index == 0 {
            path.moveToPoint(point);
        } else {
            path.lineToPoint(point);
        }
    }
    if is_closed {
        path.closePath();
    }
    path.setLineWidth(LINE_WIDTH * scale_x);
    path.setLineJoinStyle(NSLineJoinStyle::Round);
    path.setLineCapStyle(NSLineCapStyle::Round);
    path
}

/// Draws the glass. The drawing runs at display time, so system colours resolve for the current look.
pub fn glass_image(size: NSSize, tone: Tone, lowest_percent: Option<u8>) -> Retained<NSImage> {
    let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
        let ink = if tone == Tone::Plain { NSColor::blackColor() } else { NSColor::labelColor() };
        if let Some(juice) = juice_polygon(lowest_percent) {
            let juice_color = if tone == Tone::Plain { NSColor::blackColor() } else { tone_color(tone) };
            juice_color.setFill();
            scaled_path(&juice, size, true).fill();
        }
        ink.setStroke();
        scaled_path(&GLASS, size, true).stroke();
        scaled_path(&STRAW, size, false).stroke();
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(size, false, &handler);
    image.setTemplate(tone == Tone::Plain);
    image
}
