//! HANG 检测调度模块
//! 
//! 定期触发 HANG 检测的调度逻辑

use super::config::HangConfig;
use super::detector::HangDetector;

/// HANG 检测调度器
pub struct HangScheduler {
    detector: HangDetector,
    config: HangConfig,
}

impl HangScheduler {
    /// 创建新的调度器
    pub fn new(config: HangConfig) -> Self {
        let detector = HangDetector::new(config.clone());
        Self { detector, config }
    }
    
    /// 从环境变量创建调度器
    pub fn from_env() -> Self {
        let config = HangConfig::from_env();
        Self::new(config)
    }
    
    /// 检查是否启用
    pub fn is_enabled(&self) -> bool {
        self.detector.is_enabled()
    }
    
    /// 获取采样间隔（秒）
    pub fn sample_interval_secs(&self) -> u64 {
        self.config.sample_interval_secs
    }
    
    /// 获取检测器引用
    pub fn detector(&self) -> &HangDetector {
        &self.detector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let config = HangConfig {
            enabled: true,
            sample_interval_secs: 30,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: vec!["checkpoint".to_string()],
            log_enabled: true,
            log_dir: "hang_logs".to_string(),
        };
        
        let scheduler = HangScheduler::new(config);
        assert!(scheduler.is_enabled());
        assert_eq!(scheduler.sample_interval_secs(), 30);
    }
}
