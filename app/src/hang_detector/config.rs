//! HANG 检测配置模块
//!
//! 通过环境变量配置 HANG 检测的各项参数

use std::env;

/// HANG 检测配置
#[derive(Debug, Clone)]
pub struct HangConfig {
    /// 是否启用 HANG 检测
    pub enabled: bool,
    /// 采样间隔下限（秒），每 tick 在 [min, max] 随机
    pub sample_interval_min_secs: u64,
    /// 采样间隔上限（秒），每 tick 在 [min, max] 随机
    pub sample_interval_max_secs: u64,
    /// 连续采样次数，默认 3
    pub sample_count: usize,
    /// 采样节点数，默认 4
    pub node_count: usize,
    /// Jaccard 判定阈值，默认 0.95
    pub jaccard_threshold: f64,
    /// 已知长阻塞模式（白名单）
    pub blocking_patterns: Vec<String>,
    /// 恢复判定黑名单；命中时不累计恢复 Normal 轮次
    pub recovery_blocking_patterns: Vec<String>,
    /// 是否启用 HANG 日志记录
    pub log_enabled: bool,
    /// HANG 日志保存目录
    pub log_dir: String,
    /// 节点级判定时，至少多少比例的 rank 连续高相似度才算节点 HANG
    pub node_rank_quorum: f64,
    /// 是否在 Jaccard 时保留行号（更敏感于函数内代码推进）
    pub keep_line_numbers: bool,
    /// 连续多少轮 Normal 才视为当前采样未满足 HANG 条件（去抖动）
    pub recovery_normal_rounds: u8,
    /// 全局判 HANG 所需的"最少 hang 节点绝对数"（与 50% 票数共同生效）
    /// 默认 2：避免 2 节点小集群里"1 个节点孤鸣 = 50%"的误报。
    pub global_min_hang_nodes: usize,
    /// 首次检测到 HANG 后，延迟多少秒再发送内网后台告警。
    ///
    /// 钉钉告警仍然立即发送，仅内网告警等待此延迟。延迟结束后再次确认仍处于
    /// HANG 状态时，才会真正发出内网告警，从而过滤掉短暂抖动。默认 20 分钟。
    pub intranet_alert_delay_secs: u64,
}

impl Default for HangConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_interval_min_secs: 50,
            sample_interval_max_secs: 60,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: default_blocking_patterns(),
            recovery_blocking_patterns: default_recovery_blocking_patterns(),
            log_enabled: true,
            log_dir: "hang_logs".to_string(),
            node_rank_quorum: 1.0,
            keep_line_numbers: true,
            recovery_normal_rounds: 3,
            global_min_hang_nodes: 2,
            intranet_alert_delay_secs: 20 * 60,
        }
    }
}

/// 默认的"已知长阻塞"白名单
///
/// **默认为空**：实践中常见关键字（如 `checkpoint`、`DataLoader`）会与
/// Megatron-LM 的 activation checkpointing (`_checkpointed_forward` /
/// `checkpoint_handler`) 以及 PyTorch 训练栈中无处不在的 `DataLoader`
/// 帧发生子串撞名，反而会**掩盖真正的 HANG**。
///
/// 如确需启用业务级白名单，请通过环境变量 `HANG_BLOCKING_PATTERNS`
/// 显式配置，并优先选择高特异性的函数名（例如 `save_checkpoint_to_disk`
/// 而非 `checkpoint`）。
///
/// **严禁加入 NCCL / c10d / CUDA 同步 / futex / epoll 等同步原语**：
/// 真正的训练 HANG（如某 rank 死锁、慢节点）正是表现为其他 rank 全部卡在
/// `ncclAllReduce` / `ProcessGroupNCCL` 等待，把它们放进白名单会直接掩盖事件。
pub fn default_blocking_patterns() -> Vec<String> {
    Vec::new()
}

/// 默认恢复判定黑名单
///
/// 这些栈通常出现在 Python/MCCL 退出或进程组销毁路径上。训练任务已经处于
/// HANG 状态时，采样到这些帧不能说明任务恢复正常，因此不累计恢复轮次。
pub fn default_recovery_blocking_patterns() -> Vec<String> {
    vec!["Py_FinalizeEx".to_string(), "~ProcessGroupMCCL".to_string()]
}

