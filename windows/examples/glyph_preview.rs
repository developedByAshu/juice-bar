//! Draws the tray mark at every icon size into an .ico, for checking it by eye in any tone.
//! Usage: cargo run -p juicebar-windows --example glyph_preview -- <out.ico> [juice hex] [percent or none] [line hex]
//! The app icon itself is made from shared/assets/juicebar.png by windows/tools/make_icon.ps1.

#[cfg(windows)]
fn main() {
    use juicebar_windows::glyph::glass_image;
    use juicebar_windows::status_icon::unpremultiply;
    use juicebar_windows::theme::GREEN;

    const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let out = arguments.first().cloned().unwrap_or_else(|| "glyph_preview.ico".to_string());
    let hex = |index: usize, fallback: u32| arguments.get(index).and_then(|hex| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()).unwrap_or(fallback);
    let color = hex(1, GREEN);
    let percent = arguments.get(2).map_or(Some(75), |text| text.parse::<u8>().ok());
    let line = hex(3, 0xFFFFFF);

    // An .ico file: a directory of images, each a 32-bit bitmap with an empty AND mask.
    let images: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&size| {
            let pixels = glass_image(size, line, color, percent);
            let mut image = Vec::new();
            let header: [u32; 10] = [40, size, size * 2, 1 | (32 << 16), 0, 0, 0, 0, 0, 0];
            for value in header {
                image.extend_from_slice(&value.to_le_bytes());
            }
            // Bitmaps in an icon run bottom row first.
            for row in (0..size).rev() {
                for column in 0..size {
                    image.extend_from_slice(&unpremultiply(pixels[(row * size + column) as usize]).to_le_bytes());
                }
            }
            let mask_row_bytes = size.div_ceil(32) * 4;
            image.extend(std::iter::repeat_n(0u8, (mask_row_bytes * size) as usize));
            image
        })
        .collect();
    let mut file = Vec::new();
    file.extend_from_slice(&[0, 0, 1, 0]);
    file.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (size, image) in SIZES.iter().zip(&images) {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        file.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in &images {
        file.extend_from_slice(image);
    }
    std::fs::write(&out, file).expect("the icon file could not be written");
    println!("Wrote {out}");
}

#[cfg(not(windows))]
fn main() {}
