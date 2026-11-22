//! The confirm card shown inside the settings window, never as a separate window (lesson from NetSpeedGauge).

use block2::RcBlock;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSWindow};
use objc2_foundation::NSString;

/// Shows a sheet on `window` with a destructive confirm button and Cancel; runs `on_confirm` only on confirm.
pub fn show_confirm(window: &NSWindow, title: &str, detail: &str, confirm_label: &str, on_confirm: impl Fn() + 'static) {
    let alert = NSAlert::new(crate::delegate::main_thread());
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.setAlertStyle(NSAlertStyle::Warning);
    let confirm = alert.addButtonWithTitle(&NSString::from_str(confirm_label));
    confirm.setHasDestructiveAction(true);
    let cancel = alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    let handler = RcBlock::new(move |response: isize| {
        if response == NSAlertFirstButtonReturn {
            on_confirm();
        }
    });
    alert.beginSheetModalForWindow_completionHandler(window, Some(&handler));
}
