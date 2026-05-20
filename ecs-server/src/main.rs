mod handlers;
mod job_info_client;
mod state;
mod storage;

use axum::{
    Router,
    routing::{get, post},
};
use state::{CollectorEntry, new_state};
use std::sync::atomic::Ordering;
use storage::Storage;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let db_path = std::env::var("ECS_DB_PATH").unwrap_or_else(|_| "./ecs_history.db".to_string());
    let storage = match Storage::open(&db_path) {
        Ok(s) => {
            tracing::info!("打开历史数据库: path={}", s.path().display());
            s
        }
        Err(e) => {
            tracing::error!("打开 SQLite 历史库失败 (path={}): {}", db_path, e);
            return;
        }
    };

    let shared = new_state(storage.clone());

    // 启动恢复：从 latest_snapshot 重建 DashMap，从 unnamed_alloc 重建未命名映射
    restore_from_disk(&shared);

    // 后台清理任务：每 10 分钟清理 24 小时前的指标与事件
    {
        let storage_bg = storage.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                tick.tick().await;
                let storage_inner = storage_bg.clone();
                let res = tokio::task::spawn_blocking(move || {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    storage_inner.cleanup_older_than(now)
                })
                .await;
                match res {
                    Ok(Ok((m, e))) if m + e > 0 => {
                        tracing::info!("[storage/cleanup] 清理过期数据: metrics={}, events={}", m, e);
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => tracing::warn!("[storage/cleanup] 清理失败: {}", e),
                    Err(e) => tracing::warn!("[storage/cleanup] 任务 join 失败: {}", e),
                }
            }
        });
    }

    // 后台 liveness 巡检：检测 active → stale → offline 翻转，写事件
    {
        let shared_bg = shared.clone();
        tokio::spawn(async move {
            // 每个 collector 上次观测到的 liveness（"active"/"stale"/"offline"）
            let mut last_state: std::collections::HashMap<String, String> = Default::default();
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tick.tick().await;
                let stale = shared_bg.stale_secs;
                let offline = shared_bg.offline_secs;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let mut transitions: Vec<(String, String, String, i64)> = Vec::new();
                for entry in shared_bg.collectors.iter() {
                    let ago = now - entry.last_seen as i64;
                    let liveness = if ago >= offline as i64 {
                        "offline"
                    } else if ago >= stale as i64 {
                        "stale"
                    } else {
                        "active"
                    };
                    let id = entry.id.clone();
                    let prev = last_state.get(&id).cloned().unwrap_or_else(|| "active".into());
                    if prev != liveness {
                        transitions.push((id.clone(), prev, liveness.to_string(), ago));
                        last_state.insert(id, liveness.to_string());
                    }
                }
                for (id, prev, curr, ago) in transitions {
                    // 仅记录恶化方向（active→stale/offline 与 stale→offline）以及恢复（→active）
                    let kind = match curr.as_str() {
                        "offline" => Some("collector_offline"),
                        "stale" if prev == "active" => Some("collector_stale"),
                        "active" if prev != "active" => Some("collector_recovered"),
                        _ => None,
                    };
                    let Some(kind) = kind else { continue };
                    tracing::info!(
                        "[liveness] collector={} {} -> {} (last_seen_ago={}s)",
                        id, prev, curr, ago
                    );
                    let storage_inner = shared_bg.storage.clone();
                    let id_clone = id.clone();
                    let detail = serde_json::json!({
                        "prev": prev, "current": curr, "last_seen_ago_secs": ago,
                    });
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) = storage_inner.insert_event(&id_clone, now, kind, &detail) {
                            tracing::warn!("[storage] 写入 liveness 事件失败: {}", e);
                        }
                    });
                }
            }
        });
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Push 接收端点
        .route("/push", post(handlers::push_handler))
        // 健康检查
        .route("/healthz", get(handlers::healthz))
        // JSON API
        .route("/api/collectors", get(handlers::api_collectors))
        .route("/api/collector/{id}", get(handlers::api_collector))
        .route(
            "/api/collector/{id}/history",
            get(handlers::api_history),
        )
        .route(
            "/api/collector/{id}/events",
            get(handlers::api_events),
        )
        .route(
            "/api/collector/{id}/flamegraph/all",
            get(handlers::api_flamegraph_all),
        )
        .route(
            "/api/collector/{id}/flamegraph/{node_ip}",
            get(handlers::api_flamegraph_node),
        )
        .route(
            "/api/collector/{id}/flamegraph/push",
            post(handlers::api_flamegraph_push),
        )
        // HTML 页面
        .route("/", get(handlers::page_dashboard))
        .route("/collector/{id}", get(handlers::page_collector))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(shared);

    let addr = std::env::var("ECS_ADDR").unwrap_or_else(|_| "0.0.0.0:4000".to_string());
    tracing::info!("准备启动 ECS 服务器: bind_addr={}", addr);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!("ECS 服务器绑定失败: bind_addr={}, error={}", addr, err);
            return;
        }
    };

    match listener.local_addr() {
        Ok(local_addr) => tracing::info!("ECS 服务器启动: http://{}", local_addr),
        Err(err) => tracing::warn!("ECS 服务器已启动，但无法读取本地监听地址: {}", err),
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap_or_else(|err| tracing::error!("ECS 服务器运行失败: {}", err));
}

fn restore_from_disk(shared: &state::SharedState) {
    match shared.storage.load_all_snapshots() {
        Ok(rows) => {
            let count = rows.len();
            for snap in rows {
                let payload: serde_json::Value =
                    serde_json::from_str(&snap.payload_json).unwrap_or(serde_json::Value::Null);
                let job_info = snap
                    .job_info_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                shared.collectors.insert(
                    snap.collector_id.clone(),
                    CollectorEntry {
                        id: snap.collector_id,
                        source_ip: snap.source_ip,
                        collector_addr: snap.collector_addr,
                        last_seen: snap.last_seen as u64,
                        payload,
                        job_id: snap.job_id,
                        job_info,
                    },
                );
            }
            if count > 0 {
                tracing::info!("从 SQLite 恢复 {} 个 collector 快照", count);
            }
        }
        Err(e) => tracing::warn!("[storage] 恢复 latest_snapshot 失败: {}", e),
    }

    match shared.storage.load_all_unnamed() {
        Ok(rows) => {
            for row in &rows {
                shared
                    .unnamed_collectors
                    .insert(row.source_ip.clone(), row.name.clone());
            }
            if !rows.is_empty() {
                tracing::info!("从 SQLite 恢复 {} 个未命名任务映射", rows.len());
            }
        }
        Err(e) => tracing::warn!("[storage] 恢复 unnamed_alloc 失败: {}", e),
    }

    match shared.storage.max_unnamed_counter() {
        Ok(max) if max > 0 => {
            shared
                .unnamed_counter
                .store((max as u64) + 1, Ordering::SeqCst);
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("[storage] 读取 unnamed 计数失败: {}", e),
    }
}
