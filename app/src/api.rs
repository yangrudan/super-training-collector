#[cfg(feature = "ssr")]
use crate::adapter::{generate_global_metrics_from_real_data, get_real_training_data};
#[cfg(feature = "ssr")]
use crate::mock::MockDataStore;
use crate::models::*;
use leptos::prelude::*;

/// 检查是否启用了 mock 模式
#[cfg(feature = "ssr")]
fn is_mock_mode() -> bool {
    std::env::var("COLLECTOR_MOCK_MODE")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false)
}

/// 获取全局聚合指标 (Level 1)
#[server(GetGlobalMetrics)]
pub async fn get_global_metrics() -> Result<GlobalMetrics, ServerFnError> {
    leptos::logging::log!("Getting global metrics from real API...");
    match get_real_training_data().await {
        Ok((ranks, nodes)) => {
            leptos::logging::log!(
                "Real API success: {} ranks, {} nodes",
                ranks.len(),
                nodes.len()
            );
            let global_metrics = generate_global_metrics_from_real_data(&nodes, &ranks);
            Ok(global_metrics)
        }
        Err(e) => {
            if is_mock_mode() {
                leptos::logging::log!("Mock mode enabled, using mock data");
                let store = MockDataStore::new();
                Ok(store.global)
            } else {
                leptos::logging::log!("Failed to get real data: {}", e);
                Err(ServerFnError::new(format!("无法连接训练集群: {}", e)))
            }
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
        }
        Err(e) => {
            if is_mock_mode() {
                leptos::logging::log!("Mock mode enabled, using mock data");
                let store = MockDataStore::new();
                store.nodes
            } else {
                leptos::logging::log!("Failed to get real data: {}", e);
                return Err(ServerFnError::new(format!("无法连接训练集群: {}", e)));
            }
        }
    };

    let mut filtered_nodes = nodes;

    // 筛选
    if let Some(filter) = status_filter {
        filtered_nodes = match filter {
            StatusFilter::All => filtered_nodes,
            StatusFilter::Healthy => filtered_nodes
                .into_iter()
                .filter(|n| n.status == HealthStatus::Healthy)
                .collect(),
            StatusFilter::Warning => filtered_nodes
                .into_iter()
                .filter(|n| n.status == HealthStatus::Warning)
                .collect(),
            StatusFilter::Critical => filtered_nodes
                .into_iter()
                .filter(|n| n.status == HealthStatus::Critical)
                .collect(),
        };
    }

    // 排序
    let field = sort_field.unwrap_or_default();
    let order = sort_order.unwrap_or_default();

    filtered_nodes.sort_by(|a, b| {
        let cmp = match field {
            SortField::SlowRatio => a.slow_ratio.partial_cmp(&b.slow_ratio).unwrap(),
            SortField::StepTime => a.p99_step_time_ms.partial_cmp(&b.p99_step_time_ms).unwrap(),
            SortField::GpuUtilization => a
                .avg_gpu_utilization
                .partial_cmp(&b.avg_gpu_utilization)
                .unwrap(),
            SortField::NcclLatency => a
                .avg_nccl_latency_ms
                .partial_cmp(&b.avg_nccl_latency_ms)
                .unwrap(),
        };
        match order {
            SortOrder::Asc => cmp,
            SortOrder::Desc => cmp.reverse(),
        }
    });

    let total = filtered_nodes.len() as u16;
    Ok(NodesResponse {
        nodes: filtered_nodes,
        total,
    })
}

/// 获取指定节点的 Rank 详情 (Level 3)
#[server(GetNodeRanks)]
pub async fn get_node_ranks(ip: String) -> Result<NodeRanksResponse, ServerFnError> {
    match get_real_training_data().await {
        Ok((ranks, nodes)) => {
            let node = nodes
                .into_iter()
                .find(|n| n.node_ip == ip)
                .ok_or_else(|| ServerFnError::new("Node not found"))?;

            let node_ranks: Vec<RankMetrics> =
                ranks.into_iter().filter(|r| r.node_ip == ip).collect();

            Ok(NodeRanksResponse {
                node,
                ranks: node_ranks,
            })
        }
        Err(e) => {
            if is_mock_mode() {
                leptos::logging::log!("Mock mode enabled, using mock data");
                let store = MockDataStore::new();

                let node = store
                    .get_node_by_ip(&ip)
                    .ok_or_else(|| ServerFnError::new("Node not found"))?;
                let ranks = store.get_ranks_by_ip(&ip);

                Ok(NodeRanksResponse { node, ranks })
            } else {
                leptos::logging::log!("Failed to get real data: {}", e);
                Err(ServerFnError::new(format!("无法连接训练集群: {}", e)))
            }
        }
    }
}

