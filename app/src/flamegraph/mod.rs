pub mod flamegraph_generator;
mod process_data;
pub mod stack_collector;
pub mod stack_merger;

use flamegraph_generator::generate_flamegraph_svg;
#[allow(unused_imports)]
use process_data::process_callstacks;
pub use process_data::process_callstacks_batch;
#[allow(deprecated)]
#[allow(unused_imports)]
use stack_collector::fetch_and_save_urls;
use stack_collector::fetch_urls_batched;
#[allow(unused_imports)]
use stack_merger::merge_stacks;
use stack_merger::StackTrie;

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// 配置文件路径的默认值
const DEFAULT_CONFIG_PATH: &str = "./config/collector.json";

/// 从环境变量获取配置文件路径，如未设置则使用默认值
pub fn get_config_path() -> String {
    std::env::var("COLLECTOR_CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string())
}

/// Default batch size for processing URLs
const DEFAULT_BATCH_SIZE: usize = 500;

/// Simple collector config: only the base port for rank 0's callstack endpoint.
#[derive(Deserialize)]
pub struct CollectorConfig {
    pub callstack_base_port: u16,
    #[serde(default = "default_step_query_port_offset")]
    pub step_query_port_offset: u16,
    /// Batch size for fetching and processing stacks. Default is 500.
    /// Smaller values reduce peak memory usage but may increase processing time.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// 训练平台 API 地址，用于 HANG 告警时查询任务/用户信息
    #[serde(default)]
    pub job_platform_api_url: String,
    /// 训练平台 appKey，用于获取 accessToken
    #[serde(default)]
    pub job_platform_app_key: String,
    /// 训练平台 appSecret，用于获取 accessToken
    #[serde(default)]
    pub job_platform_app_secret: String,
    /// 训练平台 userId，用于查询任务详情
    #[serde(default)]
    pub job_platform_user_id: String,
}

fn default_step_query_port_offset() -> u16 {
    1 // 默认偏移量为 1
}

fn default_batch_size() -> usize {
    DEFAULT_BATCH_SIZE
}

/// Load collector config from config file.
/// Path is determined by `COLLECTOR_CONFIG_PATH` env var, defaults to `./config/collector.json`.
pub fn load_collector_config(
    config_path: &str,
) -> Result<CollectorConfig, Box<dyn std::error::Error>> {
    let mut file = File::open(config_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: CollectorConfig = serde_json::from_str(&contents)?;
    Ok(config)
}

/// Build callstack URL for a single rank, port = base_port + local_rank.
pub fn build_callstack_url(ip: &str, local_rank: u8, base_port: u16) -> String {
    format!(
        "http://{}:{}/apis/pythonext/callstack",
        ip,
        base_port + local_rank as u16
    )
}

/// Build a callstack URL from probing's advertised endpoint.
/// Keeps the advertised port; only an unspecified host is replaced with the reachable node IP.
pub fn build_registered_callstack_url(
    probe_addr: &str,
    fallback_ip: &str,
    local_rank: u8,
    base_port: u16,
) -> String {
    let raw = probe_addr
        .trim()
        .trim_matches(['\'', '"'])
        .trim_end_matches('/');
    let without_scheme = raw.strip_prefix("http://").unwrap_or(raw);
    let authority = without_scheme.split('/').next().unwrap_or_default();

    if let Ok(socket_addr) = authority.parse::<std::net::SocketAddr>() {
        let authority = if socket_addr.ip().is_unspecified() {
            format_host_port(fallback_ip, socket_addr.port())
        } else {
            socket_addr.to_string()
        };
        return format!("http://{}/apis/pythonext/callstack", authority);
    }

    if authority
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .is_some()
    {
        return format!("http://{}/apis/pythonext/callstack", authority);
    }

    build_callstack_url(fallback_ip, local_rank, base_port)
}

fn format_host_port(host: &str, port: u16) -> String {
    let host = host.trim().trim_matches(['[', ']']);
    if host.contains(':') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}
/// Build callstack URLs for a node: one URL per rank, port = base_port + local_rank.
pub fn build_callstack_urls(ip: &str, rank_count: u8, base_port: u16) -> Vec<String> {
    (0..rank_count)
        .map(|local_rank| build_callstack_url(ip, local_rank, base_port))
        .collect()
}

/// 根据 world size 和 global_rank - local_rank 节点基址推导每节点固定 Rank 槽位数。
pub fn infer_regular_ranks_per_node(
    observed_ranks: &[(u32, u8, String)],
    world_size: u32,
) -> Result<u8, String> {
    if world_size == 0 {
        return Err("world_size must be greater than zero".to_string());
    }
    let node_bases = observed_ranks
        .iter()
        .filter(|(rank, _, node_ip)| *rank < world_size && !node_ip.trim().is_empty())
        .filter_map(|(rank, local_rank, _)| rank.checked_sub(*local_rank as u32))
        .collect::<BTreeSet<_>>();
    if node_bases.is_empty() {
        return Err("no training nodes observed".to_string());
    }
    let node_count = node_bases.len() as u32;
    if world_size % node_count != 0 {
        return Err(format!(
            "world_size {} cannot be evenly distributed across {} observed training nodes",
            world_size, node_count
        ));
    }
    let ranks_per_node = world_size / node_count;
    u8::try_from(ranks_per_node).map_err(|_| format!("invalid ranks per node: {}", ranks_per_node))
}

/// 按训练 Rank 的规则分布恢复全局火焰图采集目标。
/// probing 注册仅用于发现节点及 global-rank 基址，不使用逐 Rank 的
/// addr/status 删减采集目标。STOP/offline 槽位仍会发起请求，由超时处理。
pub fn build_regular_ranked_callstack_urls(
    observed_ranks: &[(u32, u8, String)],
    world_size: u32,
    base_port: u16,
) -> Result<Vec<(u32, String)>, String> {
    if world_size == 0 {
        return Err("world_size must be greater than zero".to_string());
    }

    let mut node_votes = BTreeMap::<u32, HashMap<String, usize>>::new();
    for (global_rank, local_rank, node_ip) in observed_ranks {
        if *global_rank >= world_size || node_ip.trim().is_empty() {
            continue;
        }
        let Some(node_rank_base) = global_rank.checked_sub(*local_rank as u32) else {
            continue;
        };
        *node_votes
            .entry(node_rank_base)
            .or_default()
            .entry(node_ip.clone())
            .or_default() += 1;
    }
    if node_votes.is_empty() {
        return Ok(Vec::new());
    }

    let ranks_per_node = infer_regular_ranks_per_node(observed_ranks, world_size)? as u32;

    let mut targets = Vec::with_capacity(world_size as usize);
    for (node_rank_base, votes) in node_votes {
        if node_rank_base % ranks_per_node != 0 {
            return Err(format!(
                "rank base {} does not align with {} ranks per node",
                node_rank_base, ranks_per_node
            ));
        }
        let node_ip = votes
            .into_iter()
            .max_by(|(left_ip, left_count), (right_ip, right_count)| {
                left_count
                    .cmp(right_count)
                    .then_with(|| right_ip.cmp(left_ip))
            })
            .map(|(ip, _)| ip)
            .expect("node votes cannot be empty");

        for local_rank in 0..ranks_per_node {
            let global_rank = node_rank_base + local_rank;
            if global_rank >= world_size {
                break;
            }
            targets.push((
                global_rank,
                build_callstack_url(&node_ip, local_rank as u8, base_port),
            ));
        }
    }
    targets.sort_by_key(|(rank, _)| *rank);
    Ok(targets)
}

/// Collect stacks for the given node, generate a flamegraph SVG, and return it as a String.
/// Uses batched processing to reduce memory pressure when handling large numbers of ranks.
///
/// # Arguments
/// * `node_ip` - The IP address of the node (used for logging)
/// * `urls` - List of URLs to fetch stack data from
/// * `batch_size` - Optional batch size. If None, uses DEFAULT_BATCH_SIZE (500).
///
/// # Memory Usage
/// Peak memory is proportional to batch_size rather than total URL count.
/// With batch_size=500 and ~50KB per stack, expect ~25MB peak for JSON data.
pub async fn collect_and_generate_flamegraph(
    _node_ip: &str,
    urls: Vec<String>,
    batch_size: Option<usize>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let batch_size = batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
    let total_ranks = urls.len() as u32;

    // Create shared Trie that will be incrementally built
    let trie = Arc::new(Mutex::new(StackTrie::with_total_ranks(total_ranks)));

    // Process URLs in batches
    let trie_clone = trie.clone();
    fetch_urls_batched(urls, batch_size, 4, move |batch| {
        let trie_inner = trie_clone.clone();
        async move {
            // Process this batch of JSON data into folded stack strings
            let processed = process_callstacks_batch(batch);

            // Insert processed stacks into the shared Trie
            let stacks_refs: Vec<(u32, &str)> = processed
                .iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect();

            let mut trie_guard = trie_inner.lock().map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to acquire trie lock: {}", e),
                )) as Box<dyn std::error::Error + Send + Sync>
            })?;
            trie_guard.insert_batch(stacks_refs);

            Ok(())
        }
    })
    .await?;

    // Generate folded format from the completed Trie
    let folded = {
        let trie_guard = trie.lock().map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to acquire trie lock: {}", e),
            )) as Box<dyn std::error::Error + Send + Sync>
        })?;

        let mut folded_buf = Vec::new();
        for (path, rank_str) in trie_guard.traverse_with_all_stack(&trie_guard.root, Vec::new()) {
            writeln!(folded_buf, "{} {} 1", path.join(";"), rank_str)?;
        }
        String::from_utf8_lossy(&folded_buf).into_owned()
    };

    // Generate SVG
    let svg = generate_flamegraph_svg(&folded).map_err(|e| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to generate flamegraph SVG: {}", e),
        )) as Box<dyn std::error::Error + Send + Sync>
    })?;
    Ok(svg)
}