impl HangConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // HANG_CHECK_ENABLED: 是否启用
        if let Ok(val) = env::var("HANG_CHECK_ENABLED") {
            config.enabled = val.to_lowercase() == "true" || val == "1";
        }

        // HANG_SAMPLE_INTERVAL: 采样间隔（秒），向后兼容：设为固定值时 min=max
        if let Ok(val) = env::var("HANG_SAMPLE_INTERVAL") {
            if let Ok(secs) = val.parse::<u64>() {
                let s = secs.max(10);
                config.sample_interval_min_secs = s;
                config.sample_interval_max_secs = s;
            }
        }

        // HANG_SAMPLE_INTERVAL_MIN_SECS / _MAX_SECS: 显式区间（优先级高于 HANG_SAMPLE_INTERVAL）
        if let Ok(val) = env::var("HANG_SAMPLE_INTERVAL_MIN_SECS") {
            if let Ok(secs) = val.parse::<u64>() {
                config.sample_interval_min_secs = secs.max(10);
            }
        }
        if let Ok(val) = env::var("HANG_SAMPLE_INTERVAL_MAX_SECS") {
            if let Ok(secs) = val.parse::<u64>() {
                config.sample_interval_max_secs = secs.max(10);
            }
        }
        // 保证 min <= max
        if config.sample_interval_min_secs > config.sample_interval_max_secs {
            std::mem::swap(
                &mut config.sample_interval_min_secs,
                &mut config.sample_interval_max_secs,
            );
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
                config.recovery_normal_rounds = r.max(1).min(3);
            }
        }

        // HANG_GLOBAL_MIN_HANG_NODES: 全局判 HANG 所需的"最少 hang 节点绝对数"
        // 与 50% 票数共同生效；默认 2，避免单节点孤鸣误报
        if let Ok(val) = env::var("HANG_GLOBAL_MIN_HANG_NODES") {
            if let Ok(n) = val.parse::<usize>() {
                config.global_min_hang_nodes = n.max(1);
            }
        }

        // HANG_INTRANET_ALERT_DELAY_SECS: 首次检测 HANG 到发送内网告警之间的延迟（秒）
        // 钉钉告警不受此参数影响，依然立即发送。默认 1200 秒（20 分钟）。
        if let Ok(val) = env::var("HANG_INTRANET_ALERT_DELAY_SECS") {
            if let Ok(secs) = val.parse::<u64>() {
                config.intranet_alert_delay_secs = secs;
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

    /// 检查堆栈是否命中恢复判定黑名单
    pub fn is_recovery_blocking(&self, frames: &[String]) -> bool {
        frames.iter().any(|frame| {
            self.recovery_blocking_patterns
                .iter()
                .any(|pattern| frame.contains(pattern))
        })
    }

    /// 采样间隔的"代表值"（区间均值），用于 backdate 等需要确定值的场景
    pub fn sample_interval_secs(&self) -> u64 {
        let min = self.sample_interval_min_secs;
        let max = self.sample_interval_max_secs.max(min);
        (min + max) / 2
    }

    /// 在配置区间内随机抽取一个采样间隔（含端点）
    pub fn random_sample_interval_secs(&self) -> u64 {
        let min = self.sample_interval_min_secs;
        let max = self.sample_interval_max_secs.max(min);
        if min == max {
            return min;
        }
        use rand::Rng;
        rand::thread_rng().gen_range(min..=max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // 序列化所有修改进程级 env 的测试，避免并行竞态污染 from_env() 结果
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_default_config() {
        let config = HangConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.sample_interval_min_secs, 50);
        assert_eq!(config.sample_interval_max_secs, 60);
        assert_eq!(config.sample_interval_secs(), 55);
        assert_eq!(config.sample_count, 3);
        assert_eq!(config.node_count, 4);
        assert_eq!(config.jaccard_threshold, 0.95);
        // 默认无白名单：避免 `checkpoint` / `DataLoader` 子串撞名误屏蔽真 HANG
        assert!(config.blocking_patterns.is_empty());
        assert_eq!(
            config.recovery_blocking_patterns,
            vec!["Py_FinalizeEx", "~ProcessGroupMCCL"]
        );
        assert!(config.log_enabled);
        assert_eq!(config.log_dir, "hang_logs");
        // 默认要求全部 rank 都满足条件才判节点 HANG（误报率优先）
        assert_eq!(config.node_rank_quorum, 1.0);
        assert!(config.keep_line_numbers);
        assert_eq!(config.recovery_normal_rounds, 3);
        // 默认要求至少 2 个节点同时 hang 才判全局 HANG，避免 2 节点小集群单点孤鸣误报
        assert_eq!(config.global_min_hang_nodes, 2);
    }

    #[test]
    fn test_default_blocking_patterns_exclude_sync_primitives() {
        // 默认白名单应当为空：实践证明 `checkpoint`、`DataLoader` 等关键字
        // 会与 Megatron 的 activation checkpointing / 训练栈中无处不在的
        // DataLoader 帧子串撞名，反而掩盖真 HANG。
        let patterns = default_blocking_patterns();
        assert!(patterns.is_empty(), "默认白名单必须为空，避免误屏蔽真 HANG");

        // 关键回归保护：NCCL/c10d/CUDA 同步原语**绝不**能进默认白名单，
        // 否则真正的训练 HANG（典型表现就是这些原语阻塞）会被掩盖。
        let joined = patterns.join(",").to_lowercase();
        for forbidden in [
            "nccl",
            "processgroupnccl",
            "cuda",
            "c10d",
            "pthread_cond",
            "futex",
            "epoll",
        ] {
            assert!(
                !joined.contains(forbidden),
                "默认白名单不应包含同步原语: {}",
                forbidden
            );
        }
    }

    #[test]
    fn test_is_known_blocking() {
        // 默认配置下白名单为空，任何栈都不应命中
        let config = HangConfig::default();
        let frames = vec![
            "save_checkpoint".to_string(),
            "DataLoader".to_string(),
            "_checkpointed_forward".to_string(),
        ];
        assert!(!config.is_known_blocking(&frames));

        // 显式配置白名单后才生效
        let mut config_with_pattern = HangConfig::default();
        config_with_pattern.blocking_patterns = vec!["save_checkpoint_to_disk".to_string()];

        let frames_hit = vec![
            "main".to_string(),
            "save_checkpoint_to_disk".to_string(),
        ];
        assert!(config_with_pattern.is_known_blocking(&frames_hit));

        let normal_frames = vec![
            "main".to_string(),
            "forward".to_string(),
            "backward".to_string(),
        ];
        assert!(!config_with_pattern.is_known_blocking(&normal_frames));

        // 关键回归：Megatron activation checkpointing 不应被 `save_checkpoint_to_disk`
        // 这种特异关键字误命中
        let megatron_frames = vec![
            "_checkpointed_forward".to_string(),
            "checkpoint_handler".to_string(),
            "checkpoint".to_string(),
        ];
        assert!(!config_with_pattern.is_known_blocking(&megatron_frames));
    }

    #[test]
    fn test_from_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 清理可能残留的 env，确保 HANG_SAMPLE_INTERVAL 走兼容路径
        env::remove_var("HANG_SAMPLE_INTERVAL_MIN_SECS");
        env::remove_var("HANG_SAMPLE_INTERVAL_MAX_SECS");
        // 设置环境变量
        env::set_var("HANG_CHECK_ENABLED", "true");
        env::set_var("HANG_SAMPLE_INTERVAL", "60");
        env::set_var("HANG_JACCARD_THRESHOLD", "0.98");

        let config = HangConfig::from_env();

        assert!(config.enabled);
        // 旧 HANG_SAMPLE_INTERVAL 兼容：min == max
        assert_eq!(config.sample_interval_min_secs, 60);
        assert_eq!(config.sample_interval_max_secs, 60);
        assert_eq!(config.sample_interval_secs(), 60);
        assert_eq!(config.jaccard_threshold, 0.98);

        // 清理环境变量
        env::remove_var("HANG_CHECK_ENABLED");
        env::remove_var("HANG_SAMPLE_INTERVAL");
        env::remove_var("HANG_JACCARD_THRESHOLD");
    }

    #[test]
    fn test_interval_range_from_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 显式区间应覆盖 HANG_SAMPLE_INTERVAL
        env::remove_var("HANG_SAMPLE_INTERVAL");
        env::set_var("HANG_SAMPLE_INTERVAL_MIN_SECS", "20");
        env::set_var("HANG_SAMPLE_INTERVAL_MAX_SECS", "40");
        let config = HangConfig::from_env();
        assert_eq!(config.sample_interval_min_secs, 20);
        assert_eq!(config.sample_interval_max_secs, 40);
        assert_eq!(config.sample_interval_secs(), 30);
        for _ in 0..32 {
            let v = config.random_sample_interval_secs();
            assert!(v >= 20 && v <= 40, "random {} out of [20,40]", v);
        }
        env::remove_var("HANG_SAMPLE_INTERVAL_MIN_SECS");
        env::remove_var("HANG_SAMPLE_INTERVAL_MAX_SECS");

        // min > max 时自动交换
        env::set_var("HANG_SAMPLE_INTERVAL_MIN_SECS", "80");
        env::set_var("HANG_SAMPLE_INTERVAL_MAX_SECS", "30");
        let config = HangConfig::from_env();
        assert_eq!(config.sample_interval_min_secs, 30);
        assert_eq!(config.sample_interval_max_secs, 80);
        env::remove_var("HANG_SAMPLE_INTERVAL_MIN_SECS");
        env::remove_var("HANG_SAMPLE_INTERVAL_MAX_SECS");

        // 低于 10 强制为 10
        env::set_var("HANG_SAMPLE_INTERVAL_MIN_SECS", "1");
        env::set_var("HANG_SAMPLE_INTERVAL_MAX_SECS", "5");
        let config = HangConfig::from_env();
        assert!(config.sample_interval_min_secs >= 10);
        assert!(config.sample_interval_max_secs >= 10);
        env::remove_var("HANG_SAMPLE_INTERVAL_MIN_SECS");
        env::remove_var("HANG_SAMPLE_INTERVAL_MAX_SECS");
    }

    #[test]
    fn test_log_dir_priority() {
        use std::env;
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

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
