use crate::models::*;
#[cfg(feature = "ssr")]
use crate::mock::MockDataStore;
#[cfg(feature = "ssr")]
use crate::adapter::{get_real_training_data, generate_global_metrics_from_real_data};
use leptos::prelude::*;

/// 获取全局聚合指标 (Level 1)
#[server(GetGlobalMetrics)]
pub async fn get_global_metrics() -> Result<GlobalMetrics, ServerFnError> {
    leptos::logging::log!("Getting global metrics from real API...");
    match get_real_training_data().await {
        Ok((ranks, nodes)) => {
            leptos::logging::log!("Real API success: {} ranks, {} nodes", ranks.len(), nodes.len());
            let global_metrics = generate_global_metrics_from_real_data(&nodes, &ranks);
            Ok(global_metrics)
        }
        Err(e) => {
            // 如果真实API失败，回退到mock数据
            leptos::logging::log!("Failed to get real data, falling back to mock: {}", e);
            let store = MockDataStore::new();
            Ok(store.global)
        }
    }
}

/// 获取节点列表 (Level 2)
#[server(GetNodes)]
pub async fn get_nodes(
    sort_field: Option<SortField>,
    sort_order: Option<SortOrder>,
    status_filter: Option<StatusFilter>,
) -> Result<NodesResponse, ServerFnError> {
    leptos::logging::log!("Getting nodes from real API...");
    let nodes = match get_real_training_data().await {
        Ok((_, nodes)) => {
            leptos::logging::log!("Real API success: {} nodes", nodes.len());
            for node in &nodes {
                leptos::logging::log!("Node: IP={}, Ranks={}", node.node_ip, node.rank_count);
            }
            nodes
        },
        Err(e) => {
            // 如果真实API失败，回退到mock数据
            leptos::logging::log!("Failed to get real data, falling back to mock: {}", e);
            let store = MockDataStore::new();
            store.nodes
        }
    };

    let mut filtered_nodes = nodes;

    // 筛选
    if let Some(filter) = status_filter {
        filtered_nodes = match filter {
            StatusFilter::All => filtered_nodes,
            StatusFilter::Healthy => filtered_nodes.into_iter().filter(|n| n.status == HealthStatus::Healthy).collect(),
            StatusFilter::Warning => filtered_nodes.into_iter().filter(|n| n.status == HealthStatus::Warning).collect(),
            StatusFilter::Critical => filtered_nodes.into_iter().filter(|n| n.status == HealthStatus::Critical).collect(),
        };
    }

    // 排序
    let field = sort_field.unwrap_or_default();
    let order = sort_order.unwrap_or_default();

    filtered_nodes.sort_by(|a, b| {
        let cmp = match field {
            SortField::SlowRatio => a.slow_ratio.partial_cmp(&b.slow_ratio).unwrap(),
            SortField::StepTime => a.p99_step_time_ms.partial_cmp(&b.p99_step_time_ms).unwrap(),
            SortField::GpuUtilization => a.avg_gpu_utilization.partial_cmp(&b.avg_gpu_utilization).unwrap(),
            SortField::NcclLatency => a.avg_nccl_latency_ms.partial_cmp(&b.avg_nccl_latency_ms).unwrap(),
        };
        match order {
            SortOrder::Asc => cmp,
            SortOrder::Desc => cmp.reverse(),
        }
    });

    let total = filtered_nodes.len() as u16;
    Ok(NodesResponse { nodes: filtered_nodes, total })
}

/// 获取指定节点的 Rank 详情 (Level 3)
#[server(GetNodeRanks)]
pub async fn get_node_ranks(ip: String) -> Result<NodeRanksResponse, ServerFnError> {
    match get_real_training_data().await {
        Ok((ranks, nodes)) => {
            let node = nodes.into_iter()
                .find(|n| n.node_ip == ip)
                .ok_or_else(|| ServerFnError::new("Node not found"))?;

            let node_ranks: Vec<RankMetrics> = ranks.into_iter()
                .filter(|r| r.node_ip == ip)
                .collect();

            Ok(NodeRanksResponse { node, ranks: node_ranks })
        }
        Err(e) => {
            // 如果真实API失败，回退到mock数据
            leptos::logging::log!("Failed to get real data, falling back to mock: {}", e);
            let store = MockDataStore::new();
            
            let node = store.get_node_by_ip(&ip)
                .ok_or_else(|| ServerFnError::new("Node not found"))?;
            let ranks = store.get_ranks_by_ip(&ip);
            
            Ok(NodeRanksResponse { node, ranks })
        }
    }
}

/// 获取拓扑数据
#[server(GetTopology)]
pub async fn get_topology() -> Result<Topology, ServerFnError> {
    let store = MockDataStore::new();
    Ok(store.topology)
}

/// 获取指定节点的堆栈数据 (用于火焰图)
#[server(GetNodeStacks)]
pub async fn get_node_stacks(ip: String) -> Result<NodeStacksResponse, ServerFnError> {
    use crate::mock::{generate_node_stacks, merge_stacks};
    use std::time::{SystemTime, UNIX_EPOCH};

    let store = MockDataStore::new();
    let ranks = store.get_ranks_by_ip(&ip);

    if ranks.is_empty() {
        return Err(ServerFnError::new("Node not found"));
    }

    let stacks = generate_node_stacks(&ip, &ranks);
    let merged_root = merge_stacks(&stacks);

    let collected_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(NodeStacksResponse {
        node_ip: ip,
        stacks,
        merged_root,
        collected_at,
    })
}
