//! HANG 检测核心逻辑模块
//! 
//! 实现节点选择、相似度判定和 HANG 状态更新

use std::collections::HashSet;
use rand::seq::SliceRandom;
use rand::thread_rng;

use super::config::HangConfig;
use super::jaccard::{jaccard_similarity, stack_to_set};
use super::state::{get_hang_state, HangStatus, NodeStackHistory};

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
    /// 
    /// - 如果总节点数 <= N，返回所有节点
    /// - 否则随机选择 N 个
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
    /// 返回该节点是否被判定为可能 HANG
    pub fn process_node_stacks(
        &self,
        node_ip: &str,
        stacks: Vec<Vec<String>>,  // 各个 rank 的堆栈
    ) -> (bool, f64) {
        let state = get_hang_state();
        let mut state = state.write().unwrap();
        
        // 合并所有 rank 的堆栈为一个集合
        let current_set = self.merge_rank_stacks(&stacks);
        
        // 获取或创建该节点的历史记录
        let history = state.node_history
            .entry(node_ip.to_string())
            .or_insert_with(NodeStackHistory::default);
        
        // 计算与上一次的相似度
        let similarity = if let Some(previous) = history.previous() {
            jaccard_similarity(previous, &current_set)
        } else {
            0.0  // 没有历史数据，无法比较
        };
        
        // 更新历史记录
        history.push(current_set.clone(), self.config.sample_count + 1);
        history.last_similarity = similarity;
        
        // 判断是否高相似度
        let is_similar = similarity >= self.config.jaccard_threshold;
        
        if is_similar {
            history.high_similarity_count += 1;
        } else {
            history.high_similarity_count = 0;  // 重置计数
        }
        
        // 检查白名单
        let is_known_blocking = stacks.iter().any(|stack| {
            self.config.is_known_blocking(stack)
        });
        
        // 判断是否 HANG（连续高相似度且不在白名单中）
        let is_hang = history.high_similarity_count >= self.config.sample_count as u8
            && !is_known_blocking;
        
        (is_hang, similarity)
    }
    
    /// 合并多个 rank 的堆栈为一个集合
    fn merge_rank_stacks(&self, stacks: &[Vec<String>]) -> HashSet<String> {
        let mut merged = HashSet::new();
        for stack in stacks {
            merged.extend(stack_to_set(stack));
        }
        merged
    }
    
    /// 根据各节点的检测结果更新全局状态
    /// 
    /// 投票机制：>= 50% 节点被判定为 HANG，则全局状态为 HANG
    pub fn update_global_status(
        &self,
        node_results: &[(String, bool, f64)],  // (node_ip, is_hang, similarity)
    ) -> HangStatus {
        let state = get_hang_state();
        let mut state = state.write().unwrap();
        
        if node_results.is_empty() {
            state.status = HangStatus::Collecting;
            state.touch();
            return HangStatus::Collecting;
        }
        
        let hang_count = node_results.iter().filter(|(_, is_hang, _)| *is_hang).count();
        let total_count = node_results.len();
        
        // 更新详细信息
        state.details.hang_nodes = node_results
            .iter()
            .filter(|(_, is_hang, _)| *is_hang)
            .map(|(ip, _, _)| ip.clone())
            .collect();
        
        state.details.node_similarities = node_results
            .iter()
            .map(|(ip, _, sim)| (ip.clone(), *sim))
            .collect();
        
        // 投票判定
        let new_status = if hang_count * 2 >= total_count {
            // >= 50% 节点 HANG
            state.details.consecutive_high_similarity = self.config.sample_count as u8;
            HangStatus::Hang
        } else if hang_count > 0 {
            // 有节点高相似度，但未达到阈值
            HangStatus::Warning
        } else if state.sample_round < self.config.sample_count as u8 {
            // 还在收集样本
            HangStatus::Collecting
        } else {
            HangStatus::Normal
        };
        
        state.status = new_status.clone();
        state.touch();
        
        new_status
    }
    
    /// 增加采样轮次计数
    pub fn increment_sample_round(&self) {
        let state = get_hang_state();
        let mut state = state.write().unwrap();
        state.sample_round += 1;
    }
    
    /// 重置当前轮次（节点失败时调用）
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
        state.selected_nodes.is_empty() || 
        state.sample_round >= self.config.sample_count as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HangConfig {
        HangConfig {
            enabled: true,
            sample_interval_secs: 30,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: vec!["checkpoint".to_string()],
        }
    }

    #[test]
    fn test_select_nodes_all() {
        let detector = HangDetector::new(test_config());
        let nodes = vec!["n1".to_string(), "n2".to_string()];
        
        let selected = detector.select_nodes(&nodes);
        
        // 节点数 < node_count，返回全部
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_select_nodes_random() {
        let detector = HangDetector::new(test_config());
        let nodes: Vec<String> = (0..10).map(|i| format!("node_{}", i)).collect();
        
        let selected = detector.select_nodes(&nodes);
        
        // 应该只选择 4 个
        assert_eq!(selected.len(), 4);
        // 每个选中的节点都应该在原始列表中
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

    #[test]
    fn test_merge_rank_stacks() {
        let detector = HangDetector::new(test_config());
        let stacks = vec![
            vec!["a (file:1)".to_string(), "b (file:2)".to_string()],
            vec!["b (file:3)".to_string(), "c (file:4)".to_string()],
        ];
        
        let merged = detector.merge_rank_stacks(&stacks);
        
        // 忽略行号后，应该有 3 个不同的元素
        assert_eq!(merged.len(), 3);
        assert!(merged.contains("a (file)"));
        assert!(merged.contains("b (file)"));
        assert!(merged.contains("c (file)"));
    }
}
