//! HANG 检测运行时模块
//!
//! 提供异步运行时集成，用于服务端启动调度器

use super::config::HangConfig;
use super::detector::{HangDetector, NodeObservation};
use super::logger::HangLogger;
use super::notifier::{send_hang_alert, send_hang_recovery_alert, HangAlertStats};
use super::state::{init_stc_start_time, HangStatus};
use crate::adapter::{get_real_training_data, get_real_training_data_with_world_size};
use crate::flamegraph::{build_callstack_url, build_callstack_urls, load_collector_config};
use crate::rank_analyzer::{
    analyze_trie, set_last_analysis, AnalysisTrigger, RankAnalysisConfig, RankAnalysisResult,
};
use serde_json;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use tracing;

/// 启动 HANG 检测调度器
///
/// 这个函数应该在服务启动时被调用，使用 tokio::spawn 运行
pub async fn start_hang_detector_scheduler() {
    init_stc_start_time();

    let config = HangConfig::from_env();

    if !config.enabled {
        tracing::info!("HANG detection is disabled");
        return;
    }

    tracing::info!(
        "Starting HANG detection scheduler with interval: {}~{}s (random per tick), \
         node_rank_quorum={}, global_min_hang_nodes={}, global_min_hang_ranks={}",
        config.sample_interval_min_secs,
        config.sample_interval_max_secs,
        config.node_rank_quorum,
        config.global_min_hang_nodes,
        config.global_min_hang_ranks
    );

    let detector = HangDetector::new(config.clone());
    let logger = HangLogger::new(config.clone());

    // 存储本轮各节点的堆栈数据（用于日志记录）
    let mut round_stacks: HashMap<String, Vec<Vec<String>>> = HashMap::new();

    loop {
        // 每 tick 在 [min, max] 内随机睡眠，避免与训练 step 周期同频共振
        let next_secs = config.random_sample_interval_secs();
        sleep(Duration::from_secs(next_secs)).await;
        tracing::debug!("HANG detection tick (slept {}s)", next_secs);

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
            // 重置轮次计数，开始新一轮检测
            detector.reset_round();
            round_stacks.clear(); // 清空上一轮的堆栈数据
            let selected = detector.select_nodes(&all_nodes);
            detector.set_selected_nodes(selected.clone());
            tracing::debug!("Selected nodes for sampling: {:?}", selected);
        }

        let selected_nodes = detector.get_selected_nodes();
        detector.increment_sample_round();

        // 为每个选中的节点采集堆栈
        let mut results: Vec<(String, NodeObservation, f64)> = Vec::new();
        for node_ip in selected_nodes {
            match fetch_stacks(&node_ip).await {
                Ok(stacks) => {
                    // 保存堆栈数据用于日志记录
                    round_stacks.insert(node_ip.clone(), stacks.clone());

                    // 将本轮采样到的堆栈以 TRACE 级别输出，便于在需要时排查
                    // 通过 `RUST_LOG=app::hang_detector::runner=trace` 打开
                    if tracing::enabled!(tracing::Level::TRACE) {
                        for (rank_idx, stack) in stacks.iter().enumerate() {
                            tracing::trace!(
                                node = %node_ip,
                                rank = rank_idx,
                                depth = stack.len(),
                                "sampled stack: {}",
                                stack.join(" | ")
                            );
                        }
                    }

                    let (observation, similarity) = detector.process_node_stacks(&node_ip, stacks);
                    results.push((node_ip.clone(), observation, similarity));
                    tracing::debug!(
                        "Node {}: observation={:?}, similarity={:.3}",
                        node_ip,
                        observation,
                        similarity
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch stacks for node {}: {}", node_ip, e);
                    results.push((node_ip.clone(), NodeObservation::NoSignal, 0.0));
                }
            }
        }

        // 更新全局状态（事件 ID / 恢复阈值由 state 内部统一管理）
        let status = detector.update_global_status(&results);
        tracing::info!("HANG detection round completed, status: {:?}", status);

        // 根据状态处理日志
        match &status {
            HangStatus::Hang => {
                // 检测到 HANG，尝试记录日志并采集全局火焰图（事件期内只记一次）
                if let Some(log_path) = logger.log_hang_event(round_stacks.clone(), &results).await
                {
                    tracing::warn!("HANG detected! Log saved to: {}", log_path);
                }

                // 仅在尚未发送过本事件的通知时才执行 rank 分析 + 发钉钉
                let need_notify = {
                    use super::state::get_hang_state;
                    let state = get_hang_state();
                    let state = state.read().unwrap();
                    state.should_notify_with_intranet_delay(config.intranet_alert_delay_secs)
                };

                if need_notify {
                    // 自动触发问题 Rank 分析
                    let analysis_config = RankAnalysisConfig::from_env();
                    let analysis_summary = if analysis_config.enabled {
                        match run_rank_analysis(&analysis_config).await {
                            Ok(result) => {
                                let count = result.problematic_ranks.len();
                                tracing::info!(
                                    "Rank analysis completed: {} problematic ranks found in {}ms",
                                    count,
                                    result.analysis_duration_ms
                                );
                                let summary = format_rank_analysis_summary(&result);
                                set_last_analysis(result);
                                summary
                            }
                            Err(e) => {
                                tracing::warn!("Failed to run rank analysis: {}", e);
                                format!("问题 Rank 分析执行失败：{}", e)
                            }
                        }
                    } else {
                        "问题 Rank 分析未启用".to_string()
                    };

                    // 拿到事件元数据与展示统计
                    let (event_id, stats) = {
                        use super::state::get_hang_state;
                        let state = get_hang_state();
                        let state = state.read().unwrap();
                        (
                            state.hang_event_id,
                            HangAlertStats {
                                hang_duration_secs: state.hang_duration_secs(),
                                selected_node_count: state.details.selected_node_count,
                                valid_node_count: state.details.valid_node_count,
                                hang_node_count: state.details.hang_node_count,
                                hang_rank_count: state.details.hang_rank_count,
                                total_rank_count: state.details.total_rank_count,
                                avg_similarity: state.details.avg_similarity,
                                max_similarity: state.details.max_similarity,
                            },
                        )
                    };

                    tracing::warn!("Sending HANG alert (event_id={:?})", event_id);
                    if let Some(event_id) = event_id {
                        let intranet_delay_secs = config.intranet_alert_delay_secs;
                        // 进入写锁：再次确认状态，并锁定本轮 skip 标志
                        //
                        // `attempted_intranet` 表示"本轮是否会真正尝试发送内网告警"。
                        // 若内网延迟尚未到达（或本事件内网已发过），则 attempted_intranet=false，
                        // skip_intranet=true，本轮跳过内网，等待下一轮再次检测 HANG 状态后重试。
                        let (
                            should_spawn,
                            skip_dingtalk,
                            skip_intranet,
                            skip_intranet_action,
                            attempted_intranet,
                            attempted_action,
                            intranet_success_time,
                        ) = {
                            use super::state::get_hang_state;
                            let state = get_hang_state();
                            let mut state = state.write().unwrap();
                            if state.should_notify_with_intranet_delay(intranet_delay_secs)
                                && state.hang_event_id == Some(event_id)
                            {
                                let intranet_ready =
                                    state.intranet_alert_ready(intranet_delay_secs);
                                let action_ready = state.intranet_action_ready(intranet_delay_secs);
                                state.mark_notify_in_flight();
                                (
                                    true,
                                    state.hang_notified,
                                    !intranet_ready,
                                    !action_ready,
                                    intranet_ready,
                                    action_ready,
                                    state.hang_intranet_success_time.clone(),
                                )
                            } else {
                                (false, false, false, false, false, false, None)
                            }
                        };

                        if should_spawn {
                            tokio::spawn(async move {
                                let outcome = send_hang_alert(
                                    Some(&analysis_summary),
                                    Some(event_id),
                                    stats,
                                    skip_dingtalk,
                                    skip_intranet,
                                    skip_intranet_action,
                                    intranet_success_time,
                                )
                                .await;
                                use super::state::get_hang_state;
                                let state = get_hang_state();
                                let mut state = state.write().unwrap();
                                // 钉钉：skip 仅在已成功后才会为 true，因此 outcome.dingtalk_done
                                // 直接反映本轮真实结果。
                                if !skip_dingtalk && outcome.dingtalk_done {
                                    state.mark_dingtalk_notified_for(event_id);
                                }
                                // 内网：仅当本轮"真正尝试"且 notifier 返回成功时才记账。
                                // 当 attempted_intranet=false（延迟未到）时 outcome.intranet_done
                                // 在 notifier 中被设为 skip→true，但语义是"本轮跳过"，
                                // **不能**视为发送成功，否则会永远不再尝试。
                                if attempted_intranet && outcome.intranet_done {
                                    state.mark_intranet_notified_for(
                                        event_id,
                                        outcome.intranet_success_time.clone(),
                                    );
                                }
                                // "内网后台告警动作"：同样只有"本轮真正尝试"才记账。
                                // 注意 notifier 内部要求 intranet 本体已成功才会真正发送动作通知，
                                // 因此当本轮内网失败时 outcome.intranet_action_done=false，
                                // 下一轮 intranet_action_ready 仍为 true，跟随 intranet 重试。
                                if attempted_action && outcome.intranet_action_done {
                                    state.mark_intranet_action_notified_for(event_id);
                                }
                                if !skip_dingtalk && !outcome.dingtalk_done {
                                    tracing::warn!(
                                        "DingTalk HANG alert failed, will retry on next eligible round (event_id={})",
                                        event_id
                                    );
                                }
                                if attempted_intranet && !outcome.intranet_done {
                                    tracing::warn!(
                                        "Intranet HANG alert failed, will retry on next eligible round (event_id={})",
                                        event_id
                                    );
                                }
                                if attempted_action && !outcome.intranet_action_done {
                                    tracing::warn!(
                                        "Intranet-alert-action DingTalk notify failed, will retry on next eligible round (event_id={})",
                                        event_id
                                    );
                                }
                                state.finish_notify_attempt_for(event_id);
                            });
                        }
                    }
                }
            }
            _ => {
                // 非 HANG：state 的 observe_normal 已经管理了事件清理。
                // 若刚刚从 HANG 转为 Normal 且之前已发过告警，发送保守提示通知。
                let pending = {
                    use super::state::get_hang_state;
                    let state = get_hang_state();
                    let state = state.read().unwrap();
                    state.pending_recovery()
                };
                if let Some((event_id, hang_duration_secs)) = pending {
                    tracing::warn!(
                        "Sending DingTalk HANG recovery alert (event_id={}, hang_duration={}s)",
                        event_id,
                        hang_duration_secs
                    );
                    let should_spawn = {
                        use super::state::get_hang_state;
                        let state = get_hang_state();
                        let mut state = state.write().unwrap();
                        if state.pending_recovery() == Some((event_id, hang_duration_secs)) {
                            state.mark_recovery_in_flight(event_id);
                            true
                        } else {
                            false
                        }
                    };

                    if should_spawn {
                        tokio::spawn(async move {
                            let ok =
                                send_hang_recovery_alert(Some(event_id), Some(hang_duration_secs))
                                    .await;
                            use super::state::get_hang_state;
                            let state = get_hang_state();
                            let mut state = state.write().unwrap();
                            if ok {
                                state.mark_recovery_notified(event_id);
                            } else {
                                tracing::warn!(
                                    "DingTalk HANG recovery alert failed, will retry on next eligible round (event_id={})",
                                    event_id
                                );
                                state.mark_recovery_failed(event_id);
                            }
                        });
                    }
                }
            }
        }
    }
}

