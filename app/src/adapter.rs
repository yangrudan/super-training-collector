#[cfg(feature = "ssr")]
use reqwest::Error;
#[cfg(feature = "ssr")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "ssr")]
use crate::models::{RankMetrics, NodeMetrics, GlobalMetrics, HealthStatus};
#[cfg(feature = "ssr")]
use crate::flamegraph::load_collector_config;

#[cfg(feature = "ssr")]
use std::collections::HashMap;

#[cfg(feature = "ssr")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "ssr")]
#[derive(Debug, Deserialize, Serialize)]
pub struct NodeInfo {
    pub host: String,
    pub addr: String,
    pub local_rank: u32,
    pub rank: u32,
    pub world_size: u32,
    pub group_rank: u32,
    pub group_world_size: u32,
    pub role_name: String,
    pub role_rank: u32,
    pub role_world_size: u32,
    pub status: String,
    pub timestamp: u64,
}

#[cfg(feature = "ssr")]
pub async fn get_node_info(url: &str) -> Result<Vec<NodeInfo>, Error> {
    let resp = reqwest::get(url).await?;
    let data: Vec<NodeInfo> = resp.json().await?;
    Ok(data)
}

#[cfg(feature = "ssr")]
#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;

    #[tokio::test]
    async fn test_get_node_info_with_real_api() {
        let port = load_collector_config("./config/collector.json")
            .map(|c| c.callstack_base_port)
            .unwrap_or(9933);
        let host = std::env::var("MASTER_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
        let url = format!("http://{}:{}/apis/nodes", host, port);
        let result = get_node_info(&url).await;
        
        match result {
            Ok(nodes) => {
                println!("Successfully parsed {} nodes", nodes.len());
                for node in &nodes {
                    println!("Host: {}, Rank: {}, Status: {}, Address: {}", 
                        node.host, node.rank, node.status, node.addr);
                }
                // 基本的验证
                assert!(!nodes.is_empty());
                // 验证每个节点都有必要的字段
                for node in nodes {
                    assert!(!node.host.is_empty());
                    assert!(!node.addr.is_empty());
                    assert!(!node.status.is_empty());
                    assert!(!node.role_name.is_empty());
                }
            },
            Err(e) => {
                panic!("Failed to get node info: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_real_data_conversion() {
        let result = get_real_training_data().await;
        
        match result {
            Ok((ranks, nodes)) => {
                println!("Converted {} ranks into {} nodes", ranks.len(), nodes.len());
                
                // 验证数据转换
                assert!(!ranks.is_empty());
                assert!(!nodes.is_empty());
                
                // 验证rank数据
                for rank in &ranks {
                    assert!(rank.rank_id < 1000); // 合理的rank ID
                    assert!(!rank.node_ip.is_empty());
                    assert!(matches!(rank.status, HealthStatus::Healthy | HealthStatus::Warning | HealthStatus::Critical));
                    println!("Rank {}: IP={}, Status={:?}", rank.rank_id, rank.node_ip, rank.status);
                }
                
                // 验证node数据
                for node in &nodes {
                    assert!(!node.node_ip.is_empty());
                    assert!(node.rank_count > 0);
                    println!("Node {}: {} ranks, status={:?}", node.node_ip, node.rank_count, node.status);
                }
            },
            Err(e) => {
                panic!("Failed to get real training data: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_get_node_info_with_mock() {
        // 使用mock测试，基于真实数据格式
        let port = load_collector_config("./config/collector.json")
            .map(|c| c.callstack_base_port)
            .unwrap_or(9933);
        let expected_json = format!(r#"[{{
            "host": "test-host",
            "addr": "0.0.0.0:{}",
            "local_rank": 0,
            "rank": 0,
            "world_size": 1,
            "group_rank": 0,
            "group_world_size": 1,
            "role_name": "default",
            "role_rank": 0,
            "role_world_size": 1,
            "status": "running",
            "timestamp": 1772605483868205
        }}]"#, port);
        
        let _m = mock("GET", "/apis/nodes")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&expected_json)
            .create();

        let url = &mockito::server_url();
        let full_url = format!("{}/apis/nodes", url);
        let result = get_node_info(&full_url).await;
        
        match result {
            Ok(nodes) => {
                assert_eq!(nodes.len(), 1);
                let node = &nodes[0];
                assert_eq!(node.host, "test-host");
                assert_eq!(node.rank, 0);
                assert_eq!(node.status, "running");
                assert_eq!(node.role_name, "default");
            },
            Err(e) => panic!("Test failed with error: {}", e),
        }
    }
}

#[cfg(feature = "ssr")]
/// 从NodeInfo地址中提取IP，如果是 0.0.0.0 则对 host 做 DNS 解析获取真实 IP
fn extract_ip_from_addr(addr: &str, host: &str) -> String {
    let ip = addr.split(':').next().unwrap_or(addr);
    if ip == "0.0.0.0" {
        // host 可能是主机名，尝试 DNS 解析
        use std::net::ToSocketAddrs;
        format!("{}:0", host)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .map(|sa| sa.ip().to_string())
            .unwrap_or_else(|| host.to_string())
    } else {
        ip.to_string()
    }
}

#[cfg(feature = "ssr")]
/// 将status字符串转换为HealthStatus枚举
fn convert_status(status: &str) -> HealthStatus {
    match status.to_lowercase().as_str() {
        "running" => HealthStatus::Healthy,
        "error" | "failed" | "critical" => HealthStatus::Critical,
        _ => HealthStatus::Warning,
    }
}

#[cfg(feature = "ssr")]

#[cfg(feature = "ssr")]
/// 将NodeInfo转换为RankMetrics
pub fn convert_node_info_to_rank_metrics(node_info: NodeInfo) -> RankMetrics {
    RankMetrics {
        rank_id: node_info.rank,
        local_rank: node_info.local_rank as u8,
        node_ip: extract_ip_from_addr(&node_info.addr, &node_info.host),
        hostname: node_info.host.clone(),
        
        // 基础状态信息
        status: convert_status(&node_info.status),
        last_heartbeat: node_info.timestamp / 1_000_000, // 微秒转秒
        
        // 使用默认值的性能指标（后续集成真实API时替换）
        step_time_ms: 100.0,          // 默认步时间
        step_time_ratio: 1.0,         // 默认比率
        gpu_utilization: 90.0,        // 默认GPU利用率
        gpu_memory_used_gb: 16.0,     // 默认显存占用
        gpu_memory_total_gb: 32.0,    // 默认显存总量
        nccl_latency_ms: 5.0,         // 默认NCCL延迟
        nccl_bandwidth_gbps: 100.0,   // 默认NCCL带宽
        current_step: 0,              // 默认训练步数
        error_message: None,          // 默认无错误
    }
}

#[cfg(feature = "ssr")]
/// 从RankMetrics聚合生成NodeMetrics
pub fn aggregate_ranks_to_node_metrics(node_ip: &str, ranks: &[RankMetrics]) -> Option<NodeMetrics> {
    if ranks.is_empty() {
        return None;
    }

    let hostname = ranks[0].hostname.clone(); // 使用 NodeInfo.host 字段
    // 修复rack_id计算，避免溢出
    let last_octet = node_ip.split('.').last()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);
    let rack_id = format!("rack-{}", last_octet / 4 + 1);
    
    let rank_count = ranks.len() as u8;
    let healthy_count = ranks.iter().filter(|r| r.status == HealthStatus::Healthy).count() as u8;
    let warning_count = ranks.iter().filter(|r| r.status == HealthStatus::Warning).count() as u8;
    let critical_count = ranks.iter().filter(|r| r.status == HealthStatus::Critical).count() as u8;

    let slow_ratio = warning_count as f32 / rank_count as f32;
    let avg_step_time_ms = ranks.iter().map(|r| r.step_time_ms).sum::<f64>() / rank_count as f64;
    
    let mut step_times: Vec<f64> = ranks.iter().map(|r| r.step_time_ms).collect();
    step_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50_step_time_ms = step_times[step_times.len() / 2];
    let p99_step_time_ms = step_times[(step_times.len() * 99 / 100).min(step_times.len() - 1)];
    
    let avg_gpu_utilization = ranks.iter().map(|r| r.gpu_utilization).sum::<f32>() / rank_count as f32;
    let avg_nccl_latency_ms = ranks.iter().map(|r| r.nccl_latency_ms).sum::<f64>() / rank_count as f64;

    let status = if critical_count > 0 {
        HealthStatus::Critical
    } else if warning_count > rank_count / 2 {
        HealthStatus::Warning
    } else {
        HealthStatus::Healthy
    };

    let last_update = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Some(NodeMetrics {
        node_ip: node_ip.to_string(),
        hostname,
        rack_id,
        rank_count,
        healthy_count,
        warning_count,
        critical_count,
        slow_ratio,
        avg_step_time_ms,
        p50_step_time_ms,
        p99_step_time_ms,
        avg_gpu_utilization,
        avg_nccl_latency_ms,
        status,
        last_update,
    })
}

#[cfg(feature = "ssr")]
/// 获取真实数据并转换为应用所需格式
pub async fn get_real_training_data() -> Result<(Vec<RankMetrics>, Vec<NodeMetrics>), Error> {
    let port = load_collector_config("./config/collector.json")
        .map(|c| c.callstack_base_port)
        .unwrap_or(9933);
    let host = std::env::var("MASTER_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
    let url = format!("http://{}:{}/apis/nodes", host, port);
    let node_infos = get_node_info(&url).await?;
    
    // 转换为RankMetrics
    let ranks: Vec<RankMetrics> = node_infos
        .into_iter()
        .map(convert_node_info_to_rank_metrics)
        .collect();
    
    // 按节点IP分组并聚合为NodeMetrics
    let mut nodes_map: HashMap<String, Vec<RankMetrics>> = HashMap::new();
    for rank in &ranks {
        nodes_map.entry(rank.node_ip.clone())
            .or_insert_with(Vec::new)
            .push(rank.clone());
    }
    
    let nodes: Vec<NodeMetrics> = nodes_map
        .iter()
        .filter_map(|(node_ip, ranks)| aggregate_ranks_to_node_metrics(node_ip, ranks))
        .collect();
    
    Ok((ranks, nodes))
}

#[cfg(feature = "ssr")]
/// 生成全局聚合指标
pub fn generate_global_metrics_from_real_data(nodes: &[NodeMetrics], ranks: &[RankMetrics]) -> GlobalMetrics {
    let total_nodes = nodes.len() as u16;
    let total_ranks = ranks.len() as u16;

    let healthy_nodes = nodes.iter().filter(|n| n.status == HealthStatus::Healthy).count() as u16;
    let warning_nodes = nodes.iter().filter(|n| n.status == HealthStatus::Warning).count() as u16;
    let critical_nodes = nodes.iter().filter(|n| n.status == HealthStatus::Critical).count() as u16;

    let healthy_ranks = ranks.iter().filter(|r| r.status == HealthStatus::Healthy).count() as u16;
    let warning_ranks = ranks.iter().filter(|r| r.status == HealthStatus::Warning).count() as u16;
    let critical_ranks = ranks.iter().filter(|r| r.status == HealthStatus::Critical).count() as u16;

    let mut all_step_times: Vec<f64> = ranks.iter().map(|r| r.step_time_ms).collect();
    all_step_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let global_p50_step_time_ms = if !all_step_times.is_empty() {
        all_step_times[all_step_times.len() / 2]
    } else {
        100.0
    };
    
    let global_p99_step_time_ms = if !all_step_times.is_empty() {
        all_step_times[(all_step_times.len() * 99 / 100).min(all_step_times.len() - 1)]
    } else {
        120.0
    };

    let global_avg_gpu_utilization = if total_ranks > 0 {
        ranks.iter().map(|r| r.gpu_utilization).sum::<f32>() / total_ranks as f32
    } else {
        0.0
    };

    let slow_node_count = nodes.iter().filter(|n| n.slow_ratio > 0.1).count();
    let slow_node_ratio = if total_nodes > 0 {
        slow_node_count as f32 / total_nodes as f32
    } else {
        0.0
    };

    let current_step = ranks.iter().map(|r| r.current_step).max().unwrap_or(0);
    let steps_per_second = 10.0; // 默认值
    let estimated_remaining_hours = Some(1.5); // 默认值

    let last_update = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    GlobalMetrics {
        total_nodes,
        total_ranks,
        healthy_nodes,
        warning_nodes,
        critical_nodes,
        healthy_ranks,
        warning_ranks,
        critical_ranks,
        global_p50_step_time_ms,
        global_p99_step_time_ms,
        global_avg_gpu_utilization,
        slow_node_ratio,
        current_step,
        steps_per_second,
        estimated_remaining_hours,
        last_update,
    }
}
