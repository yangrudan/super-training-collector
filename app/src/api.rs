use crate::models::*;
use crate::mock::MockDataStore;
use leptos::prelude::*;

/// 获取全局聚合指标 (Level 1)
#[server(GetGlobalMetrics)]
pub async fn get_global_metrics() -> Result<GlobalMetrics, ServerFnError> {
    let store = MockDataStore::new();
    Ok(store.global)
}

/// 获取节点列表 (Level 2)
#[server(GetNodes)]
pub async fn get_nodes(
    sort_field: Option<SortField>,
    sort_order: Option<SortOrder>,
    status_filter: Option<StatusFilter>,
) -> Result<NodesResponse, ServerFnError> {
    let store = MockDataStore::new();
    let mut nodes = store.nodes;

    // 筛选
    if let Some(filter) = status_filter {
        nodes = match filter {
            StatusFilter::All => nodes,
            StatusFilter::Healthy => nodes.into_iter().filter(|n| n.status == HealthStatus::Healthy).collect(),
            StatusFilter::Warning => nodes.into_iter().filter(|n| n.status == HealthStatus::Warning).collect(),
            StatusFilter::Critical => nodes.into_iter().filter(|n| n.status == HealthStatus::Critical).collect(),
        };
    }

    // 排序
    let field = sort_field.unwrap_or_default();
    let order = sort_order.unwrap_or_default();

    nodes.sort_by(|a, b| {
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

    let total = nodes.len() as u16;
    Ok(NodesResponse { nodes, total })
}

/// 获取指定节点的 Rank 详情 (Level 3)
#[server(GetNodeRanks)]
pub async fn get_node_ranks(ip: String) -> Result<NodeRanksResponse, ServerFnError> {
    let store = MockDataStore::new();

    let node = store.get_node_by_ip(&ip)
        .ok_or_else(|| ServerFnError::new("Node not found"))?;

    let ranks = store.get_ranks_by_ip(&ip);

    Ok(NodeRanksResponse { node, ranks })
}

/// 获取拓扑数据
#[server(GetTopology)]
pub async fn get_topology() -> Result<Topology, ServerFnError> {
    let store = MockDataStore::new();
    Ok(store.topology)
}