/// 获取所有节点 IP 列表
async fn fetch_all_nodes() -> Result<Vec<String>, String> {
    match get_real_training_data().await {
        Ok((_ranks, nodes)) => {
            let node_ips: Vec<String> = nodes.iter().map(|n| n.node_ip.clone()).collect();
            Ok(node_ips)
        }
        Err(e) => Err(format!("Failed to get real training data: {}", e)),
    }
}

/// 从节点获取堆栈数据
///
/// 返回堆栈列表，每个堆栈对应一个 rank
async fn fetch_stacks(node_ip: &str) -> Result<Vec<Vec<String>>, String> {
    use crate::flamegraph::get_config_path;
    let config = load_collector_config(&get_config_path())
        .map_err(|e| format!("Failed to load config: {}", e))?;

    // 获取该节点的 rank_count
    let rank_count = {
        match get_real_training_data().await {
            Ok((_ranks, nodes)) => nodes
                .iter()
                .find(|n| n.node_ip == node_ip)
                .map(|n| n.rank_count)
                .unwrap_or(4),
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

    // 并发地拉取所有 rank，避免单个 rank（如被 STOP 的进程导致 py-spy attach 卡住）阻塞整节点
    let fetches = urls.into_iter().map(|url| {
        let client = client.clone();
        async move {
            let result = fetch_callstack(&client, &url).await;
            (url, result)
        }
    });
    let results = futures::future::join_all(fetches).await;

    let mut stacks = Vec::with_capacity(results.len());
    for (url, result) in results {
        match result {
            Ok(stack) => stacks.push(stack),
            Err(e) => {
                tracing::warn!("Failed to fetch from {}: {}", url, e);
                stacks.push(Vec::new());
            }
        }
    }

    Ok(stacks)
}

/// 从单个 URL 获取调用栈
async fn fetch_callstack(client: &reqwest::Client, url: &str) -> Result<Vec<String>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    // 解析堆栈帧列表
    let mut frames = Vec::new();

    if let serde_json::Value::Array(frame_array) = json {
        for frame in frame_array {
            // 直接序列化整个原始帧对象，避免格式化导致的信息损失
            let frame_str = frame.to_string();
            if !frame_str.is_empty() && frame_str != "null" {
                frames.push(frame_str);
            }
        }
    }

    Ok(frames)
}

/// 采集全局堆栈并运行问题 Rank 分析
pub async fn run_rank_analysis(
    config: &RankAnalysisConfig,
) -> Result<crate::rank_analyzer::RankAnalysisResult, String> {
    run_rank_analysis_with_trigger(config, AnalysisTrigger::HangDetected).await
}

/// 采集全局堆栈并运行问题 Rank 分析（指定触发来源）
pub async fn run_rank_analysis_with_trigger(
    config: &RankAnalysisConfig,
    trigger: AnalysisTrigger,
) -> Result<crate::rank_analyzer::RankAnalysisResult, String> {
    use crate::flamegraph::{
        get_config_path, process_callstacks_batch, stack_collector::fetch_urls_batched,
        stack_merger::StackTrie,
    };
    use crate::rank_analyzer::{correlate_result, ParallelTopology};
    use std::sync::{Arc, Mutex};

    let collector_config = load_collector_config(&get_config_path())
        .map_err(|e| format!("Failed to load collector config: {}", e))?;

    let (ranks, nodes, reported_world_size) = get_real_training_data_with_world_size()
        .await
        .map_err(|e| format!("Failed to get training data: {}", e))?;

    if ranks.is_empty() || nodes.is_empty() {
        return Err("No nodes available".to_string());
    }

    let mut sorted_ranks = ranks;
    sorted_ranks.sort_by_key(|rank| rank.rank_id);
    let rank_ids = sorted_ranks
        .iter()
        .map(|rank| rank.rank_id)
        .collect::<Vec<_>>();
    let observed_world_size = rank_ids
        .iter()
        .max()
        .map(|rank| rank.saturating_add(1))
        .unwrap_or(0);
    let process_env = std::env::vars().collect::<HashMap<_, _>>();
    let mut topology_error = None;
    let configured_world_size = match ParallelTopology::configured_world_size(&process_env) {
        Ok(world_size) => world_size,
        Err(error) => {
            topology_error = Some(error);
            None
        }
    };
    let mut total_ranks = reported_world_size
        .or(configured_world_size)
        .unwrap_or(observed_world_size);
    if observed_world_size > total_ranks {
        topology_error = Some(format!(
            "观测到 Rank {}，超出配置 world_size {}",
            observed_world_size - 1,
            total_ranks
        ));
        total_ranks = observed_world_size;
    }
    let missing_process_ranks = find_missing_process_ranks(total_ranks, &rank_ids);

    let mut all_urls = Vec::with_capacity(sorted_ranks.len());
    let mut rank_to_node: HashMap<u32, String> = HashMap::new();
    for rank in &sorted_ranks {
        rank_to_node.insert(rank.rank_id, rank.node_ip.clone());
        all_urls.push(build_callstack_url(
            &rank.node_ip,
            rank.local_rank,
            collector_config.callstack_base_port,
        ));
    }

    let parallel_topology = if topology_error.is_none() {
        match ParallelTopology::from_env_map(&process_env, total_ranks) {
            Ok(topology) => Some(topology),
            Err(error) => {
                topology_error = Some(error);
                None
            }
        }
    } else {
        None
    };

    let trie = Arc::new(Mutex::new(StackTrie::with_total_ranks(total_ranks)));
    let trie_clone = trie.clone();
    let missing_ranks = Arc::new(Mutex::new(Vec::<u32>::new()));
    let missing_ranks_clone = missing_ranks.clone();
    let index_to_rank = Arc::new(rank_ids);
    let index_to_rank_clone = index_to_rank.clone();

    fetch_urls_batched(all_urls, collector_config.batch_size, 4, move |batch| {
        let trie_inner = trie_clone.clone();
        let missing_ranks_inner = missing_ranks_clone.clone();
        let index_to_rank = index_to_rank_clone.clone();
        async move {
            let batch_indices = batch
                .iter()
                .map(|(rank_index, _)| *rank_index as u32)
                .collect::<Vec<_>>();
            let processed = process_callstacks_batch(batch);
            let processed_indices = processed
                .iter()
                .map(|(rank_index, _)| *rank_index)
                .collect::<std::collections::HashSet<_>>();
            let translated = processed
                .into_iter()
                .filter_map(|(rank_index, stack)| {
                    index_to_rank
                        .get(rank_index as usize)
                        .copied()
                        .map(|global_rank| (global_rank, stack))
                })
                .collect::<Vec<_>>();
            let stacks_refs = translated
                .iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect::<Vec<_>>();

            let mut trie_guard = trie_inner.lock().map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to acquire trie lock: {}", e),
                )) as Box<dyn std::error::Error + Send + Sync>
            })?;
            trie_guard.insert_batch(stacks_refs);
            drop(trie_guard);

            let batch_missing = batch_indices
                .into_iter()
                .filter(|rank_index| !processed_indices.contains(rank_index))
                .filter_map(|rank_index| index_to_rank.get(rank_index as usize).copied())
                .collect::<Vec<_>>();
            if !batch_missing.is_empty() {
                let mut missing_guard = missing_ranks_inner.lock().map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to acquire missing rank lock: {}", e),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?;
                missing_guard.extend(batch_missing);
            }
            Ok(())
        }
    })
    .await
    .map_err(|e| format!("Failed to collect stacks: {}", e))?;

    let trie_guard = trie.lock().map_err(|e| format!("Lock error: {}", e))?;
    let mut result = analyze_trie(&trie_guard, config, trigger);
    drop(trie_guard);

    let missing_rank_ids = missing_ranks
        .lock()
        .map_err(|e| format!("Missing rank lock error: {}", e))?
        .clone();

    for rank in &mut result.problematic_ranks {
        rank.node_ip = rank_to_node.get(&rank.rank_id).cloned();
    }
    append_missing_ranks(
        &mut result,
        &missing_process_ranks,
        &rank_to_node,
        "训练进程缺失（未出现在节点注册信息中）",
    );
    append_missing_ranks(
        &mut result,
        &missing_rank_ids,
        &rank_to_node,
        "调用栈采集失败或返回空栈",
    );
    correlate_result(&mut result, parallel_topology.as_ref(), topology_error);

    Ok(result)
}

