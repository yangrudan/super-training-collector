//! HANG 日志记录模块
//!
//! 当检测到 HANG 时，记录堆栈信息到本地文件，并采集全局火焰图

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use chrono::Local;
use serde::Serialize;
use tracing;

use super::config::HangConfig;
use super::detector::NodeObservation;
use super::state::get_hang_state;
use crate::adapter::get_real_training_data;
use crate::flamegraph::{collect_and_generate_flamegraph, get_config_path, load_collector_config};

/// HANG 日志条目
#[derive(Debug, Serialize)]
pub struct HangLogEntry {
    /// 检测时间（ISO 8601 格式）
    pub timestamp: String,
    /// HANG 节点列表
    pub hang_nodes: Vec<String>,
    /// 各节点的 Jaccard 相似度
    pub node_similarities: HashMap<String, f64>,
    /// 各节点的堆栈数据
    pub node_stacks: HashMap<String, Vec<Vec<String>>>,
    /// 连续高相似度次数
    pub consecutive_high_similarity: u8,
    /// 采样配置
    pub config: HangLogConfig,
    /// 触发日志时的检测状态快照
    pub detection_state: HangDetectionStateLog,
    /// 本轮各节点观测结果
    pub node_observations: Vec<HangNodeObservationLog>,
}

/// 日志中的检测状态快照
#[derive(Debug, Serialize)]
pub struct HangDetectionStateLog {
    pub event_id: Option<u64>,
    pub hang_first_detected_at: Option<u64>,
    pub selected_nodes: Vec<String>,
    pub sample_round: u8,
    pub consecutive_normal_count: u8,
    pub hang_notified: bool,
    pub hang_intranet_notified: bool,
    pub hang_notify_in_flight: bool,
    pub pending_recovery: Option<HangPendingRecoveryLog>,
}

/// 日志中的待发送恢复通知信息
#[derive(Debug, Serialize)]
pub struct HangPendingRecoveryLog {
    pub event_id: u64,
    pub hang_duration_secs: u64,
}

/// 日志中的单节点观测结果
#[derive(Debug, Serialize)]
pub struct HangNodeObservationLog {
    pub node_ip: String,
    pub observation: String,
    pub similarity: f64,
}

/// 日志中的配置信息
#[derive(Debug, Serialize)]
pub struct HangLogConfig {
    /// 区间均值（兼容历史字段，便于离线分析）
    pub sample_interval_secs: u64,
    /// 采样间隔下限（秒）
    pub sample_interval_min_secs: u64,
    /// 采样间隔上限（秒）
    pub sample_interval_max_secs: u64,
    pub sample_count: usize,
    pub node_count: usize,
    pub jaccard_threshold: f64,
}

impl From<&HangConfig> for HangLogConfig {
    fn from(config: &HangConfig) -> Self {
        Self {
            sample_interval_secs: config.sample_interval_secs(),
            sample_interval_min_secs: config.sample_interval_min_secs,
            sample_interval_max_secs: config.sample_interval_max_secs,
            sample_count: config.sample_count,
            node_count: config.node_count,
            jaccard_threshold: config.jaccard_threshold,
        }
    }
}

/// HANG 日志记录器
pub struct HangLogger {
    config: HangConfig,
}

impl HangLogger {
    /// 创建新的日志记录器
    pub fn new(config: HangConfig) -> Self {
        Self { config }
    }

    /// 从环境变量创建日志记录器
    pub fn from_env() -> Self {
        Self::new(HangConfig::from_env())
    }

