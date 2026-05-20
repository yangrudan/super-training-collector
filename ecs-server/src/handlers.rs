//! HTTP 请求处理器

use crate::job_info_client::fetch_job_info;
use crate::state::{CollectorEntry, FlamegraphState, SharedState, resolve_collector_identity};
use crate::storage::{SnapshotRow, extract_metrics, hang_status_of};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Push 接收 ───────────────────────────────────────────────────────────────

/// POST /push  — 接收 Collector 推送
pub async fn push_handler(
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: axum::body::Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[ecs/push] JSON 解析失败 from {}: {}", addr, e);
            return (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
        }
    };

    let job_id = payload
        .get("job_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let (id, collector_addr) = resolve_collector_identity(&state, &job_id, &addr);
    let payload_keys = payload.as_object().map(|m| m.len()).unwrap_or(0);

    tracing::debug!(
        "[ecs/push] 收到推送: source_ip={}, task_id={}, job_id={}, payload_keys={}",
        addr.ip(),
        id,
        if job_id.is_empty() { "<empty>" } else { &job_id },
        payload_keys,
    );

    // 检测当前 HANG 状态及与上一帧的差异
    let is_hanging = payload
        .get("hang")
        .and_then(|h| h.get("status"))
        .and_then(|v| v.as_str())
        .map(|s| s == "Hang")
        .unwrap_or(false);
    let hang_status_now = hang_status_of(&payload);

    let (old_job_info, prev_is_hanging, prev_hang_status, prev_critical) = match state
        .collectors
        .get(&id)
    {
        Some(entry) => {
            let prev_hanging = entry
                .payload
                .get("hang")
                .and_then(|h| h.get("status"))
                .and_then(|v| v.as_str())
                .map(|s| s == "Hang")
                .unwrap_or(false);
            let prev_status = hang_status_of(&entry.payload);
            let prev_critical = entry
                .payload
                .get("global")
                .and_then(|g| g.get("critical_nodes"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            (
                entry.job_info.clone(),
                prev_hanging,
                prev_status,
                prev_critical,
            )
        }
        None => (None, false, "Unknown".to_string(), 0),
    };

    let now_ts = now_secs();

    state.collectors.insert(
        id.clone(),
        CollectorEntry {
            id: id.clone(),
            source_ip: addr.ip().to_string(),
            collector_addr: collector_addr.clone(),
            last_seen: now_ts,
            payload: payload.clone(),
            job_id: job_id.clone(),
            job_info: old_job_info.clone(),
        },
    );

    // 持久化到 SQLite（spawn_blocking 避免阻塞 axum runtime）
    {
        let storage = state.storage.clone();
        let metrics = extract_metrics(now_ts as i64, &payload);
        let snap = SnapshotRow {
            collector_id: id.clone(),
            source_ip: addr.ip().to_string(),
            collector_addr: collector_addr.clone(),
            last_seen: now_ts as i64,
            job_id: job_id.clone(),
            job_info_json: old_job_info
                .as_ref()
                .and_then(|j| serde_json::to_string(j).ok()),
            payload_json: payload.to_string(),
        };
        let id_for_blocking = id.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = storage.insert_metrics(&id_for_blocking, &metrics) {
                tracing::warn!("[storage] 写入指标失败: id={}, err={}", id_for_blocking, e);
            }
            if let Err(e) = storage.upsert_snapshot(&snap) {
                tracing::warn!("[storage] 写入快照失败: id={}, err={}", id_for_blocking, e);
            }
        });
    }

    // 状态翻转事件：HANG / 非 HANG 翻转
    if prev_is_hanging != is_hanging {
        let kind = if is_hanging { "hang_start" } else { "hang_end" };
        let detail = json!({
            "prev_status": prev_hang_status,
            "status": hang_status_now,
            "hang": payload.get("hang").cloned().unwrap_or(json!(null)),
        });
        let storage = state.storage.clone();
        let id2 = id.clone();
        let kind2 = kind.to_string();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = storage.insert_event(&id2, now_ts as i64, &kind2, &detail) {
                tracing::warn!("[storage] 写入事件失败: id={}, kind={}, err={}", id2, kind2, e);
            }
        });
    }

    // critical_nodes 数量变化事件（仅当从 0 变非 0 或反之时记录，避免噪音）
    let critical_now = payload
        .get("global")
        .and_then(|g| g.get("critical_nodes"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if (prev_critical == 0) != (critical_now == 0) {
        let detail = json!({
            "prev_critical": prev_critical,
            "critical": critical_now,
        });
        let storage = state.storage.clone();
        let id2 = id.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) =
                storage.insert_event(&id2, now_ts as i64, "critical_change", &detail)
            {
                tracing::warn!("[storage] 写入事件失败: id={}, err={}", id2, e);
            }
        });
    }

    // 如果正在 HANG 且有 job_id，异步查询任务信息
    if is_hanging && !job_id.is_empty() && state.job_platform.is_configured() {
        let state_clone = state.clone();
        let jp = state.job_platform.clone();
        let id_for_async = id.clone();
        let job_id_for_async = job_id.clone();
        tokio::spawn(async move {
            if let Some(info) = fetch_job_info(
                &jp.api_url,
                &jp.app_key,
                &jp.app_secret,
                &jp.user_id,
                &job_id_for_async,
            )
            .await
            {
                tracing::info!(
                    "[ecs/push] 获取到任务信息: job={}, creator={}",
                    job_id_for_async,
                    info.creator_name
                );
                if let Some(mut entry) = state_clone.collectors.get_mut(&id_for_async) {
                    entry.job_info = Some(info.clone());
                }
                if let Ok(info_str) = serde_json::to_string(&info) {
                    let storage = state_clone.storage.clone();
                    let id_blocking = id_for_async.clone();
                    let detail = json!({"job_id": job_id_for_async, "creator": info.creator_name});
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) = storage.update_snapshot_job_info(&id_blocking, &info_str) {
                            tracing::warn!("[storage] 更新 job_info 失败: {}", e);
                        }
                        if let Err(e) = storage.insert_event(
                            &id_blocking,
                            now_secs() as i64,
                            "job_info_fetched",
                            &detail,
                        ) {
                            tracing::warn!("[storage] 写入事件失败: {}", e);
                        }
                    });
                }
            }
        });
    }

    // 检查是否有待处理的火焰图请求，若有则在响应体中通知 Collector
    let flamegraph_requested = state
        .flamegraph_channels
        .get(&id)
        .map(|tx| matches!(*tx.borrow(), FlamegraphState::Requested))
        .unwrap_or(false);

    if flamegraph_requested {
        tracing::debug!("[ecs/push] 通知 collector 生成火焰图: id={}", id);
        Json(json!({"flamegraph_requested": true})).into_response()
    } else {
        StatusCode::OK.into_response()
    }
}