/// Collect a flamegraph while preserving explicit global rank IDs.
///
/// ranked_urls maps each request to its global rank. rank_universe controls the
/// missing-rank side of labels and should normally contain every original rank,
/// including offline or unregistered ranks.
pub async fn collect_and_generate_ranked_flamegraph(
    ranked_urls: Vec<(u32, String)>,
    rank_universe: Vec<u32>,
    batch_size: Option<usize>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let batch_size = batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
    let (request_rank_ids, urls): (Vec<_>, Vec<_>) = ranked_urls.into_iter().unzip();
    let rank_universe = if rank_universe.is_empty() {
        request_rank_ids.clone()
    } else {
        rank_universe
    };

    let trie = Arc::new(Mutex::new(StackTrie::with_ranks(rank_universe)));
    let trie_clone = trie.clone();
    let index_to_rank = Arc::new(request_rank_ids);

    fetch_urls_batched(urls, batch_size, 4, move |batch| {
        let trie_inner = trie_clone.clone();
        let index_to_rank = index_to_rank.clone();
        async move {
            let processed = process_callstacks_batch(batch);
            let translated = remap_processed_stacks(processed, &index_to_rank);
            let stacks_refs = translated
                .iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect::<Vec<_>>();

            let mut trie_guard = trie_inner.lock().map_err(|error| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to acquire trie lock: {}", error),
                )) as Box<dyn std::error::Error + Send + Sync>
            })?;
            trie_guard.insert_batch(stacks_refs);
            Ok(())
        }
    })
    .await?;

    let folded = {
        let trie_guard = trie.lock().map_err(|error| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to acquire trie lock: {}", error),
            )) as Box<dyn std::error::Error + Send + Sync>
        })?;
        let mut folded_buf = Vec::new();
        for (path, rank_str) in trie_guard.traverse_with_all_stack(&trie_guard.root, Vec::new()) {
            writeln!(folded_buf, "{} {} 1", path.join(";"), rank_str)?;
        }
        String::from_utf8_lossy(&folded_buf).into_owned()
    };

    generate_flamegraph_svg(&folded).map_err(|error| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to generate flamegraph SVG: {}", error),
        )) as Box<dyn std::error::Error + Send + Sync>
    })
}

