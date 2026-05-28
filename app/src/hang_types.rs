//! HANG 检测共享类型
//!
//! 这些类型需要在 SSR 和客户端都能使用

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HANG 检测状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HangStatus {
    /// 正常运行
    Normal,
    /// 确认 HANG（连续达到阈值）
    Hang,
    /// 检测未启用
    #[default]
    Disabled,
}

/// HANG 检测详细信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HangDetails {
    /// 被判定为 HANG 的节点列表
    pub hang_nodes: Vec<String>,
    /// 各节点的 Jaccard 相似度
    pub node_similarities: HashMap<String, f64>,
    /// 连续高相似度次数
    pub consecutive_high_similarity: u8,
    /// 最后一次检测时间
    pub last_check_time: u64,
    /// 本次 HANG 已持续秒数（进入 HANG 后才有值）
    pub hang_duration_secs: Option<u64>,
    /// STC 从启动到现在的守护时长（秒）
    pub stc_uptime_secs: u64,
    /// 本轮选中的节点数
    pub selected_node_count: usize,
    /// 本轮有效参与判定的节点数（排除 NoSignal）
    pub valid_node_count: usize,
    /// 本轮被判定为 HANG 的节点数
    pub hang_node_count: usize,
    /// 本轮有 HANG 证据的 rank 数
    pub hang_rank_count: usize,
    /// 本轮有效节点的 rank 总数
    pub total_rank_count: usize,
    /// 本轮有效节点的平均相似度
    pub avg_similarity: Option<f64>,
    /// 本轮有效节点的最高相似度
    pub max_similarity: Option<f64>,
}

/// HANG 状态快照（用于 API 响应）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HangStatusSnapshot {
    /// 当前状态
    pub status: HangStatus,
    /// 详细信息
    pub details: HangDetails,
    /// 快照时间
    pub timestamp: u64,
}

impl Default for HangStatusSnapshot {
    fn default() -> Self {
        Self {
            status: HangStatus::Disabled,
            details: HangDetails::default(),
            timestamp: 0,
        }
    }
}
