use serde::{Deserialize, Serialize};

/// 健康状态枚举
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HealthStatus {
    #[default]
    Healthy,  // 绿色：正常
    Warning,  // 黄色：性能下降但未故障
    Critical, // 红色：故障或严重异常
}

impl HealthStatus {
    pub fn css_class(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "status-healthy",
            HealthStatus::Warning => "status-warning",
            HealthStatus::Critical => "status-critical",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "正常",
            HealthStatus::Warning => "警告",
            HealthStatus::Critical => "故障",
        }
    }
}

/// 单个 Rank 的指标数据
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankMetrics {
    pub rank_id: u32,             // 全局唯一 rank ID (0-127)
    pub local_rank: u8,           // 节点内 GPU 编号 (0-7)
    pub node_ip: String,          // 所属节点 IP
    pub hostname: String,         // 主机名 (来自 NodeInfo.host)

    // 核心指标
    pub step_time_ms: f64,        // 当前 step 耗时 (毫秒)
    pub step_time_ratio: f64,     // 相对全局 P50 的倍数
    pub gpu_utilization: f32,     // GPU 利用率 (0-100%)
    pub gpu_memory_used_gb: f32,  // GPU 显存占用 (GB)
    pub gpu_memory_total_gb: f32, // GPU 显存总量 (GB)

    // 通信指标
    pub nccl_latency_ms: f64,     // NCCL 通信延迟 (毫秒)
    pub nccl_bandwidth_gbps: f32, // NCCL 带宽 (Gbps)

    // 状态
    pub status: HealthStatus,
    pub last_heartbeat: u64,      // Unix 时间戳 (秒)
    pub current_step: u64,        // 当前训练 step
    pub error_message: Option<String>,
}

/// 节点聚合指标
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub node_ip: String,          // 节点 IP
    pub hostname: String,         // 主机名
    pub rack_id: String,          // 机柜 ID

    // 聚合指标
    pub rank_count: u8,           // 节点上的 rank 数量 (通常为 8)
    pub healthy_count: u8,        // 健康 rank 数量
    pub warning_count: u8,        // 警告 rank 数量
    pub critical_count: u8,       // 故障 rank 数量

    // 性能聚合
    pub slow_ratio: f32,          // 慢 rank 占比 (0.0-1.0)
    pub avg_step_time_ms: f64,    // 平均 step 耗时
    pub p50_step_time_ms: f64,    // P50 step 耗时
    pub p99_step_time_ms: f64,    // P99 step 耗时
    pub avg_gpu_utilization: f32, // 平均 GPU 利用率
    pub avg_nccl_latency_ms: f64, // 平均 NCCL 延迟

    // 状态
    pub status: HealthStatus,     // 节点整体状态
    pub last_update: u64,         // 最后更新时间戳
}

/// 全局聚合指标 (Level 1 视图)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalMetrics {
    pub total_nodes: u16,         // 总节点数
    pub total_ranks: u16,         // 总 rank 数

    // 健康分布
    pub healthy_nodes: u16,
    pub warning_nodes: u16,
    pub critical_nodes: u16,
    pub healthy_ranks: u16,
    pub warning_ranks: u16,
    pub critical_ranks: u16,

    // 全局性能指标
    pub global_p50_step_time_ms: f64,
    pub global_p99_step_time_ms: f64,
    pub global_avg_gpu_utilization: f32,
    pub slow_node_ratio: f32,     // 慢节点占比

    // 训练进度
    pub current_step: u64,
    pub steps_per_second: f64,
    pub estimated_remaining_hours: Option<f64>,

    // 时间戳
    pub last_update: u64,
}

/// 拓扑视图数据
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    pub racks: Vec<RackInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RackInfo {
    pub rack_id: String,
    pub nodes: Vec<NodeSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeSummary {
    pub node_ip: String,
    pub status: HealthStatus,
    pub slow_ratio: f32,
}

/// 节点列表响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodesResponse {
    pub nodes: Vec<NodeMetrics>,
    pub total: u16,
}