fn find_missing_process_ranks(world_size: u32, observed_rank_ids: &[u32]) -> Vec<u32> {
    let observed = observed_rank_ids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    (0..world_size)
        .filter(|rank_id| !observed.contains(rank_id))
        .collect()
}

fn append_missing_ranks(
    result: &mut RankAnalysisResult,
    missing_rank_ids: &[u32],
    rank_to_node: &HashMap<u32, String>,
    reason: &str,
) {
    let mut seen_missing = std::collections::HashSet::new();

    for rank_id in missing_rank_ids
        .iter()
        .copied()
        .filter(|rank_id| seen_missing.insert(*rank_id))
    {
        if let Some(existing) = result
            .problematic_ranks
            .iter_mut()
            .find(|rank| rank.rank_id == rank_id)
        {
            existing.issue_reason = Some(reason.to_string());
            continue;
        }

        result
            .problematic_ranks
            .push(crate::rank_analyzer::ProblematicRank {
                rank_id,
                node_ip: rank_to_node.get(&rank_id).cloned(),
                issue_reason: Some(reason.to_string()),
                anomaly_score: 0,
                divergence_points: Vec::new(),
                parallel_context: None,
                suspected_dimensions: Vec::new(),
                root_cause_confidence: crate::rank_analysis_types::RootCauseConfidence::High,
                parallel_evidence: Vec::new(),
            });
    }

    result.problematic_ranks.sort_by(|a, b| {
        b.issue_reason
            .is_some()
            .cmp(&a.issue_reason.is_some())
            .then_with(|| b.anomaly_score.cmp(&a.anomaly_score))
            .then_with(|| a.rank_id.cmp(&b.rank_id))
    });
}

