//! HANG 检测运行时模块
//!
//! 提供异步运行时集成，用于服务端启动调度器

use super::config::HangConfig;
use super::detector::HangDetector;
use crate::adapter::get_real_training_data;
use crate::flamegraph::{build_callstack_urls, load_collector_config};
use std::time::Duration;
use tokio::time::interval;
use tracing;
use serde_json;

/// 启动 HANG 检测调度器
/// 
/// 这个函数应该在服务启动时被调用，使用 tokio::spawn 运行
pub async fn start_hang_detector_scheduler() {
    let config = HangConfig::from_env();
    
    if !config.enabled {
        tracing::info!("HANG detection is disabled");
        return;
    }
    
    tracing::info!("Starting HANG detection scheduler with interval: {}s", config.sample_interval_secs);
    
    let detector = HangDetector::new(config.clone());
    let mut tick = interval(Duration::from_secs(config.sample_interval_secs));
    
    loop {
        tick.tick().await;
        
        // 获取所有节点 IP 列表
        let all_nodes = match fetch_all_nodes().await {
            Ok(nodes) => {
                tracing::debug!("Fetched {} nodes", nodes.len());
                nodes
            }
            Err(e) => {
                tracing::warn!("Failed to fetch nodes: {}", e);
                detector.reset_round();
                continue;
            }
        };
        
        if all_nodes.is_empty() {
            tracing::warn!("No nodes available");
            detector.reset_round();
            continue;
        }
        
        // 检查是否需要选择新的节点
        if detector.needs_new_nodes() {
            let selected = detector.select_nodes(&all_nodes);
            detector.set_selected_nodes(selected.clone());
            tracing::debug!("Selected nodes for sampling: {:?}", selected);
        }
        
        let selected_nodes = detector.get_selected_nodes();
        detector.increment_sample_round();
        
        // 为每个选中的节点采集堆栈
        let mut results = Vec::new();
        for node_ip in selected_nodes {
            match fetch_stacks(&node_ip).await {
                Ok(stacks) => {
                    let (is_hang, similarity) = detector.process_node_stacks(&node_ip, stacks);
                    results.push((node_ip.clone(), is_hang, similarity));
                    tracing::debug!("Node {}: hang={}, similarity={:.3}", node_ip, is_hang, similarity);
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch stacks for node {}: {}", node_ip, e);
                }
            }
        }
        
        // 更新全局状态
        let status = detector.update_global_status(&results);
        tracing::info!("HANG detection round completed, status: {:?}", status);
    }
}

/// 获取所有节点 IP 列表
async fn fetch_all_nodes() -> Result<Vec<String>, String> {
    match get_real_training_data().await {
        Ok((_ranks, nodes)) => {
            let node_ips: Vec<String> = nodes.iter().map(|n| n.node_ip.clone()).collect();
            Ok(node_ips)
        }
        Err(e) => Err(format!("Failed to get real training data: {}", e))
    }
}

/// 从节点获取堆栈数据
/// 
/// 返回堆栈列表，每个堆栈对应一个 rank
async fn fetch_stacks(node_ip: &str) -> Result<Vec<Vec<String>>, String> {
    let config = load_collector_config("./config/collector.json")
        .map_err(|e| format!("Failed to load config: {}", e))?;
    
    // 获取该节点的 rank_count
    let rank_count = {
        match get_real_training_data().await {
            Ok((_ranks, nodes)) => {
                nodes
                    .iter()
                    .find(|n| n.node_ip == node_ip)
                    .map(|n| n.rank_count)
                    .unwrap_or(4)
            }
            Err(e) => {
                tracing::warn!("Failed to get real training data: {}", e);
                4
            }
        }
    };
    
    // 构建每个 rank 的 URL
    let urls = build_callstack_urls(node_ip, rank_count, config.callstack_base_port);
    
    // 并发获取各 rank 的堆栈
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    
    let mut stacks = Vec::new();
    
    for url in urls {
        match fetch_callstack(&client, &url).await {
            Ok(stack) => {
                stacks.push(stack);
            }
            Err(e) => {
                tracing::warn!("Failed to fetch from {}: {}", url, e);
                // 即使一个 rank 失败，也继续尝试其他 rank
                stacks.push(Vec::new());
            }
        }
    }
    
    Ok(stacks)
}

/// 从单个 URL 获取调用栈
async fn fetch_callstack(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<String>, String> {
    let response = client.get(url).send().await
        .map_err(|e| format!("HTTP request failed: {}", e))?;
    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    
    // 解析堆栈帧列表
    let mut frames = Vec::new();
    
    if let serde_json::Value::Array(frame_array) = json {
        for frame in frame_array {
            let frame_str = format_frame(&frame);
            if !frame_str.is_empty() {
                frames.push(frame_str);
            }
        }
    }
    
    Ok(frames)
}

/// 格式化单个堆栈帧
fn format_frame(frame: &serde_json::Value) -> String {
    let obj = match frame.as_object() {
        Some(o) => o,
        None => return String::new(),
    };
    
    // 尝试获取函数名和文件位置
    let func = obj
        .get("func")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    
    let file = obj
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    
    let lineno = obj
        .get("lineno")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    format!("{} ({}:{})", func, file, lineno)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_frame_complete() {
        let json = serde_json::json!({
            "func": "forward",
            "file": "model.py",
            "lineno": 42
        });
        
        let frame_str = format_frame(&json);
        assert_eq!(frame_str, "forward (model.py:42)");
    }

    #[test]
    fn test_format_frame_missing_fields() {
        let json = serde_json::json!({
            "func": "backward"
        });
        
        let frame_str = format_frame(&json);
        assert_eq!(frame_str, "backward (unknown:0)");
    }

    #[test]
    fn test_format_frame_empty() {
        let json = serde_json::json!(null);
        
        let frame_str = format_frame(&json);
        assert_eq!(frame_str, "");
    }
}
