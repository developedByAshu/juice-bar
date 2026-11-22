//! The Juice Bar mark, drawn for the tray: a juice glass with a straw. The juice level is the lowest
//! battery and its colour the tone. The geometry is `juicebar_shared::glyph`, so the Mac and Windows
//! icons are the same shape; this file only turns it into smooth pixels.
//!
//! Pixels are premultiplied 0xAARRGGBB, the format the drawing surface and icon code take.

use juicebar_shared::glyph::{juice_polygon, BOX, FOOT_RADIUS, GLASS, LINE_WIDTH, STRAW};

// Each pixel is sampled on a grid this many times per side, to smooth the juice's slanted edges.
const SAMPLES: u32 = 4;
const ARC_STEPS: usize = 6;

/**
 * Returns how much of a pixel a rounded rectangle covers, from 0 to 1. Partly covered edge pixels
 * are what make corners look smooth rather than jagged.
 */
pub fn round_rect_coverage(px: f32, py: f32, x: f32, y: f32, width: f32, height: f32, radius: f32) -> f32 {
    let half_width = width / 2.0;
    let half_height = height / 2.0;
    let radius = radius.min(half_width).min(half_height);
    let offset_x = (px - (x + half_width)).abs() - (half_width - radius);
    let offset_y = (py - (y + half_height)).abs() - (half_height - radius);
    let outside = offset_x.max(0.0).hypot(offset_y.max(0.0));
    let inside = offset_x.max(offset_y).min(0.0);
    (0.5 - (outside + inside - radius)).clamp(0.0, 1.0)
}

/// Returns a 0xRRGGBB colour at a coverage as one premultiplied 0xAARRGGBB pixel.
pub fn premultiply(color: u32, coverage: f32) -> u32 {
    let alpha = coverage.clamp(0.0, 1.0);
    let channel = |shift: u32| ((((color >> shift) & 0xFF) as f32 * alpha).round() as u32) << shift;
    (((alpha * 255.0).round() as u32) << 24) | channel(16) | channel(8) | channel(0)
}

/// Lays a colour at a coverage over a premultiplied pixel.
fn over(pixel: u32, color: u32, coverage: f32) -> u32 {
    if coverage <= 0.0 {
        return pixel;
    }
    let top = premultiply(color, coverage);
    let keep = 1.0 - coverage.clamp(0.0, 1.0);
    let channel = |shift: u32| ((((top >> shift) & 0xFF) as f32 + ((pixel >> shift) & 0xFF) as f32 * keep).round() as u32).min(255) << shift;
    channel(24) | channel(16) | channel(8) | channel(0)
}

/// Maps the design box (y up) onto a square of `size` pixels (y down), centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub scale: f64,
    pub left: f64,
    pub top: f64,
}

impl Placement {
    pub fn for_size(size: u32) -> Self {
        let scale = f64::from(size) / BOX.0.max(BOX.1);
        Self { scale, left: (f64::from(size) - BOX.0 * scale) / 2.0, top: (f64::from(size) - BOX.1 * scale) / 2.0 }
    }

    /// Returns a design point in pixels.
    pub fn to_pixels(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.left + x * self.scale, self.top + (BOX.1 - y) * self.scale)
    }
}

/// Returns the glass outline as a closed path with the foot corners rounded, in design units.
pub fn glass_path() -> Vec<(f64, f64)> {
    let [top_left, top_right, bottom_right, bottom_left] = GLASS;
    let mut path = vec![top_left, top_right];
    // Each foot corner becomes a short arc between the two walls that meet there.
    for (corner, from, to) in [(bottom_right, top_right, bottom_left), (bottom_left, bottom_right, top_left)] {
        let toward = |target: (f64, f64)| {
            let (dx, dy) = (target.0 - corner.0, target.1 - corner.1);
            let length = dx.hypot(dy);
            (corner.0 + dx / length * FOOT_RADIUS, corner.1 + dy / length * FOOT_RADIUS)
        };
        let (start, end) = (toward(from), toward(to));
        for step in 0..=ARC_STEPS {
            let t = step as f64 / ARC_STEPS as f64;
            // A quadratic curve through the corner stands in for the arc at icon sizes.
            let x = (1.0 - t).powi(2) * start.0 + 2.0 * (1.0 - t) * t * corner.0 + t * t * end.0;
            let y = (1.0 - t).powi(2) * start.1 + 2.0 * (1.0 - t) * t * corner.1 + t * t * end.1;
            path.push((x, y));
        }
    }
    path.push(top_left);
    path
}

/// Returns the distance from a point to a line segment.
fn segment_distance(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared == 0.0 { 0.0 } else { (((point.0 - start.0) * dx + (point.1 - start.1) * dy) / length_squared).clamp(0.0, 1.0) };
    (point.0 - (start.0 + t * dx)).hypot(point.1 - (start.1 + t * dy))
}

/// Returns how much of a pixel a stroked path covers. Measuring to the nearest segment gives round joins and caps.
fn stroke_coverage(center: (f64, f64), path: &[(f64, f64)], half_width: f64) -> f32 {
    let distance = path.windows(2).map(|pair| segment_distance(center, pair[0], pair[1])).fold(f64::MAX, f64::min);
    (0.5 + half_width - distance).clamp(0.0, 1.0) as f32
}

/// Checks whether a point is inside a convex polygon given in order.
fn inside_convex(point: (f64, f64), polygon: &[(f64, f64)]) -> bool {
    let mut sign = 0.0;
    for index in 0..polygon.len() {
        let (a, b) = (polygon[index], polygon[(index + 1) % polygon.len()]);
        let cross = (b.0 - a.0) * (point.1 - a.1) - (b.1 - a.1) * (point.0 - a.0);
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    true
}

/**
 * Draws the mark into a square of `size` pixels: glass and straw in `line_color`, juice in
 * `juice_color` up to `percent`. `None` draws an empty glass, for nothing read yet.
 */
pub fn glass_image(size: u32, line_color: u32, juice_color: u32, percent: Option<u8>) -> Vec<u32> {
    let placement = Placement::for_size(size);
    let to_pixels = |points: &[(f64, f64)]| points.iter().map(|point| placement.to_pixels(*point)).collect::<Vec<_>>();
    let glass = to_pixels(&glass_path());
    let straw = to_pixels(&STRAW);
    let juice = juice_polygon(percent).map(|polygon| to_pixels(&polygon));
    // Strokes never get thinner than one pixel, so the 16 px icon stays solid.
    let half_width = (LINE_WIDTH * placement.scale).max(1.0) / 2.0;
    let mut pixels = vec![0u32; (size * size) as usize];
    for row in 0..size {
        for column in 0..size {
            let center = (f64::from(column) + 0.5, f64::from(row) + 0.5);
            let juice_coverage = juice.as_ref().map_or(0.0, |polygon| {
                let mut hits = 0;
                for sub_row in 0..SAMPLES {
                    for sub_column in 0..SAMPLES {
                        let sample = (f64::from(column) + (f64::from(sub_column) + 0.5) / f64::from(SAMPLES), f64::from(row) + (f64::from(sub_row) + 0.5) / f64::from(SAMPLES));
                        hits += u32::from(inside_convex(sample, polygon));
                    }
                }
                hits as f32 / (SAMPLES * SAMPLES) as f32
            });
            let line_coverage = stroke_coverage(center, &glass, half_width).max(stroke_coverage(center, &straw, half_width));
            let pixel = over(over(0, juice_color, juice_coverage), line_color, line_coverage);
            pixels[(row * size + column) as usize] = pixel;
        }
    }
    pixels
}