fn format_rank_analysis_summary(result: &RankAnalysisResult) -> String {
    let topology_line = if result.parallel_topology.available {
        format!(
            "Megatron 拓扑 TP/PP/DP/EP/CP={}/{}/{}/{}/{}，order={}",
            result.parallel_topology.tp_size,
            result.parallel_topology.pp_size,
            result.parallel_topology.dp_size,
            result.parallel_topology.ep_size,
            result.parallel_topology.cp_size,
            result.parallel_topology.rank_order,
        )
    } else {
        format!(
            "并行拓扑未参与：{}",
            result
                .parallel_topology
                .degraded_reason
                .as_deref()
                .unwrap_or("未知原因")
        )
    };

    if result.problematic_ranks.is_empty() {
        return format!(
            "- 未发现明显异常 Rank（总 Rank: {}，耗时: {}ms，阈值: {:.0}%）\n- {}",
            result.total_ranks,
            result.analysis_duration_ms,
            result.minority_threshold * 100.0,
            topology_line,
        );
    }

    let top_ranks = result
        .problematic_ranks
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, rank)| {
            let node_ip = rank.node_ip.as_deref().unwrap_or("-");
            let dimensions = if rank.suspected_dimensions.is_empty() {
                "无拓扑支持".to_string()
            } else {
                rank.suspected_dimensions
                    .iter()
                    .map(|dimension| dimension.label())
                    .collect::<Vec<_>>()
                    .join("/")
            };
            if let Some(reason) = &rank.issue_reason {
                format!(
                    "{}. Rank {}（节点: {}，置信度: {}，异常: {}）",
                    index + 1,
                    rank.rank_id,
                    node_ip,
                    rank.root_cause_confidence.label(),
                    reason
                )
            } else {
                let major_point = rank
                    .divergence_points
                    .iter()
                    .min_by(|a, b| {
                        a.minority_coverage
                            .partial_cmp(&b.minority_coverage)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|point| {
                        format!(
                            "{} ({:.0}%)",
                            shorten_frame_name(&point.frame_name),
                            point.minority_coverage * 100.0
                        )
                    })
                    .unwrap_or_else(|| "无明显分叉".to_string());
                format!(
                    "{}. Rank {}（节点: {}，置信度: {}，疑似维度: {}，关联证据: {} 条，主要分叉: {}）",
                    index + 1,
                    rank.rank_id,
                    node_ip,
                    rank.root_cause_confidence.label(),
                    dimensions,
                    rank.parallel_evidence.len(),
                    major_point
                )
            }
        })
        .collect::<Vec<_>>();

    format!(
        "- 检测到 {} 个问题 Rank（共 {} 个，耗时: {}ms，阈值: {:.0}%）\n- {}\n{}",
        result.problematic_ranks.len(),
        result.total_ranks,
        result.analysis_duration_ms,
        result.minority_threshold * 100.0,
        topology_line,
        top_ranks.join("\n")
    )
}