/// 获取拓扑数据
#[server(GetTopology)]
pub async fn get_topology() -> Result<Topology, ServerFnError> {
    let store = MockDataStore::new();
    Ok(store.topology)
}

/// 从 config/collector.json 获取所有节点及其 Rank URL 列表 (IP 来自训练数据, 端口自动递增)
#[server(GetAllNodesCallstackInfo)]
pub async fn get_all_nodes_callstack_info() -> Result<Vec<(String, u8, u16)>, ServerFnError> {
    use crate::flamegraph::{get_config_path, load_collector_config};

    let config = load_collector_config(&get_config_path())
        .map_err(|e| ServerFnError::new(format!("Failed to load collector config: {}", e)))?;

    let nodes = match get_real_training_data().await {
        Ok((_, nodes)) => nodes,
        Err(e) => {
            if is_mock_mode() {
                let store = MockDataStore::new();
                store.nodes
            } else {
                return Err(ServerFnError::new(format!("无法连接训练集群: {}", e)));
            }
        }
    };

    let mut result: Vec<(String, u8, u16)> = nodes
        .into_iter()
        .map(|n| (n.node_ip, n.rank_count, config.callstack_base_port))
        .collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// 获取指定节点的堆栈火焰图 SVG (端口从 config/collector.json 自动推算)
#[server(GetNodeFlamegraph)]
pub async fn get_node_flamegraph(ip: String) -> Result<String, ServerFnError> {
    use crate::flamegraph::{
        build_callstack_urls, collect_and_generate_flamegraph, get_config_path,
        load_collector_config,
    };

    let config = load_collector_config(&get_config_path())
        .map_err(|e| ServerFnError::new(format!("Failed to load collector config: {}", e)))?;

    // 获取该节点的 rank_count
    let rank_count = match get_real_training_data().await {
        Ok((_, nodes)) => nodes
            .into_iter()
            .find(|n| n.node_ip == ip)
            .map(|n| n.rank_count)
            .unwrap_or(4),
        Err(e) => {
            if is_mock_mode() {
                let store = MockDataStore::new();
                store.get_node_by_ip(&ip).map(|n| n.rank_count).unwrap_or(4)
            } else {
                return Err(ServerFnError::new(format!("无法连接训练集群: {}", e)));
            }
        }
    };

    let urls = build_callstack_urls(&ip, rank_count, config.callstack_base_port);

    let svg = collect_and_generate_flamegraph(&ip, urls, Some(config.batch_size))
        .await
        .map_err(|e| ServerFnError::new(format!("Failed to generate flamegraph: {}", e)))?;

    Ok(svg)
}
/// 获取所有节点全部 Rank 的堆栈，合并生成一张火焰图 SVG
#[server(GetAllNodesFlamegraph)]
pub async fn get_all_nodes_flamegraph() -> Result<String, ServerFnError> {
    use crate::flamegraph::{
        collect_and_generate_flamegraph, get_config_path, load_collector_config,
    };

    let config = load_collector_config(&get_config_path())
        .map_err(|e| ServerFnError::new(format!("Failed to load collector config: {}", e)))?;

    let ranks = match get_real_training_data().await {
        Ok((ranks, _)) => ranks,
        Err(e) => {
            if is_mock_mode() {
                // mock 模式回退：仍使用 nodes 构建 URL
                use crate::flamegraph::build_callstack_urls;
                let store = MockDataStore::new();
                let mut all_urls: Vec<String> = Vec::new();
                for node in &store.nodes {
                    let urls = build_callstack_urls(
                        &node.node_ip,
                        node.rank_count,
                        config.callstack_base_port,
                    );
                    all_urls.extend(urls);
                }
                if all_urls.is_empty() {
                    return Err(ServerFnError::new("No nodes found"));
                }
                let svg =
                    collect_and_generate_flamegraph("all_nodes", all_urls, Some(config.batch_size))
                        .await
                        .map_err(|e| {
                            ServerFnError::new(format!(
                                "Failed to generate combined flamegraph: {}",
                                e
                            ))
                        })?;
                return Ok(svg);
            } else {
                return Err(ServerFnError::new(format!("无法连接训练集群: {}", e)));
            }
        }
    };

    // 按 rank_id 排序构建 URL，确保 URL index 与全局 rank ID 一致
    // ranks 已在 get_real_training_data() 中按 rank_id 排序
    let all_urls: Vec<String> = ranks
        .iter()
        .map(|r| {
            format!(
                "http://{}:{}/apis/pythonext/callstack",
                r.node_ip,
                config.callstack_base_port + r.local_rank as u16
            )
        })
        .collect();

    if all_urls.is_empty() {
        return Err(ServerFnError::new("No nodes found"));
    }

    let svg = collect_and_generate_flamegraph("all_nodes", all_urls, Some(config.batch_size))
        .await
        .map_err(|e| {
            ServerFnError::new(format!("Failed to generate combined flamegraph: {}", e))
        })?;

    Ok(svg)
}

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

/// 获取当前是否处于 mock 模式
#[server(GetMockModeStatus)]
pub async fn get_mock_mode_status() -> Result<bool, ServerFnError> {
    Ok(is_mock_mode())
}

// ============ Step 指标 API (Phase 2) ============

/// 检查是否启用了 Step 显示功能
#[server(GetStepShowEnabled)]
pub async fn get_step_show_enabled() -> Result<bool, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::adapter::is_step_show_enabled;
        Ok(is_step_show_enabled())
    }
    #[cfg(not(feature = "ssr"))]
    {
        Ok(false)
    }
}

