//! Input Monitoring: macOS hides some dongle interfaces (the mouse's) until the user allows it.
//! Asked once, only when a plugged-in dongle needs it, so most users never see the prompt.

const REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
const ACCESS_GRANTED: u32 = 0;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}

/// Checks whether Input Monitoring is allowed for Juice Bar.
pub fn has_input_monitoring() -> bool {
    // SAFETY: a plain IOKit query with a documented constant.
    unsafe { IOHIDCheckAccess(REQUEST_TYPE_LISTEN_EVENT) == ACCESS_GRANTED }
}

/// Shows the system prompt for Input Monitoring if it was never answered. Returns whether it is allowed.
pub fn request_input_monitoring() -> bool {
    // SAFETY: as above; the system shows its own prompt at most once.
    unsafe { IOHIDRequestAccess(REQUEST_TYPE_LISTEN_EVENT) }
}