/// GET /healthz  — 健康检查
pub async fn healthz() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "ecs-server"
    }))
}

// ─── JSON API ─────────────────────────────────────────────────────────────────

/// GET /api/collectors  — 所有 Collector 摘要列表
pub async fn api_collectors(State(state): State<SharedState>) -> impl IntoResponse {
    let stale = state.stale_secs;
    let offline = state.offline_secs;
    let mut list: Vec<serde_json::Value> = state
        .collectors
        .iter()
        .map(|entry| serde_json::to_value(entry.to_summary(stale, offline)).unwrap_or_default())
        .collect();
    list.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });
    Json(list)
}

/// GET /api/collector/{id}  — 单个 Collector 详情
pub async fn api_collector(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Response {
    let stale = state.stale_secs;
    let offline = state.offline_secs;
    match state.collectors.get(&id) {
        Some(entry) => Json(serde_json::to_value(entry.to_detail(stale, offline)).unwrap_or_default())
            .into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "collector not found"}))).into_response(),
    }
}

// ─── 历史 API ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// 起始时间戳（unix epoch 秒）。缺省 = now - window_secs
    pub since: Option<i64>,
    /// 结束时间戳（unix epoch 秒）。缺省 = now
    pub until: Option<i64>,
    /// 时间窗口（秒），与 since 互斥；当 since 不提供时使用，缺省 21600 (6h)
    pub window_secs: Option<i64>,
}

