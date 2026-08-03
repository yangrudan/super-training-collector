//! 问题 Rank 分析共享类型
//!
//! 这些类型需要在 SSR 和客户端都能使用

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParallelDimension {
    PP,
    DP,
    TP,
    EP,
    CP,
}

impl ParallelDimension {
    pub fn label(self) -> &'static str {
        match self {
            Self::PP => "PP",
            Self::DP => "DP",
            Self::TP => "TP",
            Self::EP => "EP",
            Self::CP => "CP",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParallelEvidenceKind {
    GroupOutlier,
    GroupCorrelated,
    ReplicaRepeated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum RootCauseConfidence {
    #[default]
    Low,
    Medium,
    High,
}

impl RootCauseConfidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Medium => "中",
            Self::High => "高",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RankParallelContext {
    pub pp_rank: Option<u32>,
    pub dp_rank: Option<u32>,
    pub tp_rank: Option<u32>,
    pub ep_rank: Option<u32>,
    pub cp_rank: Option<u32>,
    pub expert_dp_rank: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelEvidence {
    pub dimension: ParallelDimension,
    pub kind: ParallelEvidenceKind,
    pub group_ranks: Vec<u32>,
    pub matching_ranks: Vec<u32>,
    pub coverage: f64,
    pub frame_name: String,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelTopologySummary {
    pub available: bool,
    pub framework: String,
    pub tp_size: u32,
    pub pp_size: u32,
    pub dp_size: u32,
    pub ep_size: u32,
    pub cp_size: u32,
    pub rank_order: String,
    pub degraded_reason: Option<String>,
}

impl Default for ParallelTopologySummary {
    fn default() -> Self {
        Self {
            available: false,
            framework: "megatron".to_string(),
            tp_size: 1,
            pp_size: 1,
            dp_size: 1,
            ep_size: 1,
            cp_size: 1,
            rank_order: String::new(),
            degraded_reason: Some("未获取到 Megatron 并行拓扑".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DivergencePoint {
    pub frame_name: String,
    pub depth: u32,
    pub majority_count: u32,
    pub minority_count: u32,
    pub minority_coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblematicRank {
    pub rank_id: u32,
    pub node_ip: Option<String>,
    pub issue_reason: Option<String>,
    pub anomaly_score: u32,
    pub divergence_points: Vec<DivergencePoint>,
    #[serde(default)]
    pub parallel_context: Option<RankParallelContext>,
    #[serde(default)]
    pub suspected_dimensions: Vec<ParallelDimension>,
    #[serde(default)]
    pub root_cause_confidence: RootCauseConfidence,
    #[serde(default)]
    pub parallel_evidence: Vec<ParallelEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnalysisTrigger {
    HangDetected,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankAnalysisResult {
    pub problematic_ranks: Vec<ProblematicRank>,
    pub total_ranks: u32,
    pub analysis_duration_ms: u64,
    pub trigger: AnalysisTrigger,
    pub timestamp: u64,
    pub minority_threshold: f64,
    #[serde(default)]
    pub parallel_topology: ParallelTopologySummary,
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
            parallel_topology: ParallelTopologySummary::default(),
        }
    }
}