/// 获取全局 Step 指标（首页使用）
#[server(GetGlobalStepMetrics)]
pub async fn get_global_step_metrics() -> Result<crate::models::GlobalStepMetrics, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::adapter::{get_global_step_metrics as fetch_global_step, is_step_show_enabled};

        if !is_step_show_enabled() {
            return Err(ServerFnError::new("Step 显示功能未启用 (STEP_SHOW=true)"));
        }

        match fetch_global_step().await {
            Ok(metrics) => Ok(metrics),
            Err(e) => Err(ServerFnError::new(format!("获取 Step 指标失败: {}", e))),
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::new("SSR feature required"))
    }
}

/// 获取指定 Rank 的 Step 指标
#[server(GetRankStepMetrics)]
pub async fn get_rank_step_metrics(
    ip: String,
    local_rank: u8,
    rank_id: u32,
) -> Result<crate::models::RankStepMetrics, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::adapter::{get_rank_step_metrics as fetch_rank_step, is_step_show_enabled};

        if !is_step_show_enabled() {
            return Err(ServerFnError::new("Step 显示功能未启用 (STEP_SHOW=true)"));
        }

        match fetch_rank_step(&ip, local_rank, rank_id).await {
            Ok(metrics) => Ok(metrics),
            Err(e) => Err(ServerFnError::new(format!(
                "获取 Rank Step 指标失败: {}",
                e
            ))),
        }
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::new("SSR feature required"))
    }
}

// ============ HANG 检测 API (Phase 3) ============

/// 获取 HANG 检测状态
#[server(GetHangStatus)]
pub async fn get_hang_status() -> Result<crate::hang_types::HangStatusSnapshot, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::hang_detector::state::get_hang_state;

        let state = get_hang_state();
        let state = state
            .read()
            .map_err(|e| ServerFnError::new(format!("获取 HANG 状态失败: {}", e)))?;

        Ok(state.snapshot())
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::new("SSR feature required"))
    }
}

/// 检查 HANG 检测是否启用
#[server(GetHangCheckEnabled)]
pub async fn get_hang_check_enabled() -> Result<bool, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::hang_detector::config::HangConfig;
        Ok(HangConfig::from_env().enabled)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Ok(false)
    }
}

// ============ 问题 Rank 分析 API ============

/// 手动触发问题 Rank 分析（实时采集堆栈 + 分析）
#[server(AnalyzeProblematicRanks)]
pub async fn analyze_problematic_ranks(
) -> Result<crate::rank_analysis_types::RankAnalysisResult, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::hang_detector::runner::run_rank_analysis_with_trigger;
        use crate::hang_detector::state::get_hang_state;
        use crate::rank_analyzer::{set_last_analysis, AnalysisTrigger, RankAnalysisConfig};
        use crate::hang_types::HangStatus;

        let state = get_hang_state();
        let snapshot = state
            .read()
            .map_err(|e| ServerFnError::new(format!("获取 HANG 状态失败: {}", e)))?
            .snapshot();

        match snapshot.status {
            HangStatus::Hang => {}
            HangStatus::Normal => {
                return Err(ServerFnError::new(
                    "当前未检测到 HANG，跳过问题 Rank 分析",
                ));
            }
            HangStatus::Disabled => {
                return Err(ServerFnError::new(
                    "HANG 检测未启用，无法进行问题 Rank 分析",
                ));
            }
        }

        let config = RankAnalysisConfig::from_env();
        let result = run_rank_analysis_with_trigger(&config, AnalysisTrigger::Manual)
            .await
            .map_err(|e| ServerFnError::new(format!("分析失败: {}", e)))?;

        set_last_analysis(result.clone());
        Ok(result)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::new("SSR feature required"))
    }
}

/// 获取最近一次问题 Rank 分析结果（缓存）
#[server(GetProblematicRanks)]
pub async fn get_problematic_ranks(
) -> Result<Option<crate::rank_analysis_types::RankAnalysisResult>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use crate::rank_analyzer::get_last_analysis;
        Ok(get_last_analysis())
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::new("SSR feature required"))
    }
}
