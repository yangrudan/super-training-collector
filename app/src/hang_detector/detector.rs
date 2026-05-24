//! HANG 检测核心逻辑模块
//!
//! 实现节点选择、per-rank Jaccard 相似度判定和 HANG 状态更新

use rand::seq::SliceRandom;
use rand::thread_rng;

use super::config::HangConfig;
use super::jaccard::{jaccard_similarity, stack_to_set_with_options};
use super::state::{get_hang_state, HangStatus, NodeStackHistory, RankStackHistory};

/// 单节点本轮检测结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeObservation {
    /// 无有效信号（采集失败 / 新选节点首次采样）
    NoSignal,
    /// 本节点被判定为 HANG
    Hang,
    /// 命中恢复判定黑名单，不应累计恢复轮次
    RecoveryBlocked,
    /// 本节点正常
    Normal,
}

/// HANG 检测器
pub struct HangDetector {
    config: HangConfig,
}

impl HangDetector {
    /// 创建新的检测器
    pub fn new(config: HangConfig) -> Self {
        Self { config }
    }

    /// 从环境变量创建检测器
    pub fn from_env() -> Self {
        Self::new(HangConfig::from_env())
    }

    /// 检查是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// 随机选择 N 个节点进行采样
    pub fn select_nodes(&self, all_nodes: &[String]) -> Vec<String> {
        if all_nodes.is_empty() {
            return Vec::new();
        }

        let n = self.config.node_count.min(all_nodes.len());

        if n >= all_nodes.len() {
            return all_nodes.to_vec();
        }

        let mut rng = thread_rng();
        let mut nodes = all_nodes.to_vec();
        nodes.shuffle(&mut rng);
        nodes.truncate(n);
        nodes
    }

    /// 处理单个节点的堆栈采集
    ///
    /// 返回该节点的观测结果以及（用于 UI 展示的）代表性相似度。
    /// 当返回 `NoSignal` 时，调用方不应将其计入全局投票。
    pub fn process_node_stacks(
        &self,
        node_ip: &str,
        stacks: Vec<Vec<String>>,
    ) -> (NodeObservation, f64) {
        // 节点首轮：所有 rank 全空且无历史 → 真无信号
        if stacks.iter().all(|s| s.is_empty()) {
            let state = get_hang_state();
            let state = state.read().unwrap();
            let any_history = state
                .node_history
                .get(node_ip)
                .map(|h| h.ranks.iter().any(|r| r.has_history()))
                .unwrap_or(false);
            drop(state);
            if !any_history {
                return (NodeObservation::NoSignal, 0.0);
            }
        }

        // 白名单检查：任一 rank 命中已知长阻塞则放行
        let is_known_blocking = stacks
            .iter()
            .any(|stack| !stack.is_empty() && self.config.is_known_blocking(stack));
        let is_recovery_blocking = stacks
            .iter()
            .any(|stack| !stack.is_empty() && self.config.is_recovery_blocking(stack));

        let state = get_hang_state();
        let mut state = state.write().unwrap();

        let history = state
            .node_history
            .entry(node_ip.to_string())
            .or_insert_with(NodeStackHistory::default);
        history.ensure_rank_count(stacks.len());

        let mut similarities: Vec<f64> = Vec::with_capacity(stacks.len());
        let mut had_history_count = 0usize;
        let mut high_sim_rank_count = 0usize;
        let threshold = self.config.sample_count as u8;

        for (i, stack) in stacks.iter().enumerate() {
            let rank_history: &mut RankStackHistory = &mut history.ranks[i];

            // 采集失败/空：只对"曾经成功采集过"的 rank 累计失败计数
            if stack.is_empty() {
                if rank_history.has_history() {
                    rank_history.consecutive_failure_count =
                        rank_history.consecutive_failure_count.saturating_add(1);
                    rank_history.high_similarity_count = 0;
                    rank_history.last_similarity = None;
                    had_history_count += 1;
                    if rank_history.consecutive_failure_count >= threshold {
                        high_sim_rank_count += 1;
                    }
                }
                continue;
            }

            // 成功采集：清零失败计数
            rank_history.consecutive_failure_count = 0;

            let current_set =
                stack_to_set_with_options(stack, self.config.keep_line_numbers);
            if current_set.is_empty() {
                continue;
            }

            let similarity = rank_history.last().map(|last| {
                had_history_count += 1;
                jaccard_similarity(last, &current_set)
            });

            rank_history.push(current_set, self.config.sample_count + 1);

            if let Some(sim) = similarity {
                similarities.push(sim);
                rank_history.last_similarity = Some(sim);
                if sim >= self.config.jaccard_threshold {
                    rank_history.high_similarity_count =
                        rank_history.high_similarity_count.saturating_add(1);
                } else {
                    rank_history.high_similarity_count = 0;
                }
            } else {
                // 这是该 rank 的第一次采样，无法比较
                rank_history.last_similarity = None;
            }

            if rank_history.high_similarity_count >= threshold {
                high_sim_rank_count += 1;
            }
        }

        // 计算节点级代表性相似度（用于 UI / 日志）
        let representative = if similarities.is_empty() {
            0.0
        } else {
            similarities.iter().copied().sum::<f64>() / similarities.len() as f64
        };
        history.last_similarity = representative;

        // 若本轮没有任何 rank 可比（既无相似度可算、也无失败可计） → 无信号
        if had_history_count == 0 {
            return (NodeObservation::NoSignal, representative);
        }

        // 白名单优先：节点不判 HANG
        if is_known_blocking {
            return (NodeObservation::Normal, representative);
        }

        // 节点级 quorum：至少多少比例的 rank 表现出 hang 证据（高相似 或 持续采集失败）
        let quorum_threshold =
            ((stacks.len().max(1)) as f64 * self.config.node_rank_quorum).ceil() as usize;
        let is_hang = high_sim_rank_count >= quorum_threshold.max(1);

        let observation = if is_hang {
            NodeObservation::Hang
        } else if is_recovery_blocking {
            NodeObservation::RecoveryBlocked
        } else {
            NodeObservation::Normal
        };
        (observation, representative)
    }

