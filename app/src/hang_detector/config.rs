//! HANG 检测配置模块
//!
//! 通过环境变量配置 HANG 检测的各项参数

use std::env;

/// HANG 检测配置
#[derive(Debug, Clone)]
pub struct HangConfig {
    /// 是否启用 HANG 检测
    pub enabled: bool,
    /// 采样间隔（秒），默认 60
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
    /// 节点级判定时，至少多少比例的 rank 连续高相似度才算节点 HANG
    pub node_rank_quorum: f64,
    /// 是否在 Jaccard 时保留行号（更敏感于函数内代码推进）
    pub keep_line_numbers: bool,
    /// 连续多少轮 Normal 才视为真正恢复（去抖动）
    pub recovery_normal_rounds: u8,
}

impl Default for HangConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_interval_secs: 60,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: default_blocking_patterns(),
            log_enabled: true,
            log_dir: "hang_logs".to_string(),
            node_rank_quorum: 0.5,
            keep_line_numbers: true,
            recovery_normal_rounds: 2,
        }
    }
}

/// 默认的"已知长阻塞"白名单
///
/// **只包含明确的业务级长阻塞**（如 checkpoint 写盘、首轮数据加载），
/// 这些操作堆栈在数十秒内保持稳定但属于正常行为。
///
/// **严禁加入 NCCL / c10d / CUDA 同步 / futex / epoll 等同步原语**：
/// 真正的训练 HANG（如某 rank 死锁、慢节点）正是表现为其他 rank 全部卡在
/// `ncclAllReduce` / `ProcessGroupNCCL` 等待，把它们放进白名单会直接掩盖事件。
pub fn default_blocking_patterns() -> Vec<String> {
    [
        "checkpoint",
        "save_model",
        "load_data",
        "DataLoader",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
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

        // HANG_NODE_RANK_QUORUM: 节点级 rank 占比阈值
        if let Ok(val) = env::var("HANG_NODE_RANK_QUORUM") {
            if let Ok(q) = val.parse::<f64>() {
                config.node_rank_quorum = q.max(0.1).min(1.0);
            }
        }

        // HANG_KEEP_LINE_NUMBERS: 是否保留行号
        if let Ok(val) = env::var("HANG_KEEP_LINE_NUMBERS") {
            config.keep_line_numbers = val.to_lowercase() == "true" || val == "1";
        }

        // HANG_RECOVERY_NORMAL_ROUNDS: 恢复阈值
        if let Ok(val) = env::var("HANG_RECOVERY_NORMAL_ROUNDS") {
            if let Ok(r) = val.parse::<u8>() {
                config.recovery_normal_rounds = r.max(1).min(10);
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
        // 优先级：OUTPUT_DIR/hang_logs > HANG_LOG_DIR > 默认 hang_logs
        if let Ok(output_dir) = env::var("OUTPUT_DIR") {
            if !output_dir.is_empty() {
                // 使用 OUTPUT_DIR 下的 hang_logs 子目录
                use std::path::Path;
                let hang_path = Path::new(&output_dir).join("hang_logs");
                if let Some(hang_path_str) = hang_path.to_str() {
                    config.log_dir = hang_path_str.to_string();
                }
            }
        } else if let Ok(val) = env::var("HANG_LOG_DIR") {
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
        assert_eq!(config.sample_interval_secs, 60);
        assert_eq!(config.sample_count, 3);
        assert_eq!(config.node_count, 4);
        assert_eq!(config.jaccard_threshold, 0.95);
        assert!(!config.blocking_patterns.is_empty());
        assert!(config.log_enabled);
        assert_eq!(config.log_dir, "hang_logs");
        assert_eq!(config.node_rank_quorum, 0.5);
        assert!(config.keep_line_numbers);
        assert_eq!(config.recovery_normal_rounds, 2);
    }

    #[test]
    fn test_default_blocking_patterns_exclude_sync_primitives() {
        // 关键回归保护：NCCL/c10d/CUDA 同步原语**绝不**能进默认白名单，
        // 否则真正的训练 HANG（典型表现就是这些原语阻塞）会被掩盖。
        let patterns = default_blocking_patterns();
        let joined = patterns.join(",");
        for needle in [
            "checkpoint",
            "DataLoader",
        ] {
            assert!(joined.contains(needle), "missing pattern: {}", needle);
        }
        for forbidden in [
            "nccl",
            "ProcessGroupNCCL",
            "cuda",
            "c10d",
            "pthread_cond",
            "futex",
            "epoll",
        ] {
            assert!(
                !joined.to_lowercase().contains(&forbidden.to_lowercase()),
                "默认白名单不应包含同步原语: {}",
                forbidden
            );
        }
    }

    #[test]
    fn test_is_known_blocking() {
        let config = HangConfig::default();

        let frames_with_checkpoint = vec![
            "main".to_string(),
            "train_loop".to_string(),
            "save_checkpoint".to_string(), // 包含 "checkpoint"
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

    #[test]
    fn test_log_dir_priority() {
        use std::env;

        // 清理所有相关环境变量
        env::remove_var("OUTPUT_DIR");
        env::remove_var("HANG_LOG_DIR");

        // 测试默认值
        let config = HangConfig::from_env();
        assert_eq!(config.log_dir, "hang_logs");

        // 测试 HANG_LOG_DIR
        env::set_var("HANG_LOG_DIR", "/custom/hang/path");
        let config = HangConfig::from_env();
        assert_eq!(config.log_dir, "/custom/hang/path");

        // 测试 OUTPUT_DIR 优先级更高
        env::set_var("OUTPUT_DIR", "/output/base");
        let config = HangConfig::from_env();
        assert_eq!(config.log_dir, "/output/base/hang_logs");

        // 清理环境变量
        env::remove_var("OUTPUT_DIR");
        env::remove_var("HANG_LOG_DIR");
    }
}
