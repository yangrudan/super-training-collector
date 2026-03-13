# 数据结构参考

本文档详细描述了超级训练监控面板API返回的所有数据结构。

## 健康状态枚举

```rust
enum HealthStatus {
    Healthy,   // 绿色：正常
    Warning,   // 黄色：性能下降但未故障
    Critical,  // 红色：故障或严重异常
}
```

## 核心数据结构

### GlobalMetrics - 全局聚合指标

```rust
struct GlobalMetrics {
    // 规模统计
    pub total_nodes: u16,         // 总节点数
    pub total_ranks: u16,         // 总rank数

    // 健康分布
    pub healthy_nodes: u16,       // 健康节点数
    pub warning_nodes: u16,       // 警告节点数
    pub critical_nodes: u16,      // 故障节点数
    pub healthy_ranks: u16,       // 健康rank数
    pub warning_ranks: u16,       // 警告rank数
    pub critical_ranks: u16,      // 故障rank数

    // 全局性能指标
    pub global_p50_step_time_ms: f64,     // 全局P50 step耗时（毫秒）
    pub global_p99_step_time_ms: f64,     // 全局P99 step耗时（毫秒）
    pub global_avg_gpu_utilization: f32,  // 全局平均GPU利用率（%）
    pub slow_node_ratio: f32,             // 慢节点占比（0.0-1.0）

    // 训练进度
    pub current_step: u64,                // 当前训练step
    pub steps_per_second: f64,            // 训练速度（steps/秒）
    pub estimated_remaining_hours: Option<f64>, // 预计剩余时间（小时）

    // 时间戳
    pub last_update: u64,                 // 最后更新时间戳（秒）
}
```

### NodeMetrics - 节点聚合指标

```rust
struct NodeMetrics {
    // 基本信息
    pub node_ip: String,          // 节点IP地址
    pub hostname: String,         // 主机名
    pub rack_id: String,          // 机柜ID

    // 规模统计
    pub rank_count: u8,           // 节点上的rank数量（通常为8）
    pub healthy_count: u8,        // 健康rank数量
    pub warning_count: u8,        // 警告rank数量
    pub critical_count: u8,       // 故障rank数量

    // 性能聚合
    pub slow_ratio: f32,          // 慢rank占比（0.0-1.0）
    pub avg_step_time_ms: f64,    // 平均step耗时（毫秒）
    pub p50_step_time_ms: f64,    // P50 step耗时（毫秒）
    pub p99_step_time_ms: f64,    // P99 step耗时（毫秒）
    pub avg_gpu_utilization: f32, // 平均GPU利用率（%）
    pub avg_nccl_latency_ms: f64, // 平均NCCL延迟（毫秒）

    // 状态
    pub status: HealthStatus,     // 节点整体状态
    pub last_update: u64,         // 最后更新时间戳（秒）
}
```

### RankMetrics - 单个Rank的指标

```rust
struct RankMetrics {
    // 身份信息
    pub rank_id: u32,             // 全局唯一rank ID (0-127)
    pub local_rank: u8,           // 节点内GPU编号 (0-7)
    pub node_ip: String,          // 所属节点IP
    pub hostname: String,         // 主机名

    // 核心指标
    pub step_time_ms: f64,        // 当前step耗时（毫秒）
    pub step_time_ratio: f64,     // 相对全局P50的倍数
    pub gpu_utilization: f32,     // GPU利用率 (0-100%)
    pub gpu_memory_used_gb: f32,  // GPU显存占用 (GB)
    pub gpu_memory_total_gb: f32, // GPU显存总量 (GB)

    // 通信指标
    pub nccl_latency_ms: f64,     // NCCL通信延迟（毫秒）
    pub nccl_bandwidth_gbps: f32, // NCCL带宽 (Gbps)

    // 状态
    pub status: HealthStatus,
    pub last_heartbeat: u64,      // Unix时间戳（秒）
    pub current_step: u64,        // 当前训练step
    pub error_message: Option<String>, // 错误信息（如果有）
}
```

### Topology - 拓扑视图数据

```rust
struct Topology {
    pub racks: Vec<RackInfo>,     // 机柜列表
}

struct RackInfo {
    pub rack_id: String,          // 机柜ID
    pub nodes: Vec<NodeSummary>,  // 节点摘要列表
}

struct NodeSummary {
    pub node_ip: String,          // 节点IP
    pub status: HealthStatus,     // 节点状态
    pub slow_ratio: f32,          // 慢rank占比
}
```

