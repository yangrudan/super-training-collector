//! Rank 分析结果全局状态管理

use super::types::RankAnalysisResult;
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};

/// 全局分析结果缓存
static RANK_ANALYSIS_STATE: Lazy<Arc<RwLock<Option<RankAnalysisResult>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// 获取最近一次分析结果
pub fn get_last_analysis() -> Option<RankAnalysisResult> {
    RANK_ANALYSIS_STATE.read().ok()?.clone()
}

/// 存储分析结果
pub fn set_last_analysis(result: RankAnalysisResult) {
    if let Ok(mut state) = RANK_ANALYSIS_STATE.write() {
        *state = Some(result);
    }
}
