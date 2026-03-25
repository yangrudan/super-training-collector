use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File};
use std::io::{self, Read, Write};
use std::path::Path;

/// Represents a frame in the call stack, which can be either a C frame or a Python frame.
#[derive(Debug, Deserialize, Serialize, Clone)]
enum Frame {
    CFrame(CFrame),
    PyFrame(PyFrame),
}

/// Represents a C frame in the call stack.
#[derive(Debug, Deserialize, Serialize, Clone)]
struct CFrame {
    file: String,
    func: String,
    ip: String,
    lineno: u32,
}

/// Represents a Python frame in the call stack.
#[derive(Debug, Deserialize, Serialize, Clone)]
struct PyFrame {
    file: String,
    func: String,
    lineno: u32,
    locals: serde_json::Value,
}

/// Format a single frame as "func (file:lineno)"
fn format_frame(frame: &Frame) -> String {
    match frame {
        Frame::CFrame(f) => format!("{} ({}:{})", f.func, f.file, f.lineno),
        Frame::PyFrame(f) => format!("{} ({}:{})", f.func, f.file, f.lineno),
    }
}

/// Process a vector of frames into a folded stack string.
/// Reverses the frame order and joins with ";" separator.
/// Returns None if the frames vector is empty.
fn process_frames_to_folded_stack(frames: Vec<Frame>) -> Option<String> {
    if frames.is_empty() {
        return None;
    }

    let mut local_stack: Vec<Frame> = frames;
    local_stack.reverse();

    let folded = local_stack.iter().map(format_frame).collect::<Vec<_>>().join(";");
    Some(folded)
}

/// Process call stack frames from a batch of JSON data.
/// Returns a vector of (rank_id, folded_stack_string) tuples.
///
/// # Arguments
/// * `batch` - Vector of (rank_index, json_value) pairs
///
/// # Returns
/// * Vector of (rank_id, processed_stack_string) where the stack string is
///   in folded format: "frame1;frame2;frame3"
pub fn process_callstacks_batch(batch: Vec<(usize, serde_json::Value)>) -> Vec<(u32, String)> {
    let mut results = Vec::new();

    for (rank_index, json_value) in batch {
        // Parse frames from JSON Value
        let frames: Vec<Frame> = match serde_json::from_value(json_value) {
            Ok(f) => f,
            Err(_) => continue,
        };

        if let Some(folded_stack) = process_frames_to_folded_stack(frames) {
            results.push((rank_index as u32, folded_stack));
        }
    }

    results
}

/// Process call stacks from a JSON file and write the processed stacks to a text file.
pub fn process_callstacks(input_path: &str, output_path: &str) -> io::Result<()> {
    // Read and parse JSON file
    let mut file = File::open(input_path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Failed to open input file '{}': {}", input_path, e),
        )
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Failed to read input file '{}': {}", input_path, e),
        )
    })?;

    // Parse JSON data with better error message
    let frames: Vec<Vec<Frame>> = serde_json::from_str(&contents).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Failed to parse JSON from '{}': {}. Expected array of frame arrays.",
                input_path, e
            ),
        )
    })?;

    // Process call stacks using the helper function
    let mut prepare_stacks = Vec::new();
    for trace in frames {
        if let Some(folded_stack) = process_frames_to_folded_stack(trace) {
            prepare_stacks.push(folded_stack);
        }
    }

    // Ensure output directory exists
    if let Some(parent) = Path::new(output_path).parent() {
        create_dir_all(parent).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Failed to create output directory: {}", e),
            )
        })?;
    }

    // Write stack data to output file
    let mut output_file = File::create(output_path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Failed to create output file '{}': {}", output_path, e),
        )
    })?;
    for stack in prepare_stacks {
        writeln!(output_file, "{}", stack)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_process_callstacks() {
    //     let input_path = "./output/output.json";
    //     let output_path = "./output/processed_stacks.txt";
    //     let result = process_callstacks(input_path, output_path);
    //     assert!(result.is_ok(), "Processing call stacks should succeed");
    //     assert!(std::fs::metadata(output_path).is_ok(), "Output file should exist");
    //     let output_content = std::fs::read_to_string(output_path).expect("Failed to read output file");
    //     assert!(!output_content.is_empty(), "Output file should not be empty");
    // }

    #[test]
    fn test_process_callstacks_invalid_json() {
        use std::io::Write;

        // Create a temporary invalid JSON file
        let input_path = "./output/test_invalid.json";
        let output_path = "./output/test_invalid_output.txt";

        let mut file = File::create(input_path).expect("Failed to create test file");
        file.write_all(b"{invalid json}")
            .expect("Failed to write test data");

        let result = process_callstacks(input_path, output_path);
        assert!(result.is_err(), "Should fail with invalid JSON");

        // Check error message contains useful information
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("Failed to parse JSON"),
                "Error message should indicate JSON parsing failure: {}",
                error_msg
            );
        }

        // Cleanup
        let _ = std::fs::remove_file(input_path);
    }

    #[test]
    fn test_process_callstacks_missing_file() {
        let input_path = "./output/nonexistent_file.json";
        let output_path = "./output/test_output.txt";

        let result = process_callstacks(input_path, output_path);
        assert!(result.is_err(), "Should fail with missing file");

        // Check error message contains useful information
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("Failed to open input file"),
                "Error message should indicate file opening failure: {}",
                error_msg
            );
        }
    }

    #[test]
    fn test_process_callstacks_batch() {
        // Create test data with PyFrame entries
        let frames_json = serde_json::json!([
            {
                "PyFrame": {
                    "file": "main.py",
                    "func": "main",
                    "lineno": 10,
                    "locals": {}
                }
            },
            {
                "PyFrame": {
                    "file": "utils.py",
                    "func": "helper",
                    "lineno": 20,
                    "locals": {}
                }
            }
        ]);

        let batch = vec![(0, frames_json)];
        let results = process_callstacks_batch(batch);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 0);
        // Frames are reversed: helper comes before main
        assert_eq!(
            results[0].1,
            "helper (utils.py:20);main (main.py:10)"
        );
    }

    #[test]
    fn test_process_callstacks_batch_with_cframes() {
        // Create test data with CFrame entries
        let frames_json = serde_json::json!([
            {
                "CFrame": {
                    "file": "lib.c",
                    "func": "do_work",
                    "ip": "0x1234",
                    "lineno": 100
                }
            },
            {
                "CFrame": {
                    "file": "main.c",
                    "func": "main",
                    "ip": "0x5678",
                    "lineno": 50
                }
            }
        ]);

        let batch = vec![(5, frames_json)];
        let results = process_callstacks_batch(batch);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 5);
        // Frames are reversed
        assert_eq!(
            results[0].1,
            "main (main.c:50);do_work (lib.c:100)"
        );
    }

    #[test]
    fn test_process_callstacks_batch_empty_frames() {
        let frames_json = serde_json::json!([]);
        let batch = vec![(0, frames_json)];
        let results = process_callstacks_batch(batch);

        // Empty frames should be skipped
        assert!(results.is_empty());
    }

    #[test]
    fn test_process_callstacks_batch_invalid_json() {
        // Invalid JSON structure (not an array of frames)
        let invalid_json = serde_json::json!({"invalid": "data"});
        let batch = vec![(0, invalid_json)];
        let results = process_callstacks_batch(batch);

        // Invalid entries should be skipped
        assert!(results.is_empty());
    }

    #[test]
    fn test_process_callstacks_batch_multiple_entries() {
        let frames1 = serde_json::json!([
            {
                "PyFrame": {
                    "file": "a.py",
                    "func": "func_a",
                    "lineno": 1,
                    "locals": {}
                }
            }
        ]);

        let frames2 = serde_json::json!([
            {
                "PyFrame": {
                    "file": "b.py",
                    "func": "func_b",
                    "lineno": 2,
                    "locals": {}
                }
            }
        ]);

        let batch = vec![(0, frames1), (1, frames2)];
        let results = process_callstacks_batch(batch);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], (0, "func_a (a.py:1)".to_string()));
        assert_eq!(results[1], (1, "func_b (b.py:2)".to_string()));
    }
}
