//! The one Objective-C object the app needs: it receives launch, timers, wake, menu and button events.

use crate::{app, menu_ui, settings_ui};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplicationDelegate, NSControl, NSControlTextEditingDelegate, NSMenu, NSMenuDelegate, NSMenuItem, NSTextFieldDelegate, NSTextView, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSTimer};
use std::cell::OnceCell;

const SETTINGS_THEME_DELAY_SECONDS: f64 = 0.3;

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

        #[unsafe(method(menuDidClose:))]
        fn menu_did_close(&self, _menu: &NSMenu) {
            app::on_menu_closed();
        }
    }

    // The rename boxes: typing marks a row unsaved, Return saves at once, Escape puts the saved name back.
    unsafe impl NSControlTextEditingDelegate for AppDelegate {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            if let Some(field) = notification.object().and_then(|object| object.downcast::<NSControl>().ok()) {
                settings_ui::on_rename_typed(&field);
            }
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(&self, control: &NSControl, text_view: &NSTextView, command: Sel) -> bool {
            if command == sel!(insertNewline:) {
                settings_ui::on_rename_return(control, text_view)
            } else if command == sel!(cancelOperation:) {
                settings_ui::on_rename_cancel(control, text_view)
            } else {
                false
            }
        }
    }

    unsafe impl NSTextFieldDelegate for AppDelegate {}

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            settings_ui::on_closed();
        }
    }

    impl AppDelegate {
        #[unsafe(method(iconClicked:))]
        fn icon_clicked(&self, _sender: Option<&AnyObject>) {
            app::on_icon_clicked();
        }

        #[unsafe(method(renameSavedExpired:))]
        fn rename_saved_expired(&self, _timer: &NSTimer) {
            settings_ui::hide_rename_saved();
        }

        #[unsafe(method(hoverFired:))]
        fn hover_fired(&self, _timer: &NSTimer) {
            app::on_hover_fired();
        }

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
            // The app's own look switches a moment after this notice, so the settings window follows a little later.
            start_timer(SETTINGS_THEME_DELAY_SECONDS, sel!(settingsThemeChanged:), false);
        }

        #[unsafe(method(settingsThemeChanged:))]
        fn settings_theme_changed(&self, _timer: &NSTimer) {
            settings_ui::apply_theme();
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

        #[unsafe(method(captureOpenMenu:))]
        fn capture_open_menu(&self, _timer: &NSTimer) {
            app::capture_open_menu();
        }

        #[unsafe(method(captureReset:))]
        fn capture_reset(&self, _timer: &NSTimer) {
            app::capture_reset();
        }

        #[unsafe(method(captureRename:))]
        fn capture_rename(&self, _timer: &NSTimer) {
            app::capture_rename();
        }

        #[unsafe(method(captureCloseWhileEditing:))]
        fn capture_close_while_editing(&self, _timer: &NSTimer) {
            app::capture_close_while_editing();
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

/// Returns true when the event being handled is a right click or a Control click, as NetSpeedGauge checks it.
pub fn current_event_asks_for_menu() -> bool {
    let event = objc2_app_kit::NSApplication::sharedApplication(main_thread()).currentEvent();
    event.is_some_and(|event| {
        matches!(event.r#type(), objc2_app_kit::NSEventType::RightMouseDown | objc2_app_kit::NSEventType::RightMouseUp)
            || event.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Control)
    })
}
