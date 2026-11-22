//! Draws the app icon from the shared glass geometry and saves shared/assets/juicebar.png (1024 px).
//! Usage: cargo run -p juicebar-mac --example make_icon

#[cfg(target_os = "macos")]
fn main() {
    use juicebar_shared::glyph::{juice_polygon, BOX, GLASS, LINE_WIDTH, STRAW};
    use objc2::AnyThread;
    use objc2_app_kit::{NSBezierPath, NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSGradient, NSGraphicsContext, NSLineCapStyle, NSLineJoinStyle};
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    const SIZE: f64 = 1024.0;
    // Apple's icon grid: the tile fills 824 of 1024 px, with a 185 px corner.
    const TILE: f64 = 824.0;
    const CORNER: f64 = 185.0;
    const GLYPH_HEIGHT: f64 = 560.0;

    let rgb = |red: f64, green: f64, blue: f64| NSColor::colorWithSRGBRed_green_blue_alpha(red / 255.0, green / 255.0, blue / 255.0, 1.0);
    // SAFETY: standard bitmap creation with a nil buffer, so AppKit owns the pixels.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            SIZE as isize,
            SIZE as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .expect("bitmap");
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap).expect("context");
    NSGraphicsContext::setCurrentContext(Some(&context));

    let margin = (SIZE - TILE) / 2.0;
    let tile = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(NSRect::new(NSPoint::new(margin, margin), NSSize::new(TILE, TILE)), CORNER, CORNER);
    let gradient = NSGradient::initWithStartingColor_endingColor(NSGradient::alloc(), &rgb(255.0, 159.0, 67.0), &rgb(255.0, 84.0, 112.0)).expect("gradient");
    gradient.drawInBezierPath_angle(&tile, -90.0);

    let scale = GLYPH_HEIGHT / BOX.1;
    let offset_x = (SIZE - BOX.0 * scale) / 2.0;
    let offset_y = (SIZE - GLYPH_HEIGHT) / 2.0;
    let to_point = |(x, y): (f64, f64)| NSPoint::new(offset_x + x * scale, offset_y + y * scale);
    let path_through = |points: &[(f64, f64)], is_closed: bool| {
        let path = NSBezierPath::bezierPath();
        points.iter().enumerate().for_each(|(index, point)| if index == 0 { path.moveToPoint(to_point(*point)) } else { path.lineToPoint(to_point(*point)) });
        if is_closed {
            path.closePath();
        }
        path.setLineWidth(LINE_WIDTH * scale);
        path.setLineJoinStyle(NSLineJoinStyle::Round);
        path.setLineCapStyle(NSLineCapStyle::Round);
        path
    };

    // Juice: a sunny yellow with a lighter surface band, so it reads as liquid.
    let juice = juice_polygon(Some(72)).expect("juice");
    rgb(255.0, 214.0, 64.0).setFill();
    path_through(&juice, true).fill();
    let surface_top = juice[2].1;
    let band = [(juice[3].0, surface_top - 0.9), (juice[2].0 - 0.08, surface_top - 0.9), juice[2], juice[3]];
    rgb(255.0, 236.0, 150.0).setFill();
    path_through(&band, true).fill();

    // A white bolt in the juice: the battery hint.
    let bolt = [(6.8, 9.6), (4.8, 6.0), (6.4, 6.0), (5.6, 3.0), (8.6, 7.2), (7.0, 7.2), (8.0, 9.6)];
    NSColor::whiteColor().setFill();
    path_through(&bolt, true).fill();

    NSColor::whiteColor().setStroke();
    path_through(&GLASS, true).stroke();
    path_through(&STRAW, false).stroke();

    NSGraphicsContext::setCurrentContext(None);
    let no_options = NSDictionary::new();
    // SAFETY: an empty dictionary is valid for the PNG options.
    let data = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &no_options) }.expect("png");
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../shared/assets/juicebar.png");
    data.writeToFile_atomically(&NSString::from_str(path), true);
    println!("wrote {path}");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
