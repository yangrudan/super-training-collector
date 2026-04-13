//! HANG 检测配置模块
//! 
//! 通过环境变量配置 HANG 检测的各项参数

use std::env;

/// HANG 检测配置
#[derive(Debug, Clone)]
pub struct HangConfig {
    /// 是否启用 HANG 检测
    pub enabled: bool,
    /// 采样间隔（秒），默认 30
    pub sample_interval_secs: u64,
    /// 连续采样次数，默认 3
    pub sample_count: usize,
    /// 采样节点数，默认 4
    pub node_count: usize,
    /// Jaccard 判定阈值，默认 0.95
    pub jaccard_threshold: f64,
    /// 已知长阻塞模式（白名单）
    pub blocking_patterns: Vec<String>,
    /// 是否启用 HANG 日志记录
    pub log_enabled: bool,
    /// HANG 日志保存目录
    pub log_dir: String,
}

impl Default for HangConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_interval_secs: 30,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: vec![
                "checkpoint".to_string(),
                "save_model".to_string(),
                "load_data".to_string(),
                "DataLoader".to_string(),
            ],
            log_enabled: true,
            log_dir: "hang_logs".to_string(),
        }
    }
}

impl HangConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Self {
        let mut config = Self::default();
        
        // HANG_CHECK_ENABLED: 是否启用
        if let Ok(val) = env::var("HANG_CHECK_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }
        
        // HANG_SAMPLE_INTERVAL: 采样间隔（秒）
        if let Ok(val) = env::var("HANG_SAMPLE_INTERVAL") {
            if let Ok(secs) = val.parse::<u64>() {
                config.sample_interval_secs = secs.max(10); // 最小 10 秒
            }
        }
        
        // HANG_SAMPLE_COUNT: 连续采样次数
        if let Ok(val) = env::var("HANG_SAMPLE_COUNT") {
            if let Ok(count) = val.parse::<usize>() {
                config.sample_count = count.max(2).min(10); // 范围 [2, 10]
            }
        }
        
        // HANG_NODE_COUNT: 采样节点数
        if let Ok(val) = env::var("HANG_NODE_COUNT") {
            if let Ok(count) = val.parse::<usize>() {
                config.node_count = count.max(1).min(16); // 范围 [1, 16]
            }
        }
        
        // HANG_JACCARD_THRESHOLD: Jaccard 阈值
        if let Ok(val) = env::var("HANG_JACCARD_THRESHOLD") {
            if let Ok(threshold) = val.parse::<f64>() {
                config.jaccard_threshold = threshold.max(0.5).min(1.0); // 范围 [0.5, 1.0]
            }
        }
        
        // HANG_BLOCKING_PATTERNS: 白名单模式（逗号分隔）
        if let Ok(val) = env::var("HANG_BLOCKING_PATTERNS") {
            if !val.is_empty() {
                config.blocking_patterns = val
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        
        // HANG_LOG_ENABLED: 是否启用日志记录
        if let Ok(val) = env::var("HANG_LOG_ENABLED") {
            config.log_enabled = val.to_lowercase() == "true" || val == "1";
        }
        
        // HANG_LOG_DIR: 日志保存目录
        if let Ok(val) = env::var("HANG_LOG_DIR") {
            if !val.is_empty() {
                config.log_dir = val;
            }
        }
        
        config
    }
    
    /// 检查堆栈是否匹配已知的长阻塞模式
    pub fn is_known_blocking(&self, frames: &[String]) -> bool {
        frames.iter().any(|frame| {
            self.blocking_patterns
                .iter()
                .any(|pattern| frame.contains(pattern))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HangConfig::default();
        
        assert!(!config.enabled);
        assert_eq!(config.sample_interval_secs, 30);
        assert_eq!(config.sample_count, 3);
        assert_eq!(config.node_count, 4);
        assert_eq!(config.jaccard_threshold, 0.95);
        assert!(!config.blocking_patterns.is_empty());
        assert!(config.log_enabled);
        assert_eq!(config.log_dir, "hang_logs");
    }

    #[test]
    fn test_is_known_blocking() {
        let config = HangConfig::default();
        
        let frames_with_checkpoint = vec![
            "main".to_string(),
            "train_loop".to_string(),
            "save_checkpoint".to_string(),  // 包含 "checkpoint"
        ];
        assert!(config.is_known_blocking(&frames_with_checkpoint));
        
        let normal_frames = vec![
            "main".to_string(),
            "forward".to_string(),
            "backward".to_string(),
        ];
        assert!(!config.is_known_blocking(&normal_frames));
    }

    #[test]
    fn test_from_env() {
        // 设置环境变量
        env::set_var("HANG_CHECK_ENABLED", "true");
        env::set_var("HANG_SAMPLE_INTERVAL", "60");
        env::set_var("HANG_JACCARD_THRESHOLD", "0.98");
        
        let config = HangConfig::from_env();
        
        assert!(config.enabled);
        assert_eq!(config.sample_interval_secs, 60);
        assert_eq!(config.jaccard_threshold, 0.98);
        
        // 清理环境变量
        env::remove_var("HANG_CHECK_ENABLED");
        env::remove_var("HANG_SAMPLE_INTERVAL");
        env::remove_var("HANG_JACCARD_THRESHOLD");
    }
}