    /// 根据各节点的检测结果更新全局状态
    ///
    /// - 若所有节点都是 `NoSignal`：保持当前 status（视为本轮无效）；
    /// - 否则按 ≥ 50% Hang 节点投票判定；
    /// - HANG 事件 ID 与"连续 Normal 才恢复"的去抖动逻辑统一在 state 内。
    pub fn update_global_status(
        &self,
        node_results: &[(String, NodeObservation, f64)],
    ) -> HangStatus {
        let state = get_hang_state();
        let mut state = state.write().unwrap();

        // 更新可用的相似度详情（即便本轮整体无信号，也尽量展示采集到的部分）
        state.details.node_similarities = node_results
            .iter()
            .map(|(ip, _, sim)| (ip.clone(), *sim))
            .collect();
        state.details.hang_nodes = node_results
            .iter()
            .filter(|(_, obs, _)| *obs == NodeObservation::Hang)
            .map(|(ip, _, _)| ip.clone())
            .collect();

        let valid: Vec<&NodeObservation> = node_results
            .iter()
            .map(|(_, obs, _)| obs)
            .filter(|obs| **obs != NodeObservation::NoSignal)
            .collect();

        if valid.is_empty() {
            // 全无信号，保持原状态（避免节点重选导致的伪 Normal 抖动）
            state.observe_no_signal();
            state.touch();
            return state.status.clone();
        }

        let hang_count = valid
            .iter()
            .filter(|obs| ***obs == NodeObservation::Hang)
            .count();
        let total_count = valid.len();

        // 全局判定：必须同时满足
        //   (1) hang 节点占比 >= 50%（多数票）
        //   (2) hang 节点绝对数 >= global_min_hang_nodes（避免小集群单点孤鸣）
        // 当集群只有 1 个节点时，使 effective_min = min(配置, total_count)，
        // 否则单节点集群将永远无法触发 HANG。
        let effective_min = self.config.global_min_hang_nodes.min(total_count.max(1));
        let majority_ok = hang_count * 2 >= total_count;
        let absolute_ok = hang_count >= effective_min;

        if majority_ok && absolute_ok {
            state.details.consecutive_high_similarity = self.config.sample_count as u8;
            // 回溯：判 HANG 那一刻起堆栈已经"卡了 sample_count 个采样窗口"
            // 间隔随机化时取区间均值作为代表，保持与告警里"已持续"贴近真实值
            let backdate = (self.config.sample_count as u64)
                .saturating_mul(self.config.sample_interval_secs());
            state.enter_hang_with_backdate(backdate);
        } else if state.status == HangStatus::Hang
            && (hang_count > 0
                || valid
                    .iter()
                    .any(|obs| **obs == NodeObservation::RecoveryBlocked))
        {
            // 当系统处于 HANG 状态但检测到部分节点仍满足 HANG 条件，
            // 或命中恢复黑名单时，
            // 不应该直接触发恢复。只有当所有有效节点都normal时才开始计数恢复。
            // 这防止了节点采样不稳定或网络波动导致的误判恢复。
            state.reset_normal_counter();
        } else {
            state.observe_normal(self.config.recovery_normal_rounds);
        }

        state.touch();
        state.status.clone()
    }

