//! Small helpers every Windows file needs.

/// Returns text as a null-terminated UTF-16 string, the form Windows functions take.
pub fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies text into a fixed UTF-16 buffer such as a tooltip field, cut to fit and null-terminated.
pub fn copy_into(buffer: &mut [u16], text: &str) {
    let units: Vec<u16> = text.encode_utf16().take(buffer.len().saturating_sub(1)).collect();
    buffer[..units.len()].copy_from_slice(&units);
    buffer[units.len()] = 0;
}

/// Scales a length given at 100% screen scaling to the given dpi.
pub fn scale(value: i32, dpi: u32) -> i32 {
    (value as f32 * dpi as f32 / 96.0).round() as i32
}

/// Appends a line to %TEMP%\juicebar.log when `JUICEBAR_LOG=1`, for chasing a problem on a real machine.
pub fn log(line: &str) {
    if std::env::var_os("JUICEBAR_LOG").is_none() {
        return;
    }
    let path = std::env::temp_dir().join("juicebar.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
    }
}
