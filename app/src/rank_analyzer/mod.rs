//! 问题 Rank 自动检测模块
//!
//! 基于 StackTrie 分叉检测算法，识别执行路径偏离多数 rank 的异常 rank

pub mod analyzer;
pub mod config;
pub mod state;
pub mod types;

pub use analyzer::analyze_trie;
pub use config::RankAnalysisConfig;
pub use state::{get_last_analysis, set_last_analysis};
pub use types::{AnalysisTrigger, DivergencePoint, ProblematicRank, RankAnalysisResult};
