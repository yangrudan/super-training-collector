use inferno::flamegraph::{self, Options, Palette};
use std::io::{BufReader, Cursor};

/// Generates a flamegraph SVG from folded stack input string and returns the SVG content.
pub fn generate_flamegraph_svg(folded_stacks: &str) -> Result<String, Box<dyn std::error::Error>> {
    let reader = BufReader::new(Cursor::new(folded_stacks.as_bytes()));

    let mut options = Options::default();
    options.colors = Palette::Multi(flamegraph::color::MultiPalette::Java);

    let mut svg_buf = Vec::new();
    flamegraph::from_reader(&mut options, reader, &mut svg_buf)?;

    Ok(String::from_utf8(svg_buf)?)
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_generate_flamegraph_svg_basic() {
//         let folded_input = "main;func1;func2 1";
//         let result = generate_flamegraph_svg(folded_input);

//         assert!(result.is_ok(), "Should generate SVG successfully");
//         let svg = result.unwrap();
//         assert!(svg.contains("<svg"), "Output should be valid SVG");
//         assert!(svg.contains("</svg>"), "Output should be complete SVG");
//         assert!(svg.contains("main"), "SVG should contain stack frame names");
//     }

//     #[test]
//     fn test_generate_flamegraph_svg_empty_input() {
//         let folded_input = "";
//         let result = generate_flamegraph_svg(folded_input);

//         // Empty input should still generate a valid (though minimal) SVG
//         assert!(result.is_ok(), "Should handle empty input gracefully");
//         let svg = result.unwrap();
//         assert!(svg.contains("<svg"), "Should still generate SVG structure");
//     }

//     #[test]
//     fn test_generate_flamegraph_svg_multiple_stacks() {
//         let folded_input = "main;func1 1\nmain;func2 1";
//         let result = generate_flamegraph_svg(folded_input);

//         assert!(result.is_ok(), "Should handle multiple stacks");
//         let svg = result.unwrap();
//         assert!(svg.contains("func1") || svg.contains("func2"), "Should contain multiple stack frames");
//     }

//     #[test]
//     fn test_generate_flamegraph_svg_complex_stack() {
//         let folded_input = "main;torch::distributed::init;training_loop;model.forward 1";
//         let result = generate_flamegraph_svg(folded_input);

//         assert!(result.is_ok(), "Should handle complex nested stacks");
//         let svg = result.unwrap();
//         assert!(svg.len() > 100, "SVG should have substantial content");
//     }
// }
