//! HANG 检测共享类型
//!
//! 这些类型需要在 SSR 和客户端都能使用

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HANG 检测状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HangStatus {
    /// 正在收集样本，数据不足
    Collecting,
    /// 正常运行
    Normal,
    /// 可能 HANG（1-2次高相似度）
    Warning,
    /// 确认 HANG（连续达到阈值）
    Hang,
    /// 检测过程中出错
    Error(String),
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