    /// 增加采样轮次计数
    pub fn increment_sample_round(&self) {
        let state = get_hang_state();
        let mut state = state.write().unwrap();
        state.sample_round += 1;
    }

    /// 重置当前轮次（节点失败或新一轮开始时调用）
    pub fn reset_round(&self) {
        let state = get_hang_state();
        let mut state = state.write().unwrap();
        state.reset_round();
    }

    /// 设置选中的节点
    pub fn set_selected_nodes(&self, nodes: Vec<String>) {
        let state = get_hang_state();
        let mut state = state.write().unwrap();
        state.selected_nodes = nodes;
    }

    /// 获取当前选中的节点
    pub fn get_selected_nodes(&self) -> Vec<String> {
        let state = get_hang_state();
        let state = state.read().unwrap();
        state.selected_nodes.clone()
    }

    /// 获取当前采样轮次
    pub fn get_sample_round(&self) -> u8 {
        let state = get_hang_state();
        let state = state.read().unwrap();
        state.sample_round
    }

    /// 是否需要选择新的节点（新轮次开始）
    pub fn needs_new_nodes(&self) -> bool {
        let state = get_hang_state();
        let state = state.read().unwrap();
        state.selected_nodes.is_empty() || state.sample_round > self.config.sample_count as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hang_detector::state::HANG_STATE;
    use std::sync::Mutex;

    /// 涉及全局 HANG_STATE 的测试必须串行执行
    static GLOBAL_STATE_LOCK: Mutex<()> = Mutex::new(());

    fn test_config() -> HangConfig {
        HangConfig {
            enabled: true,
            sample_interval_min_secs: 30,
            sample_interval_max_secs: 30,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: vec!["checkpoint".to_string()],
            log_enabled: true,
            log_dir: "hang_logs".to_string(),
            node_rank_quorum: 0.5,
            keep_line_numbers: true,
            recovery_normal_rounds: 2,
            global_min_hang_nodes: 1,
            intranet_alert_delay_secs: 20 * 60,
            recovery_blocking_patterns: vec![
                "Py_FinalizeEx".to_string(),
                "~ProcessGroupMCCL".to_string(),
            ],
        }
    }

    fn reset_state() {
        let state = HANG_STATE.clone();
        let mut s = state.write().unwrap();
        *s = super::super::state::HangDetectorState::new();
    }

    #[test]
    fn test_select_nodes_all() {
        let detector = HangDetector::new(test_config());
        let nodes = vec!["n1".to_string(), "n2".to_string()];

        let selected = detector.select_nodes(&nodes);

        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_select_nodes_random() {
        let detector = HangDetector::new(test_config());
        let nodes: Vec<String> = (0..10).map(|i| format!("node_{}", i)).collect();

        let selected = detector.select_nodes(&nodes);

        assert_eq!(selected.len(), 4);
        for node in &selected {
            assert!(nodes.contains(node));
        }
    }

    #[test]
    fn test_select_nodes_empty() {
        let detector = HangDetector::new(test_config());
        let nodes: Vec<String> = vec![];

        let selected = detector.select_nodes(&nodes);

        assert!(selected.is_empty());
    }

    /// 节点重选后的第一个 tick 不应当触发 status 切换为 Normal
    #[test]
    fn test_no_signal_preserves_status() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let detector = HangDetector::new(test_config());

        // 模拟先进入 HANG
        {
            let state = HANG_STATE.clone();
            let mut s = state.write().unwrap();
            s.enter_hang();
        }

        // 新节点首次采样 -> 无历史 -> NoSignal
        let stacks = vec![vec!["foo (a.py:1)".to_string(), "bar (b.py:2)".to_string()]];
        let (obs, _) = detector.process_node_stacks("newnode", stacks);
        assert_eq!(obs, NodeObservation::NoSignal);

        // update_global_status 接收 NoSignal -> 保持 HANG
        let status = detector.update_global_status(&[(
            "newnode".to_string(),
            NodeObservation::NoSignal,
            0.0,
        )]);
        assert_eq!(status, HangStatus::Hang);
    }

    /// 持续 HANG 期间，状态在节点重选 tick 之间不会被清空
    #[test]
    fn test_persistent_hang_keeps_event_id_across_reselection() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let detector = HangDetector::new(test_config());

        // 进入 HANG
        {
            let state = HANG_STATE.clone();
            let mut s = state.write().unwrap();
            s.enter_hang();
            s.mark_notified();
            s.mark_logged();
        }
        let event_id_before = HANG_STATE.read().unwrap().hang_event_id;
        assert!(event_id_before.is_some());

        // 模拟新一轮节点重选：reset_round + 第一个 tick NoSignal
        detector.reset_round();
        let status = detector.update_global_status(&[(
            "n".to_string(),
            NodeObservation::NoSignal,
            0.0,
        )]);
        assert_eq!(status, HangStatus::Hang);

        let event_id_after = HANG_STATE.read().unwrap().hang_event_id;
        assert_eq!(event_id_before, event_id_after);

        // 标志仍保留 -> 不会重复通知 / 写日志
        let s = HANG_STATE.read().unwrap();
        assert!(s.hang_notified);
        assert!(s.hang_logged);
    }