## 响应包装结构

### NodesResponse - 节点列表响应

```rust
struct NodesResponse {
    pub nodes: Vec<NodeMetrics>,  // 节点指标列表
    pub total: u16,               // 总节点数（分页用）
}
```

### NodeRanksResponse - 节点Rank详情响应

```rust
struct NodeRanksResponse {
    pub node: NodeMetrics,        // 节点聚合指标
    pub ranks: Vec<RankMetrics>,  // 该节点的所有rank指标
}
```

## 堆栈分析数据结构

### RankStack - 单个Rank的堆栈信息

```rust
struct RankStack {
    pub rank_id: u32,             // rank ID
    pub node_ip: String,          // 节点IP
    pub callstack: Vec<String>,   // 调用栈帧列表（从栈底到栈顶）
    pub timestamp: u64,           // 采集时间戳
}
```

### MergedStackFrame - 合并后的堆栈帧节点

```rust
struct MergedStackFrame {
    pub frame_name: String,       // 函数名/栈帧名
    pub depth: u32,               // 调用深度
    pub rank_ids: Vec<u32>,       // 包含此帧的rank列表
    pub rank_count: u32,          // 包含此帧的rank数量
    pub total_ranks: u32,         // 总rank数（用于计算覆盖率）
    pub children: Vec<MergedStackFrame>, // 子栈帧
}
```

### NodeStacksResponse - 节点堆栈响应

```rust
struct NodeStacksResponse {
    pub node_ip: String,          // 节点IP
    pub stacks: Vec<RankStack>,   // 所有rank的堆栈
    pub merged_root: MergedStackFrame, // 合并后的根节点
    pub collected_at: u64,        // 采集时间戳
}
```

## Step指标数据结构

### StepRecord - 单条Step记录

```rust
struct StepRecord {
    pub step: u64,                // Step编号
    pub module: Option<String>,   // 模块名
    pub stage: Option<String>,    // 阶段名
    pub duration: Option<f64>,    // 耗时（微秒或毫秒，取决于API）
    pub allocated: Option<u64>,   // 显存分配（字节）
}
```

### GlobalStepMetrics - 全局Step指标

```rust
struct GlobalStepMetrics {
    pub current_step: u64,                // 当前step
    pub latest_duration_ms: Option<f64>,  // 最近duration（毫秒）
    pub latest_allocated_gb: Option<f64>, // 最近显存分配（GB）
    pub records: Vec<StepRecord>,         // Step记录列表
}
```

### RankStepMetrics - Rank Step指标

```rust
struct RankStepMetrics {
    pub rank_id: u32,                     // rank ID
    pub node_ip: String,                  // 节点IP
    pub current_step: u64,                // 当前step
    pub latest_duration_ms: Option<f64>,  // 最近duration（毫秒）
    pub latest_allocated_gb: Option<f64>, // 最近显存分配（GB）
    pub records: Vec<StepRecord>,         // Step记录列表
}
```

## 枚举类型

### SortField - 排序字段

```rust
enum SortField {
    SlowRatio,      // 慢rank占比
    StepTime,       // Step耗时
    GpuUtilization, // GPU利用率
    NcclLatency,    // NCCL延迟
}
```

### SortOrder - 排序方向

```rust
enum SortOrder {
    Asc,    // 升序
    Desc,   // 降序（默认）
}
```

### StatusFilter - 状态筛选

```rust
enum StatusFilter {
    All,        // 所有状态
    Healthy,    // 仅健康
    Warning,    // 仅警告
    Critical,   // 仅故障
}
```

## 辅助方法

### HealthStatus 辅助方法

```rust
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
```

### MergedStackFrame 辅助方法

```rust
impl MergedStackFrame {
    pub fn coverage(&self) -> f32 {
        if self.total_ranks == 0 {
            0.0
        } else {
            self.rank_count as f32 / self.total_ranks as f32
        }
    }

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

    pub fn rank_range_str(&self) -> String {
        // 将rank IDs转换为连续范围字符串，如 "0-4, 6, 8-10"
        // 实现细节省略...
    }
}
```