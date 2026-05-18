//! HANG 检测状态管理模块
//!
//! 管理 HANG 检测的全局状态，包括历史堆栈数据和检测结果

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

// 重新导出共享类型供内部使用
pub use crate::hang_types::{HangDetails, HangStatus, HangStatusSnapshot};

/// 单个 rank 的堆栈历史
#[derive(Debug, Clone, Default)]
pub struct RankStackHistory {
    /// 历史堆栈集合（最近 N 次）
    pub history: VecDeque<HashSet<String>>,
    /// 连续高相似度计数
    pub high_similarity_count: u8,
    /// 连续采集失败/超时计数（曾经成功过的 rank 才累计）
    pub consecutive_failure_count: u8,
    /// 最近一次相似度（无法判断时为 None）
    pub last_similarity: Option<f64>,
}

impl RankStackHistory {
    /// 添加新的堆栈采样
    pub fn push(&mut self, stack_set: HashSet<String>, max_history: usize) {
        self.history.push_back(stack_set);
        while self.history.len() > max_history {
            self.history.pop_front();
        }
    }

    /// 获取最近一次的堆栈集合
    pub fn last(&self) -> Option<&HashSet<String>> {
        self.history.back()
    }

    /// 该 rank 是否有历史采样
    pub fn has_history(&self) -> bool {
        !self.history.is_empty()
    }
}

/// 节点堆栈历史记录（按 rank 拆分）
#[derive(Debug, Clone, Default)]
pub struct NodeStackHistory {
    /// 按 rank 索引保存的历史（节点内的 rank 顺序由采集端固定）
    pub ranks: Vec<RankStackHistory>,
    /// 最近一次本节点的代表性相似度（rank 中位数，仅用于 UI）
    pub last_similarity: f64,
}

impl NodeStackHistory {
    /// 确保 rank 数量足够
    pub fn ensure_rank_count(&mut self, n: usize) {
        if self.ranks.len() < n {
            self.ranks.resize(n, RankStackHistory::default());
        }
    }
}

