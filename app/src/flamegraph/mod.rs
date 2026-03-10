mod stack_collector;
mod stack_merger;
mod process_data;
mod flamegraph_generator;

use stack_collector::fetch_and_save_urls;
use stack_merger::merge_stacks;
use process_data::process_callstacks;
use flamegraph_generator::generate_flamegraph_svg;

use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};

/// Simple collector config: only the base port for rank 0's callstack endpoint.
#[derive(Deserialize)]
pub struct CollectorConfig {
    pub callstack_base_port: u16,
}

/// Load collector config from `config/collector.json`.
pub fn load_collector_config(config_path: &str) -> Result<CollectorConfig, Box<dyn std::error::Error>> {
    let mut file = File::open(config_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: CollectorConfig = serde_json::from_str(&contents)?;
    Ok(config)
}

/// Build callstack URLs for a node: one URL per rank, port = base_port + local_rank.
pub fn build_callstack_urls(ip: &str, rank_count: u8, base_port: u16) -> Vec<String> {
    (0..rank_count)
        .map(|i| format!("http://{}:{}/apis/pythonext/callstack", ip, base_port + i as u16))
        .collect()
}

/// Collect stacks for the given node, generate a flamegraph SVG, and return it as a String.
pub async fn collect_and_generate_flamegraph(
    node_ip: &str,
    urls: Vec<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    // 1. Fetch raw stack data from URLs and save to output/output.json
    fetch_and_save_urls(urls).await?;

    // 2. Process call stacks (parse JSON → formatted text)
    let input_path = "./output/output.json".to_string();
    let processed_path = format!("./output/processed_{}.txt", node_ip.replace('.', "_"));
    let processed_path_clone = processed_path.clone();

    tokio::task::spawn_blocking(move || {
        process_callstacks(&input_path, &processed_path_clone)
    }).await??;

    // 3. Merge stacks and generate folded format
    let folded = tokio::task::spawn_blocking(move || -> Result<String, std::io::Error> {
        let file = File::open(&processed_path)?;
        let reader = BufReader::new(file);

        let mut stacks: Vec<String> = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if !line.is_empty() {
                stacks.push(line);
            }
        }

        let stack_refs: Vec<&str> = stacks.iter().map(|s| s.as_str()).collect();
        let trie = merge_stacks(stack_refs);

        let mut folded_buf = Vec::new();
        for (path, rank_str) in trie.traverse_with_all_stack(&trie.root, Vec::new()) {
            writeln!(folded_buf, "{} {} 1", path.join(";"), rank_str)?;
        }

        Ok(String::from_utf8_lossy(&folded_buf).into_owned())
    }).await??;

    // 4. Generate SVG
    let svg = generate_flamegraph_svg(&folded)?;
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_collector_config_valid() {
        let config_content = r#"{"callstack_base_port": 9933}"#;
        let temp_file = "/tmp/test_collector_config.json";
        fs::write(temp_file, config_content).expect("Failed to write test config");
        
        let result = load_collector_config(temp_file);
        assert!(result.is_ok(), "Should load valid config successfully");
        
        let config = result.unwrap();
        assert_eq!(config.callstack_base_port, 9933, "Should parse port correctly");
        
        // Cleanup
        let _ = fs::remove_file(temp_file);
    }

    #[test]
    fn test_load_collector_config_invalid_json() {
        let temp_file = "/tmp/test_invalid_config.json";
        fs::write(temp_file, "{invalid json}").expect("Failed to write test file");
        
        let result = load_collector_config(temp_file);
        assert!(result.is_err(), "Should fail on invalid JSON");
        
        // Cleanup
        let _ = fs::remove_file(temp_file);
    }

    #[test]
    fn test_load_collector_config_missing_file() {
        let result = load_collector_config("/nonexistent/path/config.json");
        assert!(result.is_err(), "Should fail on missing file");
    }

    #[test]
    fn test_build_callstack_urls() {
        let ip = "192.168.1.100";
        let rank_count = 4;
        let base_port = 9933;
        
        let urls = build_callstack_urls(ip, rank_count, base_port);
        
        assert_eq!(urls.len(), 4, "Should generate URL for each rank");
        assert_eq!(urls[0], "http://192.168.1.100:9933/apis/pythonext/callstack");
        assert_eq!(urls[1], "http://192.168.1.100:9934/apis/pythonext/callstack");
        assert_eq!(urls[2], "http://192.168.1.100:9935/apis/pythonext/callstack");
        assert_eq!(urls[3], "http://192.168.1.100:9936/apis/pythonext/callstack");
    }

    #[test]
    fn test_build_callstack_urls_zero_ranks() {
        let urls = build_callstack_urls("192.168.1.1", 0, 9933);
        assert_eq!(urls.len(), 0, "Should return empty vector for 0 ranks");
    }

    #[test]
    fn test_build_callstack_urls_many_ranks() {
        let urls = build_callstack_urls("192.168.1.1", 8, 9933);
        assert_eq!(urls.len(), 8, "Should handle 8 ranks");
        assert_eq!(urls[7], "http://192.168.1.1:9940/apis/pythonext/callstack");
    }

    #[test]
    fn test_build_callstack_urls_different_ip_formats() {
        let urls = build_callstack_urls("10.0.0.1", 2, 8000);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].starts_with("http://10.0.0.1:8000"));
        assert!(urls[1].starts_with("http://10.0.0.1:8001"));
    }
}
