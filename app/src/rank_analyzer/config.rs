//! 问题 Rank 分析配置

use std::env;

/// Rank 分析配置
#[derive(Debug, Clone)]
pub struct RankAnalysisConfig {
    /// 是否启用 Rank 分析（当 HANG 检测启用时跟随生效）
    pub enabled: bool,
    /// 少数派阈值 (0.0 - 1.0)，覆盖率低于此值的分支被视为少数派
    pub minority_threshold: f64,
}

impl Default for RankAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            minority_threshold: 0.3,
        }
    }
}

impl RankAnalysisConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = env::var("RANK_ANALYSIS_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }

        if let Ok(val) = env::var("RANK_ANALYSIS_MINORITY_THRESHOLD") {
            if let Ok(threshold) = val.parse::<f64>() {
                config.minority_threshold = threshold.clamp(0.05, 0.5);
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Prevent parallel env var access in tests
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_default_config() {
        let config = RankAnalysisConfig::default();
        assert!(config.enabled);
        assert!((config.minority_threshold - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        env::set_var("RANK_ANALYSIS_MINORITY_THRESHOLD", "0.2");
        let config = RankAnalysisConfig::from_env();
        env::remove_var("RANK_ANALYSIS_MINORITY_THRESHOLD");
        assert!((config.minority_threshold - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_threshold_clamped() {
        let _lock = ENV_LOCK.lock().unwrap();
        env::set_var("RANK_ANALYSIS_MINORITY_THRESHOLD", "0.01");
        let config = RankAnalysisConfig::from_env();
        env::remove_var("RANK_ANALYSIS_MINORITY_THRESHOLD");
        assert!((config.minority_threshold - 0.05).abs() < f64::EPSILON);

        env::set_var("RANK_ANALYSIS_MINORITY_THRESHOLD", "0.9");
        let config = RankAnalysisConfig::from_env();
        env::remove_var("RANK_ANALYSIS_MINORITY_THRESHOLD");
        assert!((config.minority_threshold - 0.5).abs() < f64::EPSILON);
    }
}
