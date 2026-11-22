//! The Windows side of Juice Bar: the tray icon, its flyout and the settings window. Everything
//! that is the same on Mac lives in `juicebar_shared`.
#![cfg(windows)]
// Window, font and drawing handles are opaque numbers Windows hands out, never dereferenced here.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// Drawing calls take a position, size and look; a struct per call would only hide that.
#![allow(clippy::too_many_arguments)]

pub use juicebar_shared as shared;

pub mod app;
pub mod app_main;
pub mod bluetooth;
pub mod canvas;
pub mod confirm_sheet;
pub mod glyph;
pub mod menu_ui;
pub mod messages;
pub mod settings_ui;
pub mod startup;
pub mod status_icon;
pub mod theme;
pub mod util;
