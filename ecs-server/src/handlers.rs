//! HTTP 请求处理器

use crate::job_info_client::fetch_job_info;
use crate::state::{CollectorEntry, SharedState, resolve_collector_identity};
use crate::storage::{SnapshotRow, extract_metrics, hang_status_of};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

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
        .and_then(|h| h.get("is_hanging"))
        .and_then(|v| v.as_bool())
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
                .and_then(|h| h.get("is_hanging"))
                .and_then(|v| v.as_bool())
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

    StatusCode::OK.into_response()
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

// ─── 火焰图反向代理 ────────────────────────────────────────────────────────────

/// GET /api/collector/{id}/flamegraph/all
pub async fn api_flamegraph_all(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Response {
    proxy_flamegraph(&state, &id, "all").await
}

/// GET /api/collector/{id}/flamegraph/{node_ip}
pub async fn api_flamegraph_node(
    State(state): State<SharedState>,
    Path((id, node_ip)): Path<(String, String)>,
) -> Response {
    proxy_flamegraph(&state, &id, &node_ip).await
}

async fn proxy_flamegraph(state: &SharedState, id: &str, target: &str) -> Response {
    let collector_addr = match state.collectors.get(id) {
        Some(e) => e.collector_addr.clone(),
        None => return (StatusCode::NOT_FOUND, "Collector not found").into_response(),
    };

    let url = format!("{}/rest/flamegraph/{}", collector_addr.trim_end_matches('/'), target);
    tracing::debug!("[ecs/flamegraph] 代理请求: {}", url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default();

    match client.get(&url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            (
                StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
                body,
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("[ecs/flamegraph] 代理失败 ({}): {}", url, e);
            (StatusCode::BAD_GATEWAY, format!("Failed to proxy flamegraph: {}", e)).into_response()
        }
    }
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
