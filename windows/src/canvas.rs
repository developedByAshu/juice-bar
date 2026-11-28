//! A drawing surface with smooth rounded shapes and ClearType text, using only built-in Windows
//! drawing. The flyout and the settings window are solid, as NetSpeedGauge's are, so text is drawn
//! straight onto the painted background exactly the way NetSpeedGauge draws it.

use crate::glyph::round_rect_coverage;
use crate::util::to_wide;
use windows_sys::Win32::Foundation::SIZE;
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GdiFlush, GetTextExtentPoint32W,
    SelectObject, SetBkMode, SetTextColor, TextOutW, ANTIALIASED_QUALITY, BITMAPINFO, CLEARTYPE_QUALITY, FONT_QUALITY, BITMAPINFOHEADER, BI_RGB,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, HBITMAP, HDC, HFONT,
    OUT_DEFAULT_PRECIS, SRCCOPY, TRANSPARENT,
};

const POINTS_PER_INCH: f32 = 72.0;

/// Which side of the given position the text sits on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Creates a ClearType font at a size in points for the given screen scaling, as NetSpeedGauge does.
pub fn create_font(family: &str, points: f32, weight: i32, dpi: u32) -> HFONT {
    font_with_quality(family, points, weight, dpi, CLEARTYPE_QUALITY)
}

/// Creates a grey-smoothed font for text on see-through pixels, such as the tray icon's number.
pub fn create_see_through_font(family: &str, points: f32, weight: i32, dpi: u32) -> HFONT {
    font_with_quality(family, points, weight, dpi, ANTIALIASED_QUALITY)
}

fn font_with_quality(family: &str, points: f32, weight: i32, dpi: u32, quality: FONT_QUALITY) -> HFONT {
    let family = to_wide(family);
    // SAFETY: `family` is null-terminated and outlives the call. The caller owns the returned font.
    unsafe {
        CreateFontW(
            -((points * dpi as f32 / POINTS_PER_INCH).round() as i32),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_DEFAULT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(quality),
            u32::from(DEFAULT_PITCH) | u32::from(FF_DONTCARE),
            family.as_ptr(),
        )
    }
}

