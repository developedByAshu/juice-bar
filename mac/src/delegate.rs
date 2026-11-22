//! The one Objective-C object the app needs: it receives launch, timers, wake, menu and button events.

use crate::{app, menu_ui, settings_ui};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplicationDelegate, NSMenu, NSMenuDelegate, NSMenuItem, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSTimer};
use std::cell::OnceCell;

/// Returns the main thread marker. Every call site runs on the main thread.
pub fn main_thread() -> MainThreadMarker {
    MainThreadMarker::new().expect("Juice Bar only runs on the main thread")
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarDelegate"]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            app::finish_startup();
        }
    }

    unsafe impl NSMenuDelegate for AppDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            app::on_menu_opening(menu);
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            settings_ui::on_closed();
        }
    }

    impl AppDelegate {
        #[unsafe(method(poll:))]
        fn poll(&self, _timer: &NSTimer) {
            app::refresh();
        }

        #[unsafe(method(didWake:))]
        fn did_wake(&self, _notification: &NSNotification) {
            app::refresh();
        }

        #[unsafe(method(themeChanged:))]
        fn theme_changed(&self, _notification: &NSNotification) {
            app::redraw_icon();
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            settings_ui::open_settings();
        }

        #[unsafe(method(reportDevice:))]
        fn report_device(&self, sender: &NSMenuItem) {
            menu_ui::on_report(sender.tag());
        }

        #[unsafe(method(tryToDetect:))]
        fn try_to_detect(&self, sender: &NSMenuItem) {
            app::probe(sender.tag());
        }

        #[unsafe(method(rowClicked:))]
        fn row_clicked(&self, _sender: &NSMenuItem) {
            settings_ui::open_settings();
        }

        #[unsafe(method(settingChanged:))]
        fn setting_changed(&self, sender: &AnyObject) {
            settings_ui::on_control_changed(sender);
        }

        #[unsafe(method(resetClicked:))]
        fn reset_clicked(&self, _sender: Option<&AnyObject>) {
            settings_ui::on_reset_clicked();
        }

        #[unsafe(method(captureOpenMenu:))]
        fn capture_open_menu(&self, _timer: &NSTimer) {
            app::capture_open_menu();
        }

        #[unsafe(method(captureFinish:))]
        fn capture_finish(&self, _timer: &NSTimer) {
            app::capture_finish();
        }

        #[unsafe(method(hideResetNote:))]
        fn hide_reset_note(&self, _timer: &NSTimer) {
            settings_ui::hide_reset_note();
        }
    }
);

thread_local! {
    static DELEGATE: OnceCell<Retained<AppDelegate>> = const { OnceCell::new() };
}

/// Returns the app's single delegate, creating it on first use.
pub fn delegate() -> Retained<AppDelegate> {
    DELEGATE.with(|cell| cell.get_or_init(|| unsafe { msg_send![AppDelegate::alloc(main_thread()), init] }).clone())
}

/// Starts a timer that calls the delegate, also while the menu is open.
pub fn start_timer(seconds: f64, selector: objc2::runtime::Sel, repeats: bool) -> Retained<NSTimer> {
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    // SAFETY: the delegate lives for the whole app, and each selector takes the timer as its one argument.
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(seconds, target, selector, None, repeats) };
    timer.setTolerance(seconds / 10.0);
    // SAFETY: the common modes constant is Foundation's own.
    unsafe { objc2_foundation::NSRunLoop::mainRunLoop().addTimer_forMode(&timer, objc2_foundation::NSRunLoopCommonModes) };
    timer
}

/// Subscribes to wake from sleep and to light or dark mode changes.
pub fn watch_system_events() {
    let target = delegate();
    let target: &AnyObject = target.as_ref();
    let workspace_center = objc2_app_kit::NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the delegate lives for the whole app, and both selectors take the notification as their one argument.
    unsafe {
        workspace_center.addObserver_selector_name_object(target, sel!(didWake:), Some(objc2_app_kit::NSWorkspaceDidWakeNotification), None);
        let theme_center = objc2_foundation::NSDistributedNotificationCenter::defaultCenter();
        let name = objc2_foundation::NSString::from_str("AppleInterfaceThemeChangedNotification");
        theme_center.addObserver_selector_name_object(target, sel!(themeChanged:), Some(&name), None);
    }
}