    /// 检查是否启用日志记录
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.config.log_enabled
    }

    /// 获取日志目录
    pub fn log_dir(&self) -> &str {
        &self.config.log_dir
    }

    /// 记录 HANG 事件（异步版本，采集全局火焰图）
    ///
    /// 返回写入的日志文件路径，如果记录失败返回 None
    pub async fn log_hang_event(
        &self,
        node_stacks: HashMap<String, Vec<Vec<String>>>,
        node_results: &[(String, NodeObservation, f64)],
    ) -> Option<String> {
        if !self.is_enabled() {
            tracing::debug!("HANG logging is disabled");
            return None;
        }

        // 检查是否需要记录
        let should_log = {
            let state = get_hang_state();
            let state = state.read().unwrap();
            state.should_log()
        };

        if !should_log {
            tracing::debug!("HANG already logged or not in HANG state");
            return None;
        }

        // 获取状态详情（在锁外复制）
        let (hang_nodes, node_similarities, consecutive_high_similarity, detection_state) = {
            let state = get_hang_state();
            let state = state.read().unwrap();
            let pending_recovery = state
                .pending_recovery
                .map(|(event_id, hang_duration_secs)| HangPendingRecoveryLog {
                    event_id,
                    hang_duration_secs,
                });
            (
                state.details.hang_nodes.clone(),
                state.details.node_similarities.clone(),
                state.details.consecutive_high_similarity,
                HangDetectionStateLog {
                    event_id: state.hang_event_id,
                    hang_first_detected_at: state.hang_first_detected_at,
                    selected_nodes: state.selected_nodes.clone(),
                    sample_round: state.sample_round,
                    consecutive_normal_count: state.consecutive_normal_count,
                    hang_notified: state.hang_notified,
                    hang_intranet_notified: state.hang_intranet_notified,
                    hang_notify_in_flight: state.hang_notify_in_flight,
                    pending_recovery,
                },
            )
        };
        let node_observations = node_results
            .iter()
            .map(
                |(node_ip, observation, similarity)| HangNodeObservationLog {
                    node_ip: node_ip.clone(),
                    observation: format!("{:?}", observation),
                    similarity: *similarity,
                },
            )
            .collect();

        // 构建日志条目
        let entry = HangLogEntry {
            timestamp: Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%z").to_string(),
            hang_nodes,
            node_similarities,
            node_stacks,
            consecutive_high_similarity,
            config: HangLogConfig::from(&self.config),
            detection_state,
            node_observations,
        };

        // 确保目录存在
        let log_dir = Path::new(&self.config.log_dir);
        if let Err(e) = fs::create_dir_all(log_dir) {
            tracing::error!("Failed to create log directory {:?}: {}", log_dir, e);
            return None;
        }

        // 生成文件名时间戳
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();

        // 写入 JSON 日志文件
        let json_filename = format!("hang_{}.json", timestamp);
        let json_filepath = log_dir.join(&json_filename);

        match write_log_file(&json_filepath, &entry) {
            Ok(_) => {
                tracing::info!("HANG log written to {:?}", json_filepath);
            }
            Err(e) => {
                tracing::error!("Failed to write HANG log: {}", e);
                return None;
            }
        }

        // 采集全局火焰图
        let svg_filename = format!("hang_{}.svg", timestamp);
        let svg_filepath = log_dir.join(&svg_filename);

        match collect_global_flamegraph().await {
            Ok(svg_content) => {
                if let Err(e) = write_svg_file(&svg_filepath, &svg_content) {
                    tracing::warn!("Failed to write global flamegraph SVG: {}", e);
                } else {
                    tracing::info!("HANG global flamegraph written to {:?}", svg_filepath);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to collect global flamegraph: {}", e);
            }
        }

        // 标记已记录
        {
            let state = get_hang_state();
            let mut state = state.write().unwrap();
            state.mark_logged();
        }

        Some(json_filepath.to_string_lossy().to_string())
    }
}

/// 写入日志文件
fn write_log_file(filepath: &Path, entry: &HangLogEntry) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let mut file = File::create(filepath)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;

    Ok(())
}

