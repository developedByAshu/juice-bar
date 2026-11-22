//! Everything Juice Bar does that is the same on Mac and Windows: devices, protocols, names,
//! the colour rule, settings and the Report link. The `mac` and `windows` crates only draw.

pub mod catalog;
pub mod demo;
pub mod device;
pub mod names;
pub mod protocols;
pub mod report;
pub mod scan;
pub mod settings;
pub mod tone;
pub mod transport;

/// The app version, for the Report text and the About line.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