/// Blends two 0xRRGGBB colours. An `amount` of 0 gives `from`, and 1 gives `to`.
pub fn mix(from: u32, to: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let start = ((from >> shift) & 0xFF) as f32;
        let end = ((to >> shift) & 0xFF) as f32;
        ((start + (end - start) * amount).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Blends a 0xRRGGBB colour at a coverage over one premultiplied 0xAARRGGBB pixel.
pub fn blend(pixel: u32, color: u32, coverage: f32) -> u32 {
    let alpha = coverage.clamp(0.0, 1.0);
    let keep = 1.0 - alpha;
    let channel = |shift: u32, source: f32| ((source * alpha + ((pixel >> shift) & 0xFF) as f32 * keep).round() as u32).min(255) << shift;
    channel(24, 255.0) | channel(16, ((color >> 16) & 0xFF) as f32) | channel(8, ((color >> 8) & 0xFF) as f32) | channel(0, (color & 0xFF) as f32)
}

/// A 32-bit bitmap selected into its own drawing surface.
struct Bitmap {
    surface: HDC,
    bitmap: HBITMAP,
    pixels: *mut u32,
    width: i32,
    height: i32,
}

impl Bitmap {
    fn new(width: i32, height: i32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        // SAFETY: the header describes a top-down 32-bit bitmap, and `pixels` receives its memory.
        unsafe {
            let surface = CreateCompatibleDC(std::ptr::null_mut());
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width;
            info.bmiHeader.biHeight = -height;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let mut pixels = std::ptr::null_mut();
            let bitmap = CreateDIBSection(surface, &info, DIB_RGB_COLORS, &mut pixels, std::ptr::null_mut(), 0);
            SelectObject(surface, bitmap);
            SetBkMode(surface, TRANSPARENT as i32);
            Self { surface, bitmap, pixels: pixels.cast(), width, height }
        }
    }

    fn pixels(&mut self) -> &mut [u32] {
        // SAFETY: queued drawing is finished after GdiFlush, and the bitmap holds width * height pixels.
        unsafe {
            GdiFlush();
            std::slice::from_raw_parts_mut(self.pixels, (self.width * self.height) as usize)
        }
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: both handles were created in new() and are released once here.
        unsafe {
            DeleteDC(self.surface);
            DeleteObject(self.bitmap);
        }
    }
}

/// An off-screen picture of a window. It is painted fully, then shown in one step, so it never flickers.
pub struct Canvas {
    target: Bitmap,
}

impl Canvas {
    /// Creates a fully see-through canvas of the given pixel size.
    pub fn new(width: i32, height: i32) -> Self {
        Self { target: Bitmap::new(width, height) }
    }

    pub fn width(&self) -> i32 {
        self.target.width
    }

    pub fn height(&self) -> i32 {
        self.target.height
    }

    /// Fills the whole canvas with one colour at an opacity; 0 leaves the backdrop showing.
    pub fn clear(&mut self, color: u32, opacity: f32) {
        let pixel = blend(0, color, opacity);
        self.target.pixels().fill(pixel);
    }

    /// Draws a filled rounded rectangle with smooth edges, at an opacity from 0 to 1.
    pub fn round_rect(&mut self, x: i32, y: i32, width: i32, height: i32, radius: f32, color: u32, opacity: f32) {
        let (canvas_width, canvas_height) = (self.target.width, self.target.height);
        let pixels = self.target.pixels();
        for row in y.max(0)..(y + height).min(canvas_height) {
            for column in x.max(0)..(x + width).min(canvas_width) {
                let coverage = round_rect_coverage(column as f32 + 0.5, row as f32 + 0.5, x as f32, y as f32, width as f32, height as f32, radius);
                if coverage > 0.0 {
                    let pixel = &mut pixels[(row * canvas_width + column) as usize];
                    *pixel = blend(*pixel, color, coverage * opacity);
                }
            }
        }
    }

    /// Draws a rounded outline of the given thickness.
    pub fn round_outline(&mut self, x: i32, y: i32, width: i32, height: i32, radius: f32, thickness: f32, color: u32, opacity: f32) {
        let (canvas_width, canvas_height) = (self.target.width, self.target.height);
        let pixels = self.target.pixels();
        for row in y.max(0)..(y + height).min(canvas_height) {
            for column in x.max(0)..(x + width).min(canvas_width) {
                let (px, py) = (column as f32 + 0.5, row as f32 + 0.5);
                let outer = round_rect_coverage(px, py, x as f32, y as f32, width as f32, height as f32, radius);
                let inner = round_rect_coverage(
                    px,
                    py,
                    x as f32 + thickness,
                    y as f32 + thickness,
                    width as f32 - 2.0 * thickness,
                    height as f32 - 2.0 * thickness,
                    (radius - thickness).max(0.0),
                );
                let coverage = (outer - inner).max(0.0);
                if coverage > 0.0 {
                    let pixel = &mut pixels[(row * canvas_width + column) as usize];
                    *pixel = blend(*pixel, color, coverage * opacity);
                }
            }
        }
    }

    /// Draws a filled circle.
    pub fn circle(&mut self, center_x: i32, center_y: i32, radius: i32, color: u32) {
        self.round_rect(center_x - radius, center_y - radius, radius * 2, radius * 2, radius as f32, color, 1.0);
    }

    /// Copies premultiplied pixels, such as a glyph, onto the canvas at a position.
    pub fn draw_pixels(&mut self, x: i32, y: i32, width: i32, source: &[u32]) {
        let (canvas_width, canvas_height) = (self.target.width, self.target.height);
        let height = source.len() as i32 / width.max(1);
        let pixels = self.target.pixels();
        for row in 0..height {
            for column in 0..width {
                let (to_x, to_y) = (x + column, y + row);
                if to_x < 0 || to_y < 0 || to_x >= canvas_width || to_y >= canvas_height {
                    continue;
                }
                let over = source[(row * width + column) as usize];
                let alpha = (over >> 24) as f32 / 255.0;
                if alpha <= 0.0 {
                    continue;
                }
                let pixel = &mut pixels[(to_y * canvas_width + to_x) as usize];
                let channel = |shift: u32| ((((over >> shift) & 0xFF) as f32 + ((*pixel >> shift) & 0xFF) as f32 * (1.0 - alpha)).round() as u32).min(255) << shift;
                *pixel = channel(24) | channel(16) | channel(8) | channel(0);
            }
        }
    }

    /// Draws text in a 0xRRGGBB colour straight onto the solid canvas with ClearType, as NetSpeedGauge does.
    pub fn text(&mut self, x: i32, y: i32, text: &str, font: HFONT, color: u32, align: Align) {
        if text.is_empty() {
            return;
        }
        let width = self.text_width(text, font);
        let left = match align {
            Align::Left => x,
            Align::Center => x - width / 2,
            Align::Right => x - width,
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        let windows_color = ((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF);
        // SAFETY: the surface and font are valid, and `wide` outlives the call.
        unsafe {
            GdiFlush();
            let previous = SelectObject(self.target.surface, font);
            SetTextColor(self.target.surface, windows_color);
            TextOutW(self.target.surface, left, y, wide.as_ptr(), wide.len() as i32);
            GdiFlush();
            // A font left selected cannot be deleted, so the caller's DeleteObject would fail and leak it.
            SelectObject(self.target.surface, previous);
        }
    }

    /// Draws text on see-through pixels, as the tray icon needs: drawn as a mask first, then blended in
    /// its colour, so the taskbar shows around each letter. Use a font from `create_see_through_font`.
    pub fn see_through_text(&mut self, x: i32, y: i32, text: &str, font: HFONT, color: u32, align: Align) {
        if text.is_empty() {
            return;
        }
        let (width, height) = self.text_size(text, font);
        let left = match align {
            Align::Left => x,
            Align::Center => x - width / 2,
            Align::Right => x - width,
        };
        let mut mask = Bitmap::new(width + 2, height);
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: the mask surface and font are valid, and `wide` outlives the call.
        unsafe {
            SelectObject(mask.surface, font);
            SetTextColor(mask.surface, 0x00FF_FFFF);
            TextOutW(mask.surface, 1, 0, wide.as_ptr(), wide.len() as i32);
        }
        let (mask_width, mask_height) = (mask.width, mask.height);
        let coverage: Vec<f32> = mask.pixels().iter().map(|pixel| ((pixel >> 8) & 0xFF) as f32 / 255.0).collect();
        let (canvas_width, canvas_height) = (self.target.width, self.target.height);
        let pixels = self.target.pixels();
        for row in 0..mask_height {
            for column in 0..mask_width {
                let amount = coverage[(row * mask_width + column) as usize];
                let (to_x, to_y) = (left - 1 + column, y + row);
                if amount <= 0.0 || to_x < 0 || to_y < 0 || to_x >= canvas_width || to_y >= canvas_height {
                    continue;
                }
                let pixel = &mut pixels[(to_y * canvas_width + to_x) as usize];
                *pixel = blend(*pixel, color, amount);
            }
        }
    }

    /// Returns the pixel width and height of a string in a font.
    pub fn text_size(&self, text: &str, font: HFONT) -> (i32, i32) {
        let wide: Vec<u16> = if text.is_empty() { vec![u16::from(b'A')] } else { text.encode_utf16().collect() };
        let mut size = SIZE { cx: 0, cy: 0 };
        // SAFETY: `wide` and `size` are valid for the duration of the calls.
        unsafe {
            let previous = SelectObject(self.target.surface, font);
            GetTextExtentPoint32W(self.target.surface, wide.as_ptr(), wide.len() as i32, &mut size);
            SelectObject(self.target.surface, previous);
        }
        (if text.is_empty() { 0 } else { size.cx }, size.cy)
    }

    /// Returns the pixel width of a string in a font.
    pub fn text_width(&self, text: &str, font: HFONT) -> i32 {
        self.text_size(text, font).0
    }

    /// Returns the pixel height of a line of text in a font.
    pub fn text_height(&self, font: HFONT) -> i32 {
        self.text_size("", font).1
    }

    /// Copies the finished picture to the screen, see-through pixels included.
    pub fn present(&self, target: HDC) {
        // SAFETY: both surfaces are valid for the duration of the call.
        unsafe { BitBlt(target, 0, 0, self.target.width, self.target.height, self.target.surface, 0, 0, SRCCOPY) };
    }

    /// Returns a copy of the pixels, for tests and screenshots.
    pub fn snapshot(&mut self) -> Vec<u32> {
        self.target.pixels().to_vec()
    }
}
