//! HANG 检测状态管理模块
//!
//! 管理 HANG 检测的全局状态，包括历史堆栈数据和检测结果

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

// 重新导出共享类型供内部使用
pub use crate::hang_types::{HangDetails, HangStatus, HangStatusSnapshot};

/// 节点堆栈历史记录
#[derive(Debug, Clone)]
pub struct NodeStackHistory {
    /// 历史堆栈集合（最近 N 次）
    pub history: VecDeque<HashSet<String>>,
    /// 连续高相似度计数
    pub high_similarity_count: u8,
    /// 最后一次相似度
    pub last_similarity: f64,
}

impl Default for NodeStackHistory {
    fn default() -> Self {
        Self {
            history: VecDeque::with_capacity(4),
            high_similarity_count: 0,
            last_similarity: 0.0,
        }
    }
}

impl NodeStackHistory {
    /// 添加新的堆栈采样
    pub fn push(&mut self, stack_set: HashSet<String>, max_history: usize) {
        self.history.push_back(stack_set);
        // 保持历史记录在限制内
        while self.history.len() > max_history {
            self.history.pop_front();
        }
    }

    /// 获取最近一次的堆栈集合
    pub fn last(&self) -> Option<&HashSet<String>> {
        self.history.back()
    }

    /// 获取倒数第二次的堆栈集合（用于比较）
    pub fn previous(&self) -> Option<&HashSet<String>> {
        if self.history.len() >= 2 {
            self.history.get(self.history.len() - 2)
        } else {
            None
        }
    }
}

/// HANG 检测器全局状态
#[derive(Debug)]
pub struct HangDetectorState {
    /// 当前轮次选中的节点
    pub selected_nodes: Vec<String>,
    /// 当前轮次内的采样计数
    pub sample_round: u8,
    /// 各节点的堆栈历史
    pub node_history: HashMap<String, NodeStackHistory>,
    /// 当前状态
    pub status: HangStatus,
    /// 详细信息
    pub details: HangDetails,
    /// 最后更新时间
    pub last_update: u64,
    /// 当前 HANG 是否已记录日志（防止重复记录）
    pub hang_logged: bool,
    /// 当前 HANG 是否已发送钉钉通知（防止重复通知）
    pub hang_notified: bool,
}

impl Default for HangDetectorState {
    fn default() -> Self {
        Self {
            selected_nodes: Vec::new(),
            sample_round: 0,
            node_history: HashMap::new(),
            status: HangStatus::Disabled,
            details: HangDetails::default(),
            last_update: 0,
            hang_logged: false,
            hang_notified: false,
        }
    }
}

impl HangDetectorState {
    /// 创建新的状态
    pub fn new() -> Self {
        Self::default()
    }

    /// 重置状态（新一轮检测）
    pub fn reset_round(&mut self) {
        self.selected_nodes.clear();
        self.sample_round = 0;
        // 保留历史数据，只重置轮次计数
    }

    /// 标记当前 HANG 已记录日志
    pub fn mark_logged(&mut self) {
        self.hang_logged = true;
    }

    /// 重置日志标记（当状态从 HANG 变为非 HANG 时调用）
    pub fn reset_logged(&mut self) {
        self.hang_logged = false;
    }

    /// 检查是否需要记录日志（HANG 且未记录过）
    pub fn should_log(&self) -> bool {
        self.status == HangStatus::Hang && !self.hang_logged
    }

    /// 标记当前 HANG 已发送钉钉通知
    pub fn mark_notified(&mut self) {
        self.hang_notified = true;
    }

    /// 重置通知标记（当状态从 HANG 变为非 HANG 时调用）
    pub fn reset_notified(&mut self) {
        self.hang_notified = false;
    }

    /// 检查是否需要发送通知（HANG 且未通知过）
    pub fn should_notify(&self) -> bool {
        self.status == HangStatus::Hang && !self.hang_notified
    }

    /// 更新时间戳
    pub fn touch(&mut self) {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// 获取当前状态的快照（用于 API 响应）
    pub fn snapshot(&self) -> HangStatusSnapshot {
        HangStatusSnapshot {
            status: self.status.clone(),
            details: self.details.clone(),
            timestamp: self.last_update,
        }
    }
}

/// 全局状态单例
pub static HANG_STATE: Lazy<Arc<RwLock<HangDetectorState>>> =
    Lazy::new(|| Arc::new(RwLock::new(HangDetectorState::new())));

/// 获取全局状态的只读引用
pub fn get_hang_state() -> Arc<RwLock<HangDetectorState>> {
    HANG_STATE.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_stack_history() {
        let mut history = NodeStackHistory::default();

        let set1: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let set2: HashSet<String> = ["c", "d"].iter().map(|s| s.to_string()).collect();

        history.push(set1.clone(), 3);
        assert_eq!(history.history.len(), 1);
        assert!(history.last().is_some());
        assert!(history.previous().is_none());

        history.push(set2.clone(), 3);
        assert_eq!(history.history.len(), 2);
        assert!(history.previous().is_some());
        assert_eq!(history.previous().unwrap(), &set1);
    }

    #[test]
    fn test_history_max_size() {
        let mut history = NodeStackHistory::default();

        for i in 0..5 {
            let set: HashSet<String> = [format!("item_{}", i)].into_iter().collect();
            history.push(set, 3);
        }

        // 最多保留 3 个
        assert_eq!(history.history.len(), 3);
    }

    #[test]
    fn test_hang_detector_state() {
        let mut state = HangDetectorState::new();

        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.sample_round, 0);
        assert_eq!(state.status, HangStatus::Disabled);

        state.selected_nodes.push("node1".to_string());
        state.sample_round = 2;
        state.touch();

        assert!(state.last_update > 0);

        state.reset_round();
        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.sample_round, 0);
    }

    #[test]
    fn test_snapshot() {
        let mut state = HangDetectorState::new();
        state.status = HangStatus::Warning;
        state.selected_nodes = vec!["node1".to_string(), "node2".to_string()];
        state.sample_round = 2;
        state.touch();

        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, HangStatus::Warning);
        assert!(snapshot.timestamp > 0);
    }
}