    /// 连续 N 次 Normal 才视为恢复
    #[test]
    fn test_recovery_requires_consecutive_normals() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let detector = HangDetector::new(test_config());

        {
            let state = HANG_STATE.clone();
            let mut s = state.write().unwrap();
            s.enter_hang();
            s.mark_notified();
        }

        // 第一次 Normal -> 仍保持 Hang，避免抖动
        let status = detector.update_global_status(&[(
            "n".to_string(),
            NodeObservation::Normal,
            0.5,
        )]);
        assert_eq!(status, HangStatus::Hang);
        assert!(HANG_STATE.read().unwrap().hang_event_id.is_some());

        // 第二次 Normal -> 达到测试配置的恢复阈值
        let status = detector.update_global_status(&[(
            "n".to_string(),
            NodeObservation::Normal,
            0.5,
        )]);
        assert_eq!(status, HangStatus::Normal);
        let s = HANG_STATE.read().unwrap();
        assert!(s.hang_event_id.is_none());
        assert!(!s.hang_notified);
    }

    /// 命中恢复黑名单时，不应累计恢复 Normal 轮次
    #[test]
    fn test_recovery_blocking_patterns_prevent_recovery_counting() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let detector = HangDetector::new(test_config());

        {
            let state = HANG_STATE.clone();
            let mut s = state.write().unwrap();
            s.enter_hang();
            s.mark_notified();
        }

        // 先累计 1 轮 Normal，确认后续黑名单会重置恢复计数。
        let status = detector.update_global_status(&[(
            "n".to_string(),
            NodeObservation::Normal,
            0.5,
        )]);
        assert_eq!(status, HangStatus::Hang);
        assert_eq!(HANG_STATE.read().unwrap().consecutive_normal_count, 1);

        let history_stack = vec!["training_loop (train.py:10)".to_string()];
        let stacks: Vec<Vec<String>> = (0..4).map(|_| history_stack.clone()).collect();
        assert_eq!(
            detector.process_node_stacks("blocked-node", stacks).0,
            NodeObservation::NoSignal
        );

        let blocked_stack = vec![
            "Py_FinalizeEx (:0)".to_string(),
            "c10d::ProcessGroupMCCL::~ProcessGroupMCCL() (:0)".to_string(),
        ];
        let stacks: Vec<Vec<String>> = (0..4).map(|_| blocked_stack.clone()).collect();
        let (obs, sim) = detector.process_node_stacks("blocked-node", stacks);
        assert_eq!(obs, NodeObservation::RecoveryBlocked);

        let status = detector.update_global_status(&[("blocked-node".to_string(), obs, sim)]);
        assert_eq!(status, HangStatus::Hang);
        let s = HANG_STATE.read().unwrap();
        assert!(s.hang_event_id.is_some());
        assert_eq!(s.consecutive_normal_count, 0);
    }

    /// 某 rank 持续采集失败（如 kill -STOP 后 py-spy attach 卡死）应当被识别为 HANG
    #[test]
    fn test_persistent_fetch_failure_triggers_hang() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let mut cfg = test_config();
        cfg.sample_count = 3;
        cfg.node_rank_quorum = 0.25; // 单节点 4 rank，1 个失败即满足 quorum
        let detector = HangDetector::new(cfg);

        // 第 1 轮：4 个 rank 都正常采集，建立历史
        let normal_stack = vec!["foo (a.py:1)".to_string(), "bar (b.py:2)".to_string()];
        let stacks: Vec<Vec<String>> = (0..4).map(|_| normal_stack.clone()).collect();
        let (obs1, _) = detector.process_node_stacks("node1", stacks);
        // 第一次没有历史可比 -> NoSignal
        assert_eq!(obs1, NodeObservation::NoSignal);

        // 第 2~4 轮：rank 0 fetch 超时（空 Vec），其他 rank 正常但堆栈变化（不算 hang）
        let mut last_obs = NodeObservation::NoSignal;
        for round in 0..3 {
            let mut stacks: Vec<Vec<String>> = vec![Vec::new()]; // rank 0 失败
            for i in 1..4 {
                // 其他 rank 堆栈每轮都不同 -> 不会 high_similarity
                stacks.push(vec![
                    format!("foo_{}_{} (a.py:1)", round, i),
                    format!("bar_{}_{} (b.py:2)", round, i),
                ]);
            }
            let (obs, _) = detector.process_node_stacks("node1", stacks);
            last_obs = obs;
        }

        // rank 0 连续 3 次失败 >= sample_count(3) -> 计入 hang rank
        // quorum_threshold = ceil(4 * 0.25) = 1 -> 1 个 hang rank 即可触发
        assert_eq!(
            last_obs,
            NodeObservation::Hang,
            "持续采集失败的 rank 应当触发 HANG"
        );
    }

    /// 节点首轮全失败应当返回 NoSignal（避免冷启动误报）
    #[test]
    fn test_first_round_all_failure_is_no_signal() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let detector = HangDetector::new(test_config());

        let stacks: Vec<Vec<String>> = (0..4).map(|_| Vec::new()).collect();
        let (obs, _) = detector.process_node_stacks("brandnew", stacks);
        assert_eq!(obs, NodeObservation::NoSignal);
    }

    /// quorum=1.0 时必须**所有 rank**都满足条件才判节点 HANG
    #[test]
    fn test_quorum_full_requires_all_ranks() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let mut cfg = test_config();
        cfg.sample_count = 2;
        cfg.node_rank_quorum = 1.0; // 全员制
        let detector = HangDetector::new(cfg);

        let frozen = vec!["foo (a.py:1)".to_string(), "bar (b.py:2)".to_string()];
        let make_changing = |round: usize, idx: usize| -> Vec<String> {
            vec![
                format!("foo_{}_{} (a.py:1)", round, idx),
                format!("bar_{}_{} (b.py:2)", round, idx),
            ]
        };

        // 建立第 1 轮历史
        let stacks_round0: Vec<Vec<String>> = (0..4).map(|i| make_changing(0, i)).collect();
        let (obs0, _) = detector.process_node_stacks("nodeA", stacks_round0);
        assert_eq!(obs0, NodeObservation::NoSignal);

        // 第 2、3 轮：rank 0~2 完全冻结（高相似），rank 3 持续变化
        for round in 1..=2 {
            let mut stacks: Vec<Vec<String>> =
                (0..3).map(|_| frozen.clone()).collect();
            stacks.push(make_changing(round, 3));
            let (obs, _) = detector.process_node_stacks("nodeA", stacks);
            // quorum=1.0 → 哪怕 3/4 hang，仍然 Normal
            assert_eq!(
                obs,
                NodeObservation::Normal,
                "round {} 仍有 1 个 rank 在变化，quorum=full 不应判 HANG",
                round
            );
        }

        // 让 rank 3 也冻结：连续 sample_count(2) 轮高相似 → 全员 hang → Hang
        // 先让 rank 3 进入 frozen，本轮 sim=jaccard(prev_changing, frozen) 通常 <0.95，
        // high_similarity_count 重置为 0，所以**两轮**才能再次累计到 sample_count
        for round in 0..3 {
            let stacks: Vec<Vec<String>> = (0..4).map(|_| frozen.clone()).collect();
            let (obs, _) = detector.process_node_stacks("nodeA", stacks);
            if round < 2 {
                // rank 3 还没攒够，至少不会判 Hang
                assert_ne!(obs, NodeObservation::Hang);
            } else {
                assert_eq!(
                    obs,
                    NodeObservation::Hang,
                    "全部 4 个 rank 都连续 {} 轮高相似后必须判 HANG",
                    2
                );
            }
        }
    }

    /// 单节点 hang 不应触发全局 HANG（避免 2 节点小集群单点孤鸣误报）
    #[test]
    fn test_global_min_hang_nodes_blocks_single_node_false_positive() {
        let _guard = GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_state();
        let mut cfg = test_config();
        cfg.global_min_hang_nodes = 2;
        let detector = HangDetector::new(cfg);

        // 2 节点：1 个 hang + 1 个 normal —— 50% 占比但绝对数 < 2，不应判 HANG
        let status = detector.update_global_status(&[
            ("n1".to_string(), NodeObservation::Hang, 1.0),
            ("n2".to_string(), NodeObservation::Normal, 0.3),
        ]);
        assert_ne!(status, HangStatus::Hang, "1/2 hang 不应触发全局 HANG");

        // 2 节点全 hang → 触发
        reset_state();
        let mut cfg = test_config();
        cfg.global_min_hang_nodes = 2;
        let detector = HangDetector::new(cfg);
        let status = detector.update_global_status(&[
            ("n1".to_string(), NodeObservation::Hang, 1.0),
            ("n2".to_string(), NodeObservation::Hang, 1.0),
        ]);
        assert_eq!(status, HangStatus::Hang);

        // 单节点集群：effective_min 自动夹紧到 1，不应被门槛永久阻塞
        reset_state();
        let mut cfg = test_config();
        cfg.global_min_hang_nodes = 2;
        let detector = HangDetector::new(cfg);
        let status = detector.update_global_status(&[(
            "only".to_string(),
            NodeObservation::Hang,
            1.0,
        )]);
        assert_eq!(status, HangStatus::Hang, "1 节点集群应允许触发");
    }
}