/// 节点 Rank 详情响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeRanksResponse {
    pub node: NodeMetrics,
    pub ranks: Vec<RankMetrics>,
}

/// 排序字段
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub enum SortField {
    #[default]
    SlowRatio,
    StepTime,
    GpuUtilization,
    NcclLatency,
}

/// 排序方向
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

/// 状态筛选
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum StatusFilter {
    All,
    Healthy,
    Warning,
    Critical,
}

impl Default for StatusFilter {
    fn default() -> Self {
        StatusFilter::All
    }
}

// ============ 堆栈分析相关数据结构 ============

/// 单个 Rank 的堆栈信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankStack {
    pub rank_id: u32,
    pub node_ip: String,
    pub callstack: Vec<String>,      // 调用栈帧列表 (从栈底到栈顶)
    pub timestamp: u64,
}

/// 合并后的堆栈帧节点 (用于火焰图展示)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergedStackFrame {
    pub frame_name: String,
    pub depth: u32,                   // 调用深度
    pub rank_ids: Vec<u32>,           // 包含此帧的 rank 列表
    pub rank_count: u32,
    pub total_ranks: u32,             // 总 rank 数，用于计算覆盖率
    pub children: Vec<MergedStackFrame>,
}

impl MergedStackFrame {
    /// 计算覆盖率 (0.0 - 1.0)
    pub fn coverage(&self) -> f32 {
        if self.total_ranks == 0 {
            0.0
        } else {
            self.rank_count as f32 / self.total_ranks as f32
        }
    }

    /// 获取覆盖率 CSS 类
    pub fn coverage_class(&self) -> &'static str {
        let coverage = self.coverage();
        if coverage >= 0.9 {
            "coverage-full"
        } else if coverage >= 0.5 {
            "coverage-partial"
        } else {
            "coverage-rare"
        }
    }

    /// 格式化 rank 分布字符串
    pub fn rank_range_str(&self) -> String {
        if self.rank_ids.is_empty() {
            return String::new();
        }

        let mut result = Vec::new();
        let mut sorted_ranks = self.rank_ids.clone();
        sorted_ranks.sort();

        let mut start = sorted_ranks[0];
        let mut end = start;

        for &rank in &sorted_ranks[1..] {
            if rank == end + 1 {
                end = rank;
            } else {
                if start == end {
                    result.push(format!("{}", start));
                } else {
                    result.push(format!("{}-{}", start, end));
                }
                start = rank;
                end = rank;
            }
        }
        
        // 处理最后一个范围
        if start == end {
            result.push(format!("{}", start));
        } else {
            result.push(format!("{}-{}", start, end));
        }

        result.join(", ")
    }
}

/// 节点堆栈响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeStacksResponse {
    pub node_ip: String,
    pub stacks: Vec<RankStack>,
    pub merged_root: MergedStackFrame,
    pub collected_at: u64,
}

// ============ Step 指标相关数据结构 (Phase 2) ============

/// Step 查询请求版本
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepQueryVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Default for StepQueryVersion {
    fn default() -> Self {
        Self { major: 0, minor: 1, patch: 0 }
    }
}

/// Step 查询选项
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepQueryOpts {
    pub limit: u32,
}

/// Step 查询 Payload
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepQueryPayload {
    pub expr: String,
    pub opts: StepQueryOpts,
}

/// Step 查询请求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepQueryRequest {
    pub version: StepQueryVersion,
    pub timestamp: u64,
    pub payload: StepQueryPayload,
}

impl StepQueryRequest {
    /// 创建默认的 Step 查询请求
    pub fn new(limit: u32) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        
        Self {
            version: StepQueryVersion::default(),
            timestamp,
            payload: StepQueryPayload {
                expr: format!(
                    "SELECT step, module, stage, duration, allocated FROM python.torch_trace WHERE step >= 0 ORDER BY step DESC LIMIT {}",
                    limit
                ),
                opts: StepQueryOpts { limit },
            },
        }
    }
}

