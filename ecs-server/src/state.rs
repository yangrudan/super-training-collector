//! ECS 服务器共享状态

use crate::storage::Storage;
use dashmap::DashMap;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── 任务平台配置 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct JobPlatformConfig {
    pub api_url: String,
    pub app_key: String,
    pub app_secret: String,
    pub user_id: String,
}

impl JobPlatformConfig {
    pub fn from_env() -> Self {
        Self {
            api_url: std::env::var("JOB_PLATFORM_API_URL").unwrap_or_default(),
            app_key: std::env::var("JOB_PLATFORM_APP_KEY").unwrap_or_default(),
            app_secret: std::env::var("JOB_PLATFORM_APP_SECRET").unwrap_or_default(),
            user_id: std::env::var("JOB_PLATFORM_USER_ID").unwrap_or_default(),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.api_url.is_empty() && !self.app_key.is_empty() && !self.app_secret.is_empty()
    }
}

/// 从训练平台 API 查询到的任务信息
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct JobInfo {
    pub creator: String,
    pub creator_name: String,
    pub name: String,
    pub gpu_type: String,
    pub gpu_num: u64,
    pub worker_num: u64,
}

// ─── 应用状态 ─────────────────────────────────────────────────────────────────

pub struct AppState {
    pub collectors: DashMap<String, CollectorEntry>,
    pub unnamed_collectors: DashMap<String, String>,
    pub unnamed_counter: AtomicU64,
    pub job_platform: JobPlatformConfig,
    pub storage: Storage,
    /// 超过 stale_secs 未推送 → 视为"陈旧"（前端打 stale 徽章，但保留数据）
    pub stale_secs: u64,
    /// 超过 offline_secs 未推送 → 视为"已离线/任务停止"（前端灰化卡片）
    pub offline_secs: u64,
}

pub type SharedState = Arc<AppState>;

pub fn new_state(storage: Storage) -> SharedState {
    let stale_secs = std::env::var("ECS_STALE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90u64);
    let offline_secs = std::env::var("ECS_OFFLINE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600u64);
    Arc::new(AppState {
        collectors: DashMap::new(),
        unnamed_collectors: DashMap::new(),
        unnamed_counter: AtomicU64::new(1),
        job_platform: JobPlatformConfig::from_env(),
        storage,
        stale_secs,
        offline_secs,
    })
}

// ─── Collector 数据 ───────────────────────────────────────────────────────────

/// 单个 Collector 的最新快照
#[derive(Clone, Debug, Serialize)]
pub struct CollectorEntry {
    pub id: String,
    pub source_ip: String,
    pub collector_addr: String,
    pub last_seen: u64,
    pub payload: serde_json::Value,
    pub job_id: String,
    pub job_info: Option<JobInfo>,
}

/// 使用 JOB_ID 作为 ECS 管理标识；未提供时按来源 IP 分配稳定的“未命名任务N”。
pub fn resolve_collector_identity(
    state: &SharedState,
    job_id: &str,
    connect_info: &SocketAddr,
) -> (String, String) {
    let source_ip = connect_info.ip().to_string();

    let (id, newly_allocated) = if job_id.trim().is_empty() {
        let mut newly = None;
        let entry = state
            .unnamed_collectors
            .entry(source_ip.clone())
            .or_insert_with(|| {
                let index = state.unnamed_counter.fetch_add(1, Ordering::SeqCst);
                let name = format!("未命名任务{}", index);
                newly = Some((name.clone(), index as i64));
                name
            });
        (entry.value().clone(), newly)
    } else {
        (job_id.trim().to_string(), None)
    };

    if let Some((name, counter)) = newly_allocated {
        let storage = state.storage.clone();
        let row = crate::storage::UnnamedAllocRow {
            source_ip: source_ip.clone(),
            name,
            counter,
        };
        tokio::task::spawn_blocking(move || {
            if let Err(e) = storage.upsert_unnamed(&row) {
                tracing::warn!("[storage] 持久化未命名任务计数失败: {}", e);
            }
        });
    }

    (id, format!("http://{}:3000", source_ip))
}

// ─── API 响应结构 ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CollectorSummary {
    pub id: String,
    pub source_ip: String,
    pub collector_addr: String,
    pub last_seen: u64,
    pub last_seen_ago_secs: i64,
    pub global: serde_json::Value,
    pub hang_status: String,
    pub node_count: u64,
    pub critical_count: u64,
    pub is_stale: bool,
    pub is_offline: bool,
    /// 用于前端展示的归一化状态：active / stale / offline
    pub liveness: String,
}

#[derive(Serialize)]
pub struct CollectorDetail {
    pub id: String,
    pub source_ip: String,
    pub collector_addr: String,
    pub last_seen: u64,
    pub last_seen_ago_secs: i64,
    pub payload: serde_json::Value,
    pub job_id: String,
    pub job_info: Option<JobInfo>,
    pub is_stale: bool,
    pub is_offline: bool,
    pub liveness: String,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl CollectorEntry {
    pub fn to_summary(&self, stale_secs: u64, offline_secs: u64) -> CollectorSummary {
        let now = now_secs();
        let ago = now as i64 - self.last_seen as i64;
        let is_offline = ago >= offline_secs as i64;
        let is_stale = !is_offline && ago >= stale_secs as i64;
        let liveness = if is_offline { "offline" } else if is_stale { "stale" } else { "active" };
        let global = self.payload.get("global").cloned().unwrap_or_default();
        let hang_status = self
            .payload
            .get("hang")
            .and_then(|h| h.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let node_count = global
            .get("total_nodes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let critical_count = global
            .get("critical_nodes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        CollectorSummary {
            id: self.id.clone(),
            source_ip: self.source_ip.clone(),
            collector_addr: self.collector_addr.clone(),
            last_seen: self.last_seen,
            last_seen_ago_secs: ago,
            global,
            hang_status,
            node_count,
            critical_count,
            is_stale,
            is_offline,
            liveness: liveness.to_string(),
        }
    }

    pub fn to_detail(&self, stale_secs: u64, offline_secs: u64) -> CollectorDetail {
        let now = now_secs();
        let ago = now as i64 - self.last_seen as i64;
        let is_offline = ago >= offline_secs as i64;
        let is_stale = !is_offline && ago >= stale_secs as i64;
        let liveness = if is_offline { "offline" } else if is_stale { "stale" } else { "active" };
        CollectorDetail {
            id: self.id.clone(),
            source_ip: self.source_ip.clone(),
            collector_addr: self.collector_addr.clone(),
            last_seen: self.last_seen,
            last_seen_ago_secs: ago,
            payload: self.payload.clone(),
            job_id: self.job_id.clone(),
            job_info: self.job_info.clone(),
            is_stale,
            is_offline,
            liveness: liveness.to_string(),
        }
    }
}
