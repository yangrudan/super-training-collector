//! 数据推送调度器
//!
//! 以配置的时间间隔（默认 30 秒）将全局指标、节点列表、HANG 状态
//! 主动 HTTP POST（JSON）到外部 ECS 接收端点。
//!
//! 配置优先级（由高到低）：
//!   1. 环境变量 `PUSH_TARGET_URL` / `PUSH_INTERVAL_SECS`
//!   2. `config/collector.json` 中的 `push_target_url` / `push_interval_secs`
//!
//! 当目标 URL 为空时，本调度器自动禁用，无任何副作用。

use crate::adapter::{generate_global_metrics_from_real_data, get_real_training_data};
use crate::flamegraph::{get_config_path, load_collector_config};
use crate::hang_detector::state::get_hang_state;
use crate::hang_types::HangStatusSnapshot;
use crate::models::{GlobalMetrics, NodeMetrics};
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 每次推送的 JSON Payload
#[derive(Serialize)]
pub struct PushPayload {
    /// Unix 时间戳（秒）
    pub timestamp: u64,
    /// 全局聚合指标
    pub global: GlobalMetrics,
    /// 节点列表
    pub nodes: Vec<NodeMetrics>,
    /// HANG 检测状态快照
    pub hang: HangStatusSnapshot,
    /// 训练任务 ID（来自环境变量 JOB_ID），供 ECS 查询任务信息
    #[serde(default)]
    pub job_id: String,
}

/// 解析推送配置：目标 URL 和推送间隔（秒）
fn resolve_push_config() -> (String, u64) {
    let config_path = get_config_path();
    let (url_from_file, interval_from_file, id_from_file, addr_from_file) =
        load_collector_config(&config_path)
            .map(|c| {
                (
                    c.push_target_url,
                    c.push_interval_secs,
                    c.push_collector_id,
                    c.push_collector_addr,
                )
            })
            .unwrap_or_default();

    let target_url = std::env::var("PUSH_TARGET_URL")
        .unwrap_or(url_from_file)
        .trim()
        .to_string();

    let interval = std::env::var("PUSH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(interval_from_file);

    // 推送间隔最小 10 秒，避免过于频繁
    (target_url, interval.max(10))
}

/// 解析 Collector 标识和地址（供 ECS 识别和反向代理使用）
fn resolve_collector_meta() -> (String, String) {
    let config_path = get_config_path();
    let (id_from_file, addr_from_file) = load_collector_config(&config_path)
        .map(|c| (c.push_collector_id, c.push_collector_addr))
        .unwrap_or_default();

    let collector_id = std::env::var("PUSH_COLLECTOR_ID")
        .unwrap_or(id_from_file)
        .trim()
        .to_string();

    let collector_addr = std::env::var("PUSH_COLLECTOR_ADDR")
        .unwrap_or(addr_from_file)
        .trim()
        .to_string();

    (collector_id, collector_addr)
}

/// 采集当前数据并构建推送 Payload
async fn build_payload() -> Option<PushPayload> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (ranks, nodes) = match get_real_training_data().await {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!("[push_scheduler] 获取训练数据失败，跳过本次推送: {}", e);
            return None;
        }
    };

    let global = generate_global_metrics_from_real_data(&nodes, &ranks);

    let hang = {
        let state_arc = get_hang_state();
        let guard = state_arc.read();
        match guard {
            Ok(s) => s.snapshot(),
            Err(e) => {
                tracing::warn!("[push_scheduler] 获取 HANG 状态失败: {}", e);
                HangStatusSnapshot::default()
            }
        }
    };

    Some(PushPayload {
        timestamp,
        global,
        nodes,
        hang,
        job_id: std::env::var("JOB_ID").unwrap_or_default(),
    })
}

/// 启动推送调度器（后台长运行任务）
///
/// 在 `server/src/main.rs` 中通过 `tokio::spawn` 调用：
/// ```ignore
/// tokio::spawn(async { app::push_scheduler::start_push_scheduler().await });
/// ```
pub async fn start_push_scheduler() {
    let (target_url, interval_secs) = resolve_push_config();

    if target_url.is_empty() {
        tracing::info!("[push_scheduler] PUSH_TARGET_URL 未配置，推送功能已禁用");
        return;
    }

    tracing::info!(
        "[push_scheduler] 推送功能已启用，目标: {}，间隔: {} 秒",
        target_url,
        interval_secs
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let (collector_id, collector_addr) = resolve_collector_meta();

    loop {
        if let Some(payload) = build_payload().await {
            let mut req = client.post(&target_url).json(&payload);
            if !collector_id.is_empty() {
                req = req.header("X-Collector-ID", &collector_id);
            }
            if !collector_addr.is_empty() {
                req = req.header("X-Collector-Addr", &collector_addr);
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        tracing::warn!("[push_scheduler] 推送返回非 2xx 状态: {}", status);
                    } else {
                        tracing::debug!("[push_scheduler] 推送成功，HTTP {}", status);
                    }
                }
                Err(e) => tracing::warn!("[push_scheduler] 推送失败: {}", e),
            }
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}
