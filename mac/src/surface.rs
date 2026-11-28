//! The view the settings window draws into, the window that holds it, and painting it to a PNG.
//! The same shape as NetSpeedGauge's surface, cut down to the one window Juice Bar draws itself.

use crate::delegate::{delegate, main_thread};
use crate::settings_ui::{self, Cursor};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBitmapImageFileType, NSCursor, NSEvent, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow, NSWindowButton, NSWindowStyleMask,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

pub const KEY_ESCAPE: u16 = 53;
// AppKit's normal and floating window levels, as NetSpeedGauge's surface names them.
pub const WINDOW_LEVEL_NORMAL: isize = 0;
pub const WINDOW_LEVEL_FLOATING: isize = 3;

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarSettingsView"]
    pub struct SettingsView;

    impl SettingsView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            settings_ui::draw();
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            settings_ui::on_mouse_exit();
            NSCursor::arrowCursor().set();
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let (x, y) = self.position_of(event);
            match settings_ui::on_mouse_move(x, y) {
                Cursor::Arrow => NSCursor::arrowCursor().set(),
                Cursor::Hand => NSCursor::pointingHandCursor().set(),
                // A rename box sets its own text cursor.
                Cursor::Text => NSCursor::IBeamCursor().set(),
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let (x, y) = self.position_of(event);
            settings_ui::on_click(x, y);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            settings_ui::on_scroll(scroll_points(event));
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            settings_ui::on_key(event.keyCode());
        }
    }
);

impl SettingsView {
    /// Creates a view that paints the settings and reports the mouse.
    fn create(width: i32, height: i32) -> Retained<Self> {
        let mtm = main_thread();
        let frame = NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(width), f64::from(height)));
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: initWithFrame: is NSView's designated initialiser.
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let options = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::MouseMoved | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
        let owner: &AnyObject = view.as_ref();
        // SAFETY: the view owns the area, and AppKit keeps its owner reference only while the area is attached.
        let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), NSRect::ZERO, options, Some(owner), None) };
        view.addTrackingArea(&area);
        // Since macOS 14 a view can paint outside its own area, which let scrolled rows show over the title bar.
        if view.respondsToSelector(sel!(setClipsToBounds:)) {
            // SAFETY: the check above shows this macOS has the method.
            let _: () = unsafe { msg_send![&*view, setClipsToBounds: true] };
        }
        view
    }

    fn position_of(&self, event: &NSEvent) -> (i32, i32) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        (point.x as i32, point.y as i32)
    }
}

/// Creates a normal titled window with close and minimize, sized by its content.
pub fn create_window(width: i32, height: i32, title: &str) -> (Retained<NSWindow>, Retained<SettingsView>) {
    let view = SettingsView::create(width, height);
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
    // SAFETY: standard window creation on the main thread.
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(main_thread()), view.frame(), style, NSBackingStoreType::Buffered, false) };
    window.setTitle(&NSString::from_str(title));
    if let Some(zoom) = window.standardWindowButton(NSWindowButton::ZoomButton) {
        zoom.setHidden(true);
    }
    // SAFETY: the settings state keeps its own reference, so AppKit must not free the window on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(&view));
    window.setInitialFirstResponder(Some(&view));
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate())));
    window.setAcceptsMouseMovedEvents(true);
    (window, view)
}

/// Paints a view and its subviews to a PNG file without showing it.
pub fn capture_view(view: &NSView, path: &str) -> bool {
    let bounds = view.bounds();
    let Some(image) = view.bitmapImageRepForCachingDisplayInRect(bounds) else {
        return false;
    };
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &image);
    let no_options = NSDictionary::new();
    // SAFETY: an empty dictionary is valid for the options.
    let Some(data) = (unsafe { image.representationUsingType_properties(NSBitmapImageFileType::PNG, &no_options) }) else {
        return false;
    };
    data.writeToFile_atomically(&NSString::from_str(path), true)
}

/// Paints a view off screen and throws the picture away, so what it draws (and where it can be clicked) is current.
pub fn paint_off_screen(view: &NSView) {
    let bounds = view.bounds();
    if let Some(image) = view.bitmapImageRepForCachingDisplayInRect(bounds) {
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &image);
    }
}

/// How far one notch of a mouse wheel scrolls, in points. A wheel reports lines, about one per notch,
/// while a trackpad reports points; read as points, a wheel crawled one point per notch.
const WHEEL_LINE_POINTS: f64 = 18.0;

/// Returns a scroll event's distance in points, for a trackpad and a mouse wheel alike.
fn scroll_points(event: &NSEvent) -> i32 {
    let delta = event.scrollingDeltaY();
    (if event.hasPreciseScrollingDeltas() { delta } else { delta * WHEEL_LINE_POINTS }).round() as i32
}