impl HistoryQuery {
    fn resolve(&self) -> (i64, i64) {
        let now = now_secs() as i64;
        let until = self.until.unwrap_or(now);
        let since = self.since.unwrap_or_else(|| {
            let window = self.window_secs.unwrap_or(21600).max(60);
            until - window
        });
        (since, until)
    }
}

/// GET /api/collector/{id}/history?since=&until=&window_secs=
pub async fn api_history(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    let (since, until) = q.resolve();
    let storage = state.storage.clone();
    let id_clone = id.clone();
    let res = tokio::task::spawn_blocking(move || storage.query_history(&id_clone, since, until))
        .await;
    match res {
        Ok(Ok(rows)) => Json(json!({
            "id": id,
            "since": since,
            "until": until,
            "points": rows,
        }))
        .into_response(),
        Ok(Err(e)) => {
            tracing::warn!("[api/history] 查询失败: id={}, err={}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
                .into_response()
        }
        Err(e) => {
            tracing::warn!("[api/history] join 失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "join error"})))
                .into_response()
        }
    }
}

/// GET /api/collector/{id}/events?since=&until=&window_secs=
pub async fn api_events(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    let (since, until) = q.resolve();
    let storage = state.storage.clone();
    let id_clone = id.clone();
    let res = tokio::task::spawn_blocking(move || storage.query_events(&id_clone, since, until))
        .await;
    match res {
        Ok(Ok(rows)) => Json(json!({
            "id": id,
            "since": since,
            "until": until,
            "events": rows,
        }))
        .into_response(),
        Ok(Err(e)) => {
            tracing::warn!("[api/events] 查询失败: id={}, err={}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
                .into_response()
        }
        Err(e) => {
            tracing::warn!("[api/events] join 失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "join error"})))
                .into_response()
        }
    }
}

// ─── 火焰图（Pull-via-Push）────────────────────────────────────────────────────

/// 生成错误提示 SVG，在代理/生成失败时作为占位符返回
fn error_svg(msg: &str) -> String {
    // 对消息做简单的 XML 转义，避免 SVG 解析失败
    let escaped = msg
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="100">
  <rect width="600" height="100" fill="#fff3cd" stroke="#ffc107" stroke-width="1"/>
  <text x="10" y="30" font-family="monospace" font-size="13" fill="#856404">火焰图获取失败</text>
  <text x="10" y="55" font-family="monospace" font-size="11" fill="#856404">{}</text>
  <text x="10" y="80" font-family="monospace" font-size="10" fill="#aaa">请稍候重试（Collector 正在生成中）</text>
</svg>"##,
        escaped
    )
}

/// GET /api/collector/{id}/flamegraph/all
///
/// 采用 Pull-via-Push 模式：
/// 1. 在 flamegraph_channels 中创建/复用 watch channel，状态置为 Requested
/// 2. 等待 Collector 通过 POST /flamegraph/push 上传 SVG（最长 60s）
/// 3. 超时或失败时返回错误 SVG
pub async fn api_flamegraph_all(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Response {
    api_flamegraph_for_target(&state, &id, "all").await
}

/// GET /api/collector/{id}/flamegraph/{node_ip}
pub async fn api_flamegraph_node(
    State(state): State<SharedState>,
    Path((id, node_ip)): Path<(String, String)>,
) -> Response {
    api_flamegraph_for_target(&state, &id, &node_ip).await
}

async fn api_flamegraph_for_target(state: &SharedState, id: &str, target: &str) -> Response {
    // 获取或创建该 collector 的 watch channel，并将状态设置为 Requested
    let mut rx: watch::Receiver<FlamegraphState> = {
        let entry = state
            .flamegraph_channels
            .entry(id.to_string())
            .or_insert_with(|| {
                let (tx, _) = watch::channel(FlamegraphState::Idle);
                Arc::new(tx)
            });
        let tx = entry.value().clone();
        // 只有 all 才有通知，node 级别暂时也走同一通道（target 信息写入 Error 消息中由 collector 区分）
        if !matches!(*tx.borrow(), FlamegraphState::Ready(_)) {
            tx.send_replace(FlamegraphState::Requested);
        }
        tx.subscribe()
    };

    tracing::debug!(
        "[ecs/flamegraph] 等待 collector 生成火焰图: id={}, target={}",
        id,
        target
    );

    // 如果已经有缓存的 Ready 状态，直接返回（避免重复等待）
    {
        let current = rx.borrow().clone();
        if let FlamegraphState::Ready(svg) = current {
            return (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
                svg,
            )
                .into_response();
        }
    }

    // 等待状态变更（最长 60 秒）
    let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            if rx.changed().await.is_err() {
                return Err("channel closed".to_string());
            }
            match rx.borrow().clone() {
                FlamegraphState::Ready(svg) => return Ok(svg),
                FlamegraphState::Error(e) => return Err(e),
                _ => {} // Idle / Requested → 继续等待
            }
        }
    })
    .await;

    match result {
        Ok(Ok(svg)) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
            svg,
        )
            .into_response(),
        Ok(Err(e)) => {
            tracing::warn!("[ecs/flamegraph] collector 报告生成失败: id={}, err={}", id, e);
            (
                StatusCode::BAD_GATEWAY,
                [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
                error_svg(&e),
            )
                .into_response()
        }
        Err(_) => {
            tracing::warn!("[ecs/flamegraph] 等待超时 60s: id={}", id);
            // 超时后重置为 Idle，避免下次直接命中 Requested 但 Collector 已不再处理
            if let Some(tx) = state.flamegraph_channels.get(id) {
                tx.send_replace(FlamegraphState::Idle);
            }
            (
                StatusCode::GATEWAY_TIMEOUT,
                [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
                error_svg("等待超时（60s），Collector 可能无法访问 callstack 端口"),
            )
                .into_response()
        }
    }
}

/// POST /api/collector/{id}/flamegraph/push
///
/// Collector 生成完火焰图后，将 SVG 内容 POST 到此端点。
/// ECS 通过 watch channel 唤醒所有等待该 collector 火焰图的客户端。
pub async fn api_flamegraph_push(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let svg = match std::str::from_utf8(&body) {
        Ok(s) if !s.is_empty() => s.to_string(),
        _ => {
            return (StatusCode::BAD_REQUEST, "empty or invalid UTF-8 SVG body").into_response();
        }
    };

    tracing::info!(
        "[ecs/flamegraph] 收到 collector 上传的火焰图: id={}, bytes={}",
        id,
        svg.len()
    );

    // 获取或创建 channel，发送 Ready 状态
    let tx = state
        .flamegraph_channels
        .entry(id.clone())
        .or_insert_with(|| {
            let (tx, _) = watch::channel(FlamegraphState::Idle);
            Arc::new(tx)
        })
        .clone();

    tx.send_replace(FlamegraphState::Ready(svg));

    // 延迟重置为 Idle（30 秒后），让短时间内的重复请求能直接命中缓存
    let state_clone = state.clone();
    let id_clone = id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        if let Some(tx) = state_clone.flamegraph_channels.get(&id_clone) {
            // 仅在仍是 Ready 状态时才重置（避免覆盖新的 Requested）
            if matches!(*tx.borrow(), FlamegraphState::Ready(_)) {
                tx.send_replace(FlamegraphState::Idle);
            }
        }
    });

    StatusCode::OK.into_response()
}

// ─── HTML 页面 ────────────────────────────────────────────────────────────────

const DASHBOARD_HTML: &str = include_str!("../templates/dashboard.html");
const COLLECTOR_HTML: &str = include_str!("../templates/collector.html");

/// GET /  — 主仪表板
pub async fn page_dashboard() -> impl IntoResponse {
    axum::response::Html(DASHBOARD_HTML)
}

/// GET /collector/{id}  — 单个 Collector 详情页
pub async fn page_collector(Path(_id): Path<String>) -> impl IntoResponse {
    axum::response::Html(COLLECTOR_HTML)
}
