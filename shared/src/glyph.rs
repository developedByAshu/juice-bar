//! The Juice Bar mark: a juice glass with a straw. The juice level is the battery level and the juice
//! colour is the tone. Both apps draw this same geometry, so the icon is identical on Mac and Windows.
//!
//! Coordinates are in a 16 x 18 box, origin bottom left, y up. Scale to the target size.

/// Width and height of the design box.
pub const BOX: (f64, f64) = (16.0, 18.0);
/// Stroke width of the glass and the straw, in box units.
pub const LINE_WIDTH: f64 = 1.25;
/// Corner radius at the glass's foot, in box units.
pub const FOOT_RADIUS: f64 = 1.2;

/// The glass, as 4 corners: top left, top right, bottom right, bottom left. Tapered like a tumbler.
pub const GLASS: [(f64, f64); 4] = [(1.6, 15.2), (12.4, 15.2), (11.0, 1.0), (3.0, 1.0)];

/// The straw: from inside the juice, out over the rim, then a short bend to the right.
pub const STRAW: [(f64, f64); 3] = [(8.6, 5.0), (11.4, 17.2), (14.4, 17.2)];

/// Gap between the glass wall and the juice, so the juice reads as liquid inside it.
const INSET: f64 = 1.5;
/// The lowest drawn level, so a near-empty device still shows a sliver of juice.
const MIN_FRACTION: f64 = 0.1;

/// Returns the x of a tapered wall at height y, between a top and a bottom corner.
fn wall_x(top: (f64, f64), bottom: (f64, f64), y: f64) -> f64 {
    bottom.0 + (top.0 - bottom.0) * (y - bottom.1) / (top.1 - bottom.1)
}

/// Returns the juice as a polygon (bottom left, bottom right, top right, top left) for a level of 0 to 100.
/// None draws no juice, for a device that has not reported yet.
pub fn juice_polygon(percent: Option<u8>) -> Option<[(f64, f64); 4]> {
    let fraction = (f64::from(percent?) / 100.0).clamp(MIN_FRACTION, 1.0);
    let [top_left, top_right, bottom_right, bottom_left] = GLASS;
    let floor = bottom_left.1 + INSET;
    let ceiling = top_left.1 - INSET;
    let level = floor + (ceiling - floor) * fraction;
    let left = |y: f64| wall_x(top_left, bottom_left, y) + INSET;
    let right = |y: f64| wall_x(top_right, bottom_right, y) - INSET;
    Some([(left(floor), floor), (right(floor), floor), (right(level), level), (left(level), level)])
}

/// Returns the mark as SVG. Strokes use `currentColor` so any page can recolour it; the juice uses `juice_color`.
pub fn glyph_svg(percent: Option<u8>, juice_color: &str) -> String {
    let flip = |(x, y): (f64, f64)| format!("{x:.2},{:.2}", BOX.1 - y);
    let points = |list: &[(f64, f64)]| list.iter().map(|point| flip(*point)).collect::<Vec<_>>().join(" ");
    let juice = juice_polygon(percent).map_or_else(String::new, |polygon| format!("  <polygon points=\"{}\" fill=\"{juice_color}\" stroke=\"none\"/>\n", points(&polygon)));
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"{LINE_WIDTH}\" stroke-linejoin=\"round\" stroke-linecap=\"round\">\n{juice}  <polygon points=\"{}\"/>\n  <polyline points=\"{}\"/>\n</svg>\n",
        BOX.0,
        BOX.1,
        points(&GLASS),
        points(&STRAW)
    )
}
