//! HTTP 请求处理器

use crate::job_info_client::fetch_job_info;
use crate::state::{CollectorEntry, SharedState, resolve_collector_identity};
use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
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

    // 检测是否 HANG（保留旧的 job_info，仅在 HANG 时触发新查询）
    let is_hanging = payload
        .get("hang")
        .and_then(|h| h.get("is_hanging"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let old_job_info = state
        .collectors
        .get(&id)
        .and_then(|e| e.job_info.clone());

    state.collectors.insert(
        id.clone(),
        CollectorEntry {
            id: id.clone(),
            source_ip: addr.ip().to_string(),
            collector_addr,
            last_seen: now_secs(),
            payload,
            job_id: job_id.clone(),
            job_info: old_job_info,
        },
    );

    // 如果正在 HANG 且有 job_id，异步查询任务信息
    if is_hanging && !job_id.is_empty() && state.job_platform.is_configured() {
        let state_clone = state.clone();
        let jp = state.job_platform.clone();
        tokio::spawn(async move {
            if let Some(info) = fetch_job_info(
                &jp.api_url,
                &jp.app_key,
                &jp.app_secret,
                &jp.user_id,
                &job_id,
            )
            .await
            {
                tracing::info!("[ecs/push] 获取到任务信息: job={}, creator={}", job_id, info.creator_name);
                if let Some(mut entry) = state_clone.collectors.get_mut(&id) {
                    entry.job_info = Some(info);
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
    let mut list: Vec<serde_json::Value> = state
        .collectors
        .iter()
        .map(|entry| serde_json::to_value(entry.to_summary()).unwrap_or_default())
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
    match state.collectors.get(&id) {
        Some(entry) => Json(serde_json::to_value(entry.to_detail()).unwrap_or_default())
            .into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "collector not found"}))).into_response(),
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
