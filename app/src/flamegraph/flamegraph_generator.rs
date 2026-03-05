use std::io::{BufReader, Cursor};
use inferno::flamegraph::{self, Options, Palette};

/// Generates a flamegraph SVG from folded stack input string and returns the SVG content.
pub fn generate_flamegraph_svg(folded_stacks: &str) -> Result<String, Box<dyn std::error::Error>> {
    let reader = BufReader::new(Cursor::new(folded_stacks.as_bytes()));

    let mut options = Options::default();
    options.colors = Palette::Multi(flamegraph::color::MultiPalette::Java);

    let mut svg_buf = Vec::new();
    flamegraph::from_reader(&mut options, reader, &mut svg_buf)?;

    Ok(String::from_utf8(svg_buf)?)
}
