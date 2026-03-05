mod stack_collector;
mod stack_merger;
mod process_data;
mod flamegraph_generator;

use stack_collector::fetch_and_save_urls;
use stack_merger::merge_stacks;
use process_data::process_callstacks;
use flamegraph_generator::generate_flamegraph_svg;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};

/// Config structure: maps node IP to a list of stack endpoint URLs.
/// Loaded from `config/flamegraph.json`.
pub type FlamegraphConfig = HashMap<String, Vec<String>>;

pub fn load_flamegraph_config(config_path: &str) -> Result<FlamegraphConfig, Box<dyn std::error::Error>> {
    let mut file = File::open(config_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: FlamegraphConfig = serde_json::from_str(&contents)?;
    Ok(config)
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
