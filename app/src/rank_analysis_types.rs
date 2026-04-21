//! 问题 Rank 分析共享类型
//!
//! 这些类型需要在 SSR 和客户端都能使用

use serde::{Deserialize, Serialize};

/// 分叉点描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DivergencePoint {
    /// 分叉处的函数名
    pub frame_name: String,
    /// 调用深度
    pub depth: u32,
    /// 多数派 rank 数量
    pub majority_count: u32,
    /// 少数派 rank 数量
    pub minority_count: u32,
    /// 少数派覆盖率 (0.0 - 1.0)
    pub minority_coverage: f64,
}

/// 单个问题 Rank 的信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblematicRank {
    /// Rank ID
    pub rank_id: u32,
    /// 所属节点 IP（如果可用）
    pub node_ip: Option<String>,
    /// 非分叉类异常说明（如调用栈采集失败）
    pub issue_reason: Option<String>,
    /// 异常分数（出现在少数派分支的次数）
    pub anomaly_score: u32,
    /// 该 rank 涉及的分叉点列表
    pub divergence_points: Vec<DivergencePoint>,
}

/// 触发来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnalysisTrigger {
    /// HANG 检测后自动触发
    HangDetected,
    /// Dashboard 手动触发
    Manual,
}

/// Rank 分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankAnalysisResult {
    /// 问题 rank 列表（按 anomaly_score 降序）
    pub problematic_ranks: Vec<ProblematicRank>,
    /// 总 rank 数
    pub total_ranks: u32,
    /// 分析耗时（毫秒）
    pub analysis_duration_ms: u64,
    /// 触发来源
    pub trigger: AnalysisTrigger,
    /// 分析时间戳（Unix 秒）
    pub timestamp: u64,
    /// 少数派阈值
    pub minority_threshold: f64,
}

impl Default for RankAnalysisResult {
    fn default() -> Self {
        Self {
            problematic_ranks: Vec::new(),
            total_ranks: 0,
            analysis_duration_ms: 0,
            trigger: AnalysisTrigger::Manual,
            timestamp: 0,
            minority_threshold: 0.3,
        }
    }
}