fn remap_processed_stacks(
    processed: Vec<(u32, String)>,
    request_rank_ids: &[u32],
) -> Vec<(u32, String)> {
    processed
        .into_iter()
        .filter_map(|(request_index, stack)| {
            request_rank_ids
                .get(request_index as usize)
                .copied()
                .map(|global_rank| (global_rank, stack))
        })
        .collect()
}
/// Legacy version of collect_and_generate_flamegraph without batch_size parameter.
/// Uses the default batch size of 500.
#[deprecated(
    since = "0.1.0",
    note = "Use collect_and_generate_flamegraph with batch_size parameter for explicit control"
)]
pub async fn collect_and_generate_flamegraph_legacy(
    node_ip: &str,
    urls: Vec<String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    collect_and_generate_flamegraph(node_ip, urls, None).await
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
        assert_eq!(
            config.callstack_base_port, 9933,
            "Should parse port correctly"
        );
        // Verify default batch_size is applied
        assert_eq!(config.batch_size, DEFAULT_BATCH_SIZE);

        // Cleanup
        let _ = fs::remove_file(temp_file);
    }

    #[test]
    fn test_load_collector_config_with_batch_size() {
        let config_content = r#"{"callstack_base_port": 9933, "batch_size": 100}"#;
        let temp_file = "/tmp/test_collector_config_batch.json";
        fs::write(temp_file, config_content).expect("Failed to write test config");

        let result = load_collector_config(temp_file);
        assert!(result.is_ok(), "Should load valid config successfully");

        let config = result.unwrap();
        assert_eq!(config.batch_size, 100, "Should parse batch_size correctly");

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
    fn regular_topology_restores_missing_and_offline_rank_slots() {
        let mut observed = Vec::new();
        for node in 0..4u32 {
            for local_rank in 0..8u8 {
                let global_rank = node * 8 + local_rank as u32;
                if global_rank == 0 || global_rank == 18 {
                    continue;
                }
                observed.push((global_rank, local_rank, format!("10.0.0.{}", node + 1)));
            }
        }

        let targets = build_regular_ranked_callstack_urls(&observed, 32, 9933).unwrap();

        assert_eq!(targets.len(), 32);
        assert_eq!(
            targets[0],
            (
                0,
                "http://10.0.0.1:9933/apis/pythonext/callstack".to_string()
            )
        );
        assert_eq!(
            targets[18],
            (
                18,
                "http://10.0.0.3:9935/apis/pythonext/callstack".to_string()
            )
        );
    }

    #[test]
    fn regular_topology_rejects_incomplete_node_discovery() {
        let observed = vec![
            (0, 0, "10.0.0.1".to_string()),
            (8, 0, "10.0.0.2".to_string()),
            (16, 0, "10.0.0.3".to_string()),
        ];

        let error = build_regular_ranked_callstack_urls(&observed, 32, 9933).unwrap_err();
        assert!(error.contains("cannot be evenly distributed"));
    }

    #[test]
    fn test_build_callstack_urls() {
        let ip = "192.168.1.100";
        let rank_count = 4;
        let base_port = 9933;

        let urls = build_callstack_urls(ip, rank_count, base_port);

        assert_eq!(urls.len(), 4, "Should generate URL for each rank");
        assert_eq!(
            urls[0],
            "http://192.168.1.100:9933/apis/pythonext/callstack"
        );
        assert_eq!(
            urls[1],
            "http://192.168.1.100:9934/apis/pythonext/callstack"
        );
        assert_eq!(
            urls[2],
            "http://192.168.1.100:9935/apis/pythonext/callstack"
        );
        assert_eq!(
            urls[3],
            "http://192.168.1.100:9936/apis/pythonext/callstack"
        );
    }

    #[test]
    fn test_build_callstack_url() {
        let url = build_callstack_url("10.0.0.1", 7, 8000);
        assert_eq!(url, "http://10.0.0.1:8007/apis/pythonext/callstack");
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

    #[test]
    fn remaps_request_indexes_to_stable_global_ranks() {
        let processed = vec![
            (0, "main;rank0".to_string()),
            (2, "main;rank2".to_string()),
            (9, "ignored".to_string()),
        ];

        assert_eq!(
            remap_processed_stacks(processed, &[1, 18, 20]),
            vec![
                (1, "main;rank0".to_string()),
                (20, "main;rank2".to_string()),
            ]
        );
    }

    #[test]
    fn explicit_rank_universe_reports_real_missing_ranks() {
        let mut trie = StackTrie::with_ranks((0..4).collect());
        trie.insert_batch(vec![(1, "main"), (3, "main")]);

        let paths = trie.traverse_with_all_stack(&trie.root, Vec::new());

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].1, "@1/3|0/2");
    }
}