/// 单条 Step 记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepRecord {
    pub step: u64,
    pub module: Option<String>,
    pub stage: Option<String>,
    pub duration: Option<f64>,      // 耗时（微秒或毫秒，取决于API）
    pub allocated: Option<u64>,     // 显存分配（字节）
}

/// Step 查询响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepQueryResponse {
    pub records: Vec<StepRecord>,
}

/// 全局 Step 指标（用于首页显示）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalStepMetrics {
    pub current_step: u64,
    pub latest_duration_ms: Option<f64>,
    pub latest_allocated_gb: Option<f64>,
    pub records: Vec<StepRecord>,
}

/// Rank Step 指标（用于三级页面显示）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankStepMetrics {
    pub rank_id: u32,
    pub node_ip: String,
    pub current_step: u64,
    pub latest_duration_ms: Option<f64>,
    pub latest_allocated_gb: Option<f64>,
    pub records: Vec<StepRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_css_class() {
        assert_eq!(HealthStatus::Healthy.css_class(), "status-healthy");
        assert_eq!(HealthStatus::Warning.css_class(), "status-warning");
        assert_eq!(HealthStatus::Critical.css_class(), "status-critical");
    }

    #[test]
    fn test_health_status_label() {
        assert_eq!(HealthStatus::Healthy.label(), "正常");
        assert_eq!(HealthStatus::Warning.label(), "警告");
        assert_eq!(HealthStatus::Critical.label(), "故障");
    }

    #[test]
    fn test_merged_stack_frame_coverage() {
        let frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![0, 1, 2, 3],
            rank_count: 4,
            total_ranks: 10,
            children: vec![],
        };
        
        assert_eq!(frame.coverage(), 0.4);
    }

    #[test]
    fn test_merged_stack_frame_coverage_zero_total() {
        let frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![0, 1],
            rank_count: 2,
            total_ranks: 0,
            children: vec![],
        };
        
        assert_eq!(frame.coverage(), 0.0);
    }

    #[test]
    fn test_merged_stack_frame_coverage_class() {
        let mut frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![],
            rank_count: 9,
            total_ranks: 10,
            children: vec![],
        };
        assert_eq!(frame.coverage_class(), "coverage-full");
        
        frame.rank_count = 6;
        assert_eq!(frame.coverage_class(), "coverage-partial");
        
        frame.rank_count = 2;
        assert_eq!(frame.coverage_class(), "coverage-rare");
    }

    #[test]
    fn test_merged_stack_frame_rank_range_str_continuous() {
        let frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![0, 1, 2, 3, 4],
            rank_count: 5,
            total_ranks: 10,
            children: vec![],
        };
        
        let result = frame.rank_range_str();
        assert_eq!(result, "0-4");
    }

    #[test]
    fn test_merged_stack_frame_rank_range_str_gaps() {
        let frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![0, 1, 2, 5, 6, 10],
            rank_count: 6,
            total_ranks: 12,
            children: vec![],
        };
        
        let result = frame.rank_range_str();
        assert!(result.contains("0-2"));
        assert!(result.contains("5-6"));
        assert!(result.contains("10"));
    }

    #[test]
    fn test_merged_stack_frame_rank_range_str_single() {
        let frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![5],
            rank_count: 1,
            total_ranks: 10,
            children: vec![],
        };
        
        let result = frame.rank_range_str();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_merged_stack_frame_rank_range_str_empty() {
        let frame = MergedStackFrame {
            frame_name: "test".to_string(),
            depth: 0,
            rank_ids: vec![],
            rank_count: 0,
            total_ranks: 10,
            children: vec![],
        };
        
        let result = frame.rank_range_str();
        assert_eq!(result, "");
    }
}