/// 写入 SVG 文件
fn write_svg_file(filepath: &Path, content: &str) -> std::io::Result<()> {
    let mut file = File::create(filepath)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// 采集全局火焰图（所有节点的所有 rank）
async fn collect_global_flamegraph() -> Result<String, String> {
    // 加载配置
    let config = load_collector_config(&get_config_path())
        .map_err(|e| format!("Failed to load collector config: {}", e))?;

    // 获取所有节点信息
    let (ranks, _nodes) = get_real_training_data()
        .await
        .map_err(|e| format!("Failed to get training data: {}", e))?;

    if ranks.is_empty() {
        return Err("No nodes available for flamegraph collection".to_string());
    }

    // 按 rank_id 排序构建 URL，确保 URL index 与全局 rank ID 一致
    // ranks 已在 get_real_training_data() 中按 rank_id 排序
    let all_urls: Vec<String> = ranks
        .iter()
        .map(|r| {
            format!(
                "http://{}:{}/apis/pythonext/callstack",
                r.node_ip,
                config.callstack_base_port + r.local_rank as u16
            )
        })
        .collect();

    if all_urls.is_empty() {
        return Err("No URLs to collect stacks from".to_string());
    }

    tracing::info!(
        "Collecting global flamegraph from {} URLs across {} ranks",
        all_urls.len(),
        ranks.len()
    );

    // 使用现有的全局火焰图采集函数
    let svg = collect_and_generate_flamegraph("global", all_urls, None)
        .await
        .map_err(|e| format!("Failed to generate global flamegraph: {}", e))?;

    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn test_config(log_dir: &str) -> HangConfig {
        HangConfig {
            enabled: true,
            sample_interval_min_secs: 30,
            sample_interval_max_secs: 30,
            sample_count: 3,
            node_count: 4,
            jaccard_threshold: 0.95,
            blocking_patterns: vec!["checkpoint".to_string()],
            log_enabled: true,
            log_dir: log_dir.to_string(),
            ..HangConfig::default()
        }
    }

    #[test]
    fn test_hang_log_config_from() {
        let config = test_config("test_logs");
        let log_config = HangLogConfig::from(&config);

        assert_eq!(log_config.sample_interval_secs, 30);
        assert_eq!(log_config.sample_interval_min_secs, 30);
        assert_eq!(log_config.sample_interval_max_secs, 30);
        assert_eq!(log_config.sample_count, 3);
        assert_eq!(log_config.node_count, 4);
        assert_eq!(log_config.jaccard_threshold, 0.95);
    }

    #[test]
    fn test_logger_disabled() {
        let mut config = test_config("test_logs");
        config.log_enabled = false;

        let logger = HangLogger::new(config);
        assert!(!logger.is_enabled());
    }

    #[test]
    fn test_write_log_file() {
        let dir = tempdir().unwrap();
        let filepath = dir.path().join("test_hang.json");

        let entry = HangLogEntry {
            timestamp: "2024-01-01T12:00:00.000+0800".to_string(),
            hang_nodes: vec!["192.168.1.1".to_string()],
            node_similarities: HashMap::from([("192.168.1.1".to_string(), 0.98)]),
            node_stacks: HashMap::new(),
            consecutive_high_similarity: 3,
            config: HangLogConfig {
                sample_interval_secs: 30,
                sample_interval_min_secs: 30,
                sample_interval_max_secs: 30,
                sample_count: 3,
                node_count: 4,
                jaccard_threshold: 0.95,
            },
            detection_state: HangDetectionStateLog {
                event_id: Some(1700000000),
                hang_first_detected_at: Some(1700000001),
                selected_nodes: vec!["192.168.1.1".to_string()],
                sample_round: 3,
                consecutive_normal_count: 0,
                hang_notified: false,
                hang_intranet_notified: false,
                hang_notify_in_flight: false,
                pending_recovery: None,
            },
            node_observations: vec![HangNodeObservationLog {
                node_ip: "192.168.1.1".to_string(),
                observation: "Hang".to_string(),
                similarity: 0.98,
            }],
        };

        let result = write_log_file(&filepath, &entry);
        assert!(result.is_ok());

        // 验证文件内容
        let content = fs::read_to_string(&filepath).unwrap();
        assert!(content.contains("192.168.1.1"));
        assert!(content.contains("0.98"));
        assert!(content.contains("detection_state"));
        assert!(content.contains("node_observations"));
    }
}