fn shorten_frame_name(name: &str) -> String {
    let short = name.rsplit("::").next().unwrap_or(name);
    if short.len() > 40 {
        format!("{}...", &short[..37])
    } else {
        short.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_rank_analysis_summary_contains_top_ranks() {
        let result = RankAnalysisResult {
            total_ranks: 8,
            analysis_duration_ms: 123,
            minority_threshold: 0.3,
            problematic_ranks: vec![
                crate::rank_analyzer::ProblematicRank {
                    rank_id: 3,
                    node_ip: Some("10.0.0.1".to_string()),
                    issue_reason: None,
                    anomaly_score: 4,
                    divergence_points: vec![crate::rank_analyzer::DivergencePoint {
                        frame_name: "foo::bar::kernel_main".to_string(),
                        depth: 1,
                        majority_count: 7,
                        minority_count: 1,
                        minority_coverage: 0.125,
                    }],
                    parallel_context: None,
                    suspected_dimensions: vec![],
                    root_cause_confidence: crate::rank_analysis_types::RootCauseConfidence::Low,
                    parallel_evidence: vec![],
                },
                crate::rank_analyzer::ProblematicRank {
                    rank_id: 5,
                    node_ip: Some("10.0.0.2".to_string()),
                    issue_reason: None,
                    anomaly_score: 2,
                    divergence_points: vec![],
                    parallel_context: None,
                    suspected_dimensions: vec![],
                    root_cause_confidence: crate::rank_analysis_types::RootCauseConfidence::Low,
                    parallel_evidence: vec![],
                },
            ],
            ..Default::default()
        };

        let summary = format_rank_analysis_summary(&result);

        assert!(summary.contains("检测到 2 个问题 Rank"));
        assert!(summary.contains("1. Rank 3"));
        assert!(summary.contains("kernel_main"));
        assert!(summary.contains("2. Rank 5"));
    }

    #[test]
    fn append_missing_ranks_marks_collection_failures() {
        let mut result = RankAnalysisResult {
            total_ranks: 4,
            problematic_ranks: vec![],
            ..Default::default()
        };
        let rank_to_node = HashMap::from([(1u32, "10.0.0.9".to_string())]);

        append_missing_ranks(
            &mut result,
            &[1, 1],
            &rank_to_node,
            "调用栈采集失败或返回空栈",
        );

        assert_eq!(result.problematic_ranks.len(), 1);
        assert_eq!(result.problematic_ranks[0].rank_id, 1);
        assert_eq!(
            result.problematic_ranks[0].issue_reason.as_deref(),
            Some("调用栈采集失败或返回空栈")
        );
        assert_eq!(
            result.problematic_ranks[0].node_ip.as_deref(),
            Some("10.0.0.9")
        );
    }

    #[test]
    fn finds_missing_process_in_non_contiguous_world() {
        let observed = (0..30).filter(|rank| *rank != 28).collect::<Vec<_>>();
        assert_eq!(find_missing_process_ranks(30, &observed), vec![28]);
    }

    #[test]
    fn missing_registered_process_is_high_confidence_root_cause() {
        let mut result = RankAnalysisResult {
            total_ranks: 30,
            ..Default::default()
        };
        append_missing_ranks(
            &mut result,
            &[28],
            &HashMap::new(),
            "训练进程缺失（未出现在节点注册信息中）",
        );
        assert_eq!(result.total_ranks, 30);
        assert_eq!(result.problematic_ranks[0].rank_id, 28);
        assert_eq!(
            result.problematic_ranks[0].root_cause_confidence,
            crate::rank_analysis_types::RootCauseConfidence::High
        );
        assert_eq!(
            result.problematic_ranks[0].issue_reason.as_deref(),
            Some("训练进程缺失（未出现在节点注册信息中）")
        );
    }
}
