//! HANG 检测运行时模块
//!
//! 提供异步运行时集成，用于服务端启动调度器

use super::config::HangConfig;
use super::detector::{HangDetector, NodeObservation};
use super::logger::HangLogger;
use super::notifier::send_hang_alert;
use super::state::HangStatus;
use crate::adapter::get_real_training_data;
use crate::flamegraph::{build_callstack_url, build_callstack_urls, load_collector_config};
use crate::rank_analyzer::{
    analyze_trie, set_last_analysis, AnalysisTrigger, RankAnalysisConfig, RankAnalysisResult,
};
use serde_json;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use tracing;

/// 启动 HANG 检测调度器
///
/// 这个函数应该在服务启动时被调用，使用 tokio::spawn 运行
pub async fn start_hang_detector_scheduler() {
    let config = HangConfig::from_env();

    if !config.enabled {
        tracing::info!("HANG detection is disabled");
        return;
    }

    tracing::info!(
        "Starting HANG detection scheduler with interval: {}s",
        config.sample_interval_secs
    );

    let detector = HangDetector::new(config.clone());
    let logger = HangLogger::new(config.clone());
    let mut tick = interval(Duration::from_secs(config.sample_interval_secs));
    // 长时间的 rank 分析可能让多个 tick 堆积，使用 Skip 行为避免短时间内连续触发
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // 存储本轮各节点的堆栈数据（用于日志记录）
    let mut round_stacks: HashMap<String, Vec<Vec<String>>> = HashMap::new();

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

                    let (observation, similarity) =
                        detector.process_node_stacks(&node_ip, stacks);
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
                if let Some(log_path) = logger.log_hang_event(round_stacks.clone()).await {
                    tracing::warn!("HANG detected! Log saved to: {}", log_path);
                }

                // 仅在尚未发送过本事件的通知时才执行 rank 分析 + 发钉钉
                let need_notify = {
                    use super::state::get_hang_state;
                    let state = get_hang_state();
                    let state = state.read().unwrap();
                    state.should_notify()
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

                    // 拿到事件元数据（event_id + 持续时长）
                    let (event_id, duration_secs) = {
                        use super::state::get_hang_state;
                        let state = get_hang_state();
                        let state = state.read().unwrap();
                        (state.hang_event_id, state.hang_duration_secs())
                    };

                    tracing::warn!(
                        "Sending DingTalk HANG alert (event_id={:?})",
                        event_id
                    );
                    send_hang_alert(Some(&analysis_summary), event_id, duration_secs).await;

                    use super::state::get_hang_state;
                    let state = get_hang_state();
                    let mut state = state.write().unwrap();
                    state.mark_notified();
                }
            }
            _ => {
                // 非 HANG：state 的 observe_normal 已经管理了事件清理，这里无需手动 reset
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
    use std::sync::{Arc, Mutex};

    let collector_config = load_collector_config(&get_config_path())
        .map_err(|e| format!("Failed to load collector config: {}", e))?;

    let (ranks, nodes) = get_real_training_data()
        .await
        .map_err(|e| format!("Failed to get training data: {}", e))?;

    if ranks.is_empty() || nodes.is_empty() {
        return Err("No nodes available".to_string());
    }

    // 按 rank_id 排序后逐个 rank 构建 URL，确保采集顺序与 adapter 中的 rank 顺序一致
    let mut sorted_ranks = ranks;
    sorted_ranks.sort_by_key(|rank| rank.rank_id);
    let mut all_urls = Vec::with_capacity(sorted_ranks.len());
    // rank_id → node_ip 映射
    let mut rank_to_node: HashMap<u32, String> = HashMap::new();
    for rank in sorted_ranks {
        rank_to_node.insert(rank.rank_id, rank.node_ip.clone());
        all_urls.push(build_callstack_url(
            &rank.node_ip,
            rank.local_rank,
            collector_config.callstack_base_port,
        ));
    }

    let total_ranks = all_urls.len() as u32;
    let trie = Arc::new(Mutex::new(StackTrie::with_total_ranks(total_ranks)));
    let trie_clone = trie.clone();
    let missing_ranks = Arc::new(Mutex::new(Vec::<u32>::new()));
    let missing_ranks_clone = missing_ranks.clone();

    fetch_urls_batched(all_urls, collector_config.batch_size, 4, move |batch| {
        let trie_inner = trie_clone.clone();
        let missing_ranks_inner = missing_ranks_clone.clone();
        async move {
            let batch_rank_ids: Vec<u32> = batch
                .iter()
                .map(|(rank_index, _)| *rank_index as u32)
                .collect();
            let processed = process_callstacks_batch(batch);
            let processed_rank_ids: std::collections::HashSet<u32> =
                processed.iter().map(|(rank, _)| *rank).collect();
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
            drop(trie_guard);

            let batch_missing: Vec<u32> = batch_rank_ids
                .into_iter()
                .filter(|rank_id| !processed_rank_ids.contains(rank_id))
                .collect();
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

    // 填充 node_ip 信息
    for rank in &mut result.problematic_ranks {
        rank.node_ip = rank_to_node.get(&rank.rank_id).cloned();
    }
    append_missing_ranks(&mut result, &missing_rank_ids, &rank_to_node);

    Ok(result)
}

fn append_missing_ranks(
    result: &mut RankAnalysisResult,
    missing_rank_ids: &[u32],
    rank_to_node: &HashMap<u32, String>,
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
            existing.issue_reason = Some("调用栈采集失败或返回空栈".to_string());
            continue;
        }

        result
            .problematic_ranks
            .push(crate::rank_analyzer::ProblematicRank {
                rank_id,
                node_ip: rank_to_node.get(&rank_id).cloned(),
                issue_reason: Some("调用栈采集失败或返回空栈".to_string()),
                anomaly_score: 0,
                divergence_points: Vec::new(),
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
    if result.problematic_ranks.is_empty() {
        return format!(
            "- 未发现明显异常 Rank（总 Rank: {}，耗时: {}ms，阈值: {:.0}%）",
            result.total_ranks,
            result.analysis_duration_ms,
            result.minority_threshold * 100.0
        );
    }

    let top_ranks: Vec<String> = result
        .problematic_ranks
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, rank)| {
            let node_ip = rank.node_ip.as_deref().unwrap_or("-");
            if let Some(reason) = &rank.issue_reason {
                format!(
                    "{}. Rank {}（节点: {}，异常: {}）",
                    index + 1,
                    rank.rank_id,
                    node_ip,
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
                    "{}. Rank {}（节点: {}，异常分数: {}，主要分叉: {}）",
                    index + 1,
                    rank.rank_id,
                    node_ip,
                    rank.anomaly_score,
                    major_point
                )
            }
        })
        .collect();

    format!(
        "- 检测到 {} 个问题 Rank（共 {} 个，耗时: {}ms，阈值: {:.0}%）\n{}",
        result.problematic_ranks.len(),
        result.total_ranks,
        result.analysis_duration_ms,
        result.minority_threshold * 100.0,
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
                },
                crate::rank_analyzer::ProblematicRank {
                    rank_id: 5,
                    node_ip: Some("10.0.0.2".to_string()),
                    issue_reason: None,
                    anomaly_score: 2,
                    divergence_points: vec![],
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

        append_missing_ranks(&mut result, &[1, 1], &rank_to_node);

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
}
