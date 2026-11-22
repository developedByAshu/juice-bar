//! Writes the mark as SVG files into shared/assets: an outline that takes any colour, and a filled one.
//! Usage: cargo run -p juicebar-shared --example export_glyph

use juicebar_shared::glyph::glyph_svg;

fn main() {
    let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
    std::fs::write(format!("{folder}/juicebar-glyph.svg"), glyph_svg(Some(70), "currentColor")).expect("write");
    std::fs::write(format!("{folder}/juicebar-glyph-empty.svg"), glyph_svg(None, "currentColor")).expect("write");
    println!("wrote {folder}/juicebar-glyph.svg and juicebar-glyph-empty.svg");
}