/// 连续 Normal 多少轮才视为真正"恢复"
pub const RECOVERY_NORMAL_ROUNDS: u8 = 2;

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
    /// 当前 HANG 事件 ID（开始时间戳，None 表示当前不在 HANG 事件中）
    pub hang_event_id: Option<u64>,
    /// 当前 HANG 事件是否已记录日志（按事件去重）
    pub hang_logged: bool,
    /// 当前 HANG 事件是否已发送钉钉通知（按事件去重）
    pub hang_notified: bool,
    /// 连续被判定为 Normal 的轮次数（用于判断是否真正恢复）
    pub consecutive_normal_count: u8,
    /// 待发送的"HANG 解除"通知（仅当上一次 HANG 已经发过告警时才会被设置）
    ///
    /// Some((event_id, hang_duration_secs)) 表示从 HANG 转为 Normal 的瞬间需要发恢复通知。
    /// 发送完成后由 [`Self::mark_recovery_notified`] 清空。
    pub pending_recovery: Option<(u64, u64)>,
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
            hang_event_id: None,
            hang_logged: false,
            hang_notified: false,
            consecutive_normal_count: 0,
            pending_recovery: None,
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

    /// 检查是否需要记录日志（HANG 且未记录过）
    pub fn should_log(&self) -> bool {
        self.status == HangStatus::Hang && self.hang_event_id.is_some() && !self.hang_logged
    }

    /// 标记当前 HANG 已发送钉钉通知
    pub fn mark_notified(&mut self) {
        self.hang_notified = true;
    }

    /// 检查是否需要发送通知（HANG 且未通知过）
    pub fn should_notify(&self) -> bool {
        self.status == HangStatus::Hang && self.hang_event_id.is_some() && !self.hang_notified
    }

    /// 进入 / 维持 HANG 状态
    ///
    /// 若上一状态不是 HANG，则生成新的 event_id，并清零 notified/logged 标志。
    /// 返回是否是"新事件"（即本次刚刚从 Normal 转入 Hang）。
    pub fn enter_hang(&mut self) -> bool {
        self.enter_hang_with_backdate(0)
    }

    /// 进入 HANG 并将事件起始时间回溯 `backdate_secs` 秒
    ///
    /// 判定为 HANG 的瞬间已经是"连续 N 次相同堆栈"之后，真实卡住时间应当回溯
    /// 约 `sample_count × sample_interval` 秒，否则首次告警的"已持续"会贴近 0。
    pub fn enter_hang_with_backdate(&mut self, backdate_secs: u64) -> bool {
        self.consecutive_normal_count = 0;
        let was_new = self.hang_event_id.is_none();
        if was_new {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let event_id = now.saturating_sub(backdate_secs);
            self.hang_event_id = Some(event_id);
            self.hang_logged = false;
            self.hang_notified = false;
        }
        self.status = HangStatus::Hang;
        was_new
    }

    /// 进入 Normal 状态（仅在连续达到阈值后才真正清空事件）
    ///
    /// 返回是否本次刚刚"真正恢复"（即从 HANG 转为 Normal 的瞬间）。
    /// 若上一次 HANG 已发过钉钉通知，则同步设置 `pending_recovery`，由 runner 触发解除通知。
    pub fn observe_normal(&mut self, recovery_threshold: u8) -> bool {
        self.consecutive_normal_count = self.consecutive_normal_count.saturating_add(1);
        if self.consecutive_normal_count >= recovery_threshold {
            let was_in_hang = self.hang_event_id.is_some();
            if was_in_hang && self.hang_notified {
                // 上一次 HANG 已发过通知 → 需要发"告警解除"通知
                let event_id = self.hang_event_id.unwrap_or(0);
                let duration = self.hang_duration_secs().unwrap_or(0);
                self.pending_recovery = Some((event_id, duration));
            }
            self.status = HangStatus::Normal;
            self.hang_event_id = None;
            self.hang_logged = false;
            self.hang_notified = false;
            was_in_hang
        } else {
            false
        }
    }

    /// 取出待发送的"HANG 解除"通知任务（取出后不再返回）
    pub fn take_pending_recovery(&mut self) -> Option<(u64, u64)> {
        self.pending_recovery.take()
    }

    /// 本轮无法判断（采集失败 / 新选节点首次采样），不改变 status 和事件标志
    pub fn observe_no_signal(&mut self) {
        // 仍然更新时间戳，但不动 status / event_id / counters
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

    /// 当前 HANG 事件已持续多少秒
    pub fn hang_duration_secs(&self) -> Option<u64> {
        self.hang_event_id.map(|start| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(start);
            now.saturating_sub(start)
        })
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
    fn test_rank_stack_history_push_and_cap() {
        let mut history = RankStackHistory::default();

        for i in 0..5 {
            let set: HashSet<String> = [format!("item_{}", i)].into_iter().collect();
            history.push(set, 3);
        }

        assert_eq!(history.history.len(), 3);
    }

    #[test]
    fn test_node_stack_history_ensure_rank_count() {
        let mut node = NodeStackHistory::default();
        node.ensure_rank_count(4);
        assert_eq!(node.ranks.len(), 4);
        // 再次扩容不会缩小
        node.ensure_rank_count(2);
        assert_eq!(node.ranks.len(), 4);
        node.ensure_rank_count(6);
        assert_eq!(node.ranks.len(), 6);
    }

    #[test]
    fn test_default_state() {
        let state = HangDetectorState::new();

        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.sample_round, 0);
        assert_eq!(state.status, HangStatus::Disabled);
        assert!(state.hang_event_id.is_none());
        assert!(!state.hang_logged);
        assert!(!state.hang_notified);
        assert_eq!(state.consecutive_normal_count, 0);
    }

    #[test]
    fn test_enter_hang_generates_event_id_once() {
        let mut state = HangDetectorState::new();

        let is_new = state.enter_hang();
        assert!(is_new);
        assert_eq!(state.status, HangStatus::Hang);
        let first_event = state.hang_event_id;
        assert!(first_event.is_some());

        // 后续仍在 HANG 中，不应产生新 event_id
        let is_new2 = state.enter_hang();
        assert!(!is_new2);
        assert_eq!(state.hang_event_id, first_event);
    }

    #[test]
    fn test_should_notify_and_log_per_event() {
        let mut state = HangDetectorState::new();
        state.enter_hang();

        assert!(state.should_notify());
        assert!(state.should_log());

        state.mark_notified();
        state.mark_logged();

        assert!(!state.should_notify());
        assert!(!state.should_log());

        // 一次 Normal 还不足以恢复（默认阈值 2）
        let recovered = state.observe_normal(RECOVERY_NORMAL_ROUNDS);
        assert!(!recovered);
        assert_eq!(state.status, HangStatus::Hang);
        assert!(state.hang_event_id.is_some());
        // 标志仍然保留，避免在抖动中重复通知
        assert!(!state.should_notify());

        // 第二次 Normal -> 真正恢复
        let recovered = state.observe_normal(RECOVERY_NORMAL_ROUNDS);
        assert!(recovered);
        assert_eq!(state.status, HangStatus::Normal);
        assert!(state.hang_event_id.is_none());
        assert!(!state.hang_notified);
        assert!(!state.hang_logged);
        // 恢复瞬间应当排队一条"告警解除"通知
        let recovery = state.take_pending_recovery();
        assert!(recovery.is_some(), "已发过告警的事件恢复时应当排队解除通知");
        // 取出后应当被清空
        assert!(state.take_pending_recovery().is_none());

        // 再次进入 HANG -> 新事件 -> 重新可通知
        state.enter_hang();
        assert!(state.should_notify());
        assert!(state.should_log());
    }

    /// 未发过告警的 HANG 事件恢复时**不**应当排队恢复通知（避免误发）
    #[test]
    fn test_recovery_without_prior_notify_skips_alert() {
        let mut state = HangDetectorState::new();
        state.enter_hang();
        // 注意：不调用 mark_notified，模拟"该 HANG 短到没来得及发告警就恢复"
        for _ in 0..RECOVERY_NORMAL_ROUNDS {
            state.observe_normal(RECOVERY_NORMAL_ROUNDS);
        }
        assert_eq!(state.status, HangStatus::Normal);
        assert!(state.take_pending_recovery().is_none());
    }

    #[test]
    fn test_observe_no_signal_keeps_status() {
        let mut state = HangDetectorState::new();
        state.enter_hang();
        state.mark_notified();

        state.observe_no_signal();
        assert_eq!(state.status, HangStatus::Hang);
        assert!(state.hang_event_id.is_some());
        assert!(state.hang_notified);
        assert_eq!(state.consecutive_normal_count, 0);
    }

    #[test]
    fn test_reset_round_keeps_event_id() {
        let mut state = HangDetectorState::new();
        state.enter_hang();
        state.selected_nodes.push("node1".to_string());
        state.sample_round = 3;

        state.reset_round();

        // 重选节点不应清空事件
        assert!(state.hang_event_id.is_some());
        assert_eq!(state.status, HangStatus::Hang);
        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.sample_round, 0);
    }

    #[test]
    fn test_snapshot() {
        let mut state = HangDetectorState::new();
        state.status = HangStatus::Hang;
        state.selected_nodes = vec!["node1".to_string(), "node2".to_string()];
        state.sample_round = 2;
        state.touch();

        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, HangStatus::Hang);
        assert!(snapshot.timestamp > 0);
    }
}
