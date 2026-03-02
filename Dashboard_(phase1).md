### 基于 Rust 构建千卡级训练任务 Web 监控面板
结合「分层+聚合+热力编码+可下钻」的核心原则，规避 rank 列表作为主视图，聚焦千卡规模下的高效问题定位：

先用mock的数据进行一个16机128卡的环境, 完成框架搭建和实现, 后续我再换成真实数据.

---

### 零、核心数据结构定义（Rust）

#### 1. 状态枚举
```rust
/// 健康状态枚举
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,   // 绿色：正常
    Warning,   // 黄色：性能下降但未故障
    Critical,  // 红色：故障或严重异常
}
```

#### 2. Rank 级别数据（单个训练进程）
```rust
/// 单个 Rank 的指标数据
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankMetrics {
    pub rank_id: u32,              // 全局唯一 rank ID (0-127)
    pub local_rank: u8,            // 节点内 GPU 编号 (0-7)
    pub node_ip: String,           // 所属节点 IP
    
    // 核心指标
    pub step_time_ms: f64,         // 当前 step 耗时 (毫秒)
    pub step_time_ratio: f64,      // 相对全局 P50 的倍数 (如 1.5x 表示慢 50%)
    pub gpu_utilization: f32,      // GPU 利用率 (0-100%)
    pub gpu_memory_used_gb: f32,   // GPU 显存占用 (GB)
    pub gpu_memory_total_gb: f32,  // GPU 显存总量 (GB)
    
    // 通信指标
    pub nccl_latency_ms: f64,      // NCCL 通信延迟 (毫秒)
    pub nccl_bandwidth_gbps: f32,  // NCCL 带宽 (Gbps)
    
    // 状态
    pub status: HealthStatus,
    pub last_heartbeat: u64,       // Unix 时间戳 (秒)
    pub current_step: u64,         // 当前训练 step
    pub error_message: Option<String>,
}
```

#### 3. Node 级别数据（节点聚合）
```rust
/// 节点聚合指标
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub node_ip: String,           // 节点 IP
    pub hostname: String,          // 主机名
    pub rack_id: String,           // 机柜 ID
    
    // 聚合指标
    pub rank_count: u8,            // 节点上的 rank 数量 (通常为 8)
    pub healthy_count: u8,         // 健康 rank 数量
    pub warning_count: u8,         // 警告 rank 数量
    pub critical_count: u8,        // 故障 rank 数量
    
    // 性能聚合
    pub slow_ratio: f32,           // 慢 rank 占比 (0.0-1.0)
    pub avg_step_time_ms: f64,     // 平均 step 耗时
    pub p50_step_time_ms: f64,     // P50 step 耗时
    pub p99_step_time_ms: f64,     // P99 step 耗时
    pub avg_gpu_utilization: f32,  // 平均 GPU 利用率
    pub avg_nccl_latency_ms: f64,  // 平均 NCCL 延迟
    
    // 状态
    pub status: HealthStatus,      // 节点整体状态 (取最严重的 rank 状态)
    pub last_update: u64,          // 最后更新时间戳
}
```

#### 4. 全局聚合数据
```rust
/// 全局聚合指标 (Level 1 视图)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalMetrics {
    pub total_nodes: u16,          // 总节点数
    pub total_ranks: u16,          // 总 rank 数
    
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
    pub slow_node_ratio: f32,      // 慢节点占比
    
    // 训练进度
    pub current_step: u64,
    pub steps_per_second: f64,
    pub estimated_remaining_hours: Option<f64>,
    
    // 时间戳
    pub last_update: u64,
}
```

#### 5. 拓扑结构
```rust
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
```

---

### 零.一、REST API 端点设计

#### Level 1 - 全局聚合
```
GET /api/global
Response: GlobalMetrics
```

#### Level 2 - 节点列表
```
GET /api/nodes?sort_by={field}&order={asc|desc}&status={healthy|warning|critical}
Response: {
    nodes: Vec<NodeMetrics>,
    total: u16,
}
```

#### Level 3 - Rank 详情
```
GET /api/nodes/{ip}/ranks
Response: {
    node: NodeMetrics,
    ranks: Vec<RankMetrics>,
}
```

#### 拓扑热图
```
GET /api/topology
Response: Topology
```

---

### 零.二、Mock 数据规格

#### 环境配置
- **节点数**: 16 台服务器
- **每节点 GPU 数**: 8 张
- **总 Rank 数**: 128 (16 × 8)
- **机柜分布**: 4 个机柜，每机柜 4 台服务器

#### 数据生成规则
```rust
// IP 生成: 192.168.1.{1-16}
// Rack 分配: rack-{01-04}，每 4 台一组

// 正常 Rank 指标范围
step_time_ms: 80.0 ~ 120.0 (正态分布, μ=100, σ=10)
gpu_utilization: 85% ~ 98%
nccl_latency_ms: 0.5 ~ 2.0

// 慢 Rank 注入 (约 10% 的 rank)
step_time_ms: 200.0 ~ 500.0
gpu_utilization: 40% ~ 70%

// 故障 Rank 注入 (约 2% 的 rank)
status: Critical
error_message: Some("NCCL timeout" | "OOM" | "Heartbeat lost")
```

#### 慢/异常判定阈值
```rust
// 慢判定: step_time > global_p50 * 1.5
// 警告判定: 节点有 1-2 个慢 rank
// 故障判定: 节点有 ≥1 个 Critical rank 或 心跳超时 > 30s
```

---

### 零.三、可视化依赖建议

#### 热力色阶实现方案

**方案一：纯 CSS 线性渐变（推荐）**
```css
/* 健康状态热力色 */
.heat-level-0 { background: #22c55e; } /* 绿色 - 正常 */
.heat-level-1 { background: #84cc16; } /* 浅绿 */
.heat-level-2 { background: #eab308; } /* 黄色 - 轻微异常 */
.heat-level-3 { background: #f97316; } /* 橙色 */
.heat-level-4 { background: #ef4444; } /* 红色 - 严重 */

/* 慢占比热力色 (0-100%) */
.slow-ratio {
    background: linear-gradient(90deg, 
        #22c55e 0%,    /* 0% 慢 */
        #eab308 30%,   /* 30% 慢 */
        #ef4444 100%   /* 100% 慢 */
    );
}
```

**方案二：charming (ECharts Rust 绑定) - 用于复杂图表**
```toml
# Cargo.toml
[dependencies]
charming = "0.3"  # ECharts 的 Rust 绑定，支持 WASM
```

适用场景：
- 时序折线图（step time 历史趋势）
- 拓扑热力图（机柜视图）
- 分布直方图（step time 分布）

**不推荐 d3-rs**：在 WASM 环境兼容性有限，建议使用纯 CSS + charming 组合。

### 一、技术栈选型（Rust 生态）
#### 1. 前端框架：Leptos（首选）/Yew
+ **核心优势**：
    - Leptos 支持 SSR/CSR 混合渲染，天然适配「聚合数据轻量渲染+下钻细节按需加载」的模式；
    - 内置响应式系统，可高效绑定热力图、聚合指标的动态更新；
    - 与 Rust 后端无缝衔接，无需跨语言序列化/反序列化（直接复用 `Node`/`Topology` 等结构体）。
+ **补充依赖**：
    - 可视化：`d3-rs`（热力图/条形图）+ `thaw`（Rust 组件库，替代 React 生态）；
    - 状态管理：Leptos `create_resource`/`create_signal`（轻量且适配分布式数据更新）；
    - 网络：`reqwest`（后端）+ `leptos_axum`（前后端一体化）。

#### 2. 后端服务：Axum + Tokio
+ 核心能力：
    - 异步处理千卡级节点的聚合查询（批量计算慢占比、异常占比、P50/P99 等统计值）；
    - 按需下钻：仅在用户点击 IP/Node 时，才查询该节点下的 rank 细节；
    - 数据缓存：对 Level 1/2 的聚合指标做 5s 窗口缓存（避免重复计算），Level 3 实时查询。

#### 3. 数据存储/计算适配
+ 千卡规模下，**避免直接查全量 rank 数据**：
    - 预聚合：后端定时（5s）计算「节点级聚合指标」（慢占比、异常占比、P50/P99 step 等），存储在内存/Redis；
    - 按需查询：Level 3 仅查询用户选中节点的 rank 数据，而非全量；
    - 拓扑数据：复用 `TopologyView` 结构体，关联机柜/交换机/IP 层级关系。

### 二、三级视图的 Rust 实现方案
#### （一）Level 1：训练任务全局态势（首页）
核心目标：1 屏判断任务是否正常，聚焦「聚合+分布+聚类」，不展示任何 rank 列表。

##### 1. 视图结构（Rust/Leptos 代码骨架）
##### 2. 核心实现要点
+ **健康分布块**：不是单纯数字，而是「可点击的筛选入口」（点击“慢”直接跳转到 Level 2 并筛选慢节点）；
+ **聚类条形图**：用热力色阶编码（颜色越深=异常数越多），点击条形直接跳转到对应 IP 的 Level 2 视图；
+ **指标精简**：严格控制在 8 个以内，避免信息过载。

#### （二）Level 2：IP/Node 聚合视图（问题定位主战场）
核心目标：按节点/IP 聚合，用「颜色/热力」替代数字，支持排序/筛选/下钻。

##### 1. 视图结构（Rust/Leptos 代码骨架）
##### 2. 核心实现要点
+ **热力编码**：
    - 慢占比：用线性热力色阶（浅黄→深红），人眼优先识别颜色而非数字；
    - P99/RTT：用对数刻度热力（适配千卡下的数值跨度）；
    - 异常占比：离散红色块（只要>0就醒目，无需精确数值）；
+ **交互设计**：
    - 点击行跳转到该 IP 的 Level 3 视图（rank 细节）；
    - 支持按「慢占比/P99」排序，快速定位最严重节点；
    - 拓扑热图：按机柜分组，节点用色标（红/黄/绿）展示状态，体现空间聚集性；
+ **数据适配**：后端返回「节点级聚合值」，而非 rank 列表（千卡下仅返回数百行节点数据，而非数千行 rank 数据）。

#### （三）Level 3：Rank/进程级细节视图（下钻诊断）
核心目标：仅针对锁定的节点，展示 rank 细节+时序指标，聚焦「诊断」而非「监控」。

##### 1. 视图结构（Rust/Leptos 代码骨架）
##### 2. 核心实现要点
+ **慢的相对性**：不展示绝对耗时，而是展示「该rank耗时 / 全局P50耗时」的倍数（如 4.2x），更直观体现“慢”；
+ **按需加载**：仅加载用户选中 IP 的 rank 数据（通常 8/16 个 rank per node），而非全量；
+ **时序聚焦**：仅展示核心诊断指标（step time 抖动、NCCL 延迟、GPU 利用率），避免信息冗余。

### 三、关键优化建议（适配千卡规模）
1. **数据层优化**：
    - 预聚合：后端定时（5s）计算 Level 1/2 的聚合指标，存储在内存/Redis，前端仅拉取聚合结果（避免每次查询全量 rank 数据）；
    - 分页/筛选：Level 2 支持分页（即使 512 节点，也无需一次渲染所有行）；
    - 缓存策略：Level 1 指标缓存 5s，Level 2 节点指标缓存 3s，Level 3 rank 数据不缓存（实时诊断）。
2. **前端渲染优化**：
    - 虚拟列表：Level 2 节点数超过 100 时，用虚拟列表（`leptos-virtual-list`）避免 DOM 过多；
    - 懒加载：Level 3 的时序图仅在用户点击 rank 后加载，默认不渲染；
    - 热力图复用：提前预定义热力色阶（如 0-10 级），避免动态计算。
3. **交互逻辑优化**：
    - 无刷新跳转：用 Leptos 路由的客户端跳转（`navigate_to`），避免页面刷新；
    - 筛选记忆：保留用户的筛选条件（如“仅看异常节点”），返回时无需重新选择；
    - 下钻轻量化：Level 3 仅展示核心诊断指标，不做多余聚合。
4. **慢/异常定义优化**：
    - 后端计算“慢”的阈值：基于全局 P50 + 3σ，或 P99/P50 倍数，前端仅展示结果；
    - 异常聚类：后端按 IP/机柜聚合异常 rank，前端仅展示聚类结果（避免展示数千个异常 rank）。

### 四、总结
基于 Rust 构建千卡级监控面板的核心是「**后端预聚合+前端热力编码+按需下钻**」：

+ Level 1：全局聚合，聚焦“是否正常”；
+ Level 2：节点聚合，聚焦“问题在哪”；
+ Level 3：rank 细节，聚焦“问题根因”；  
全程规避 rank 列表作为主视图，用「分布/聚类/热力」替代“数字列表”，适配千卡规模下的人效最大化（30 秒内定位问题节点）。

技术上依托 Leptos+Axum 的 Rust 生态，既能保证高性能（异步处理聚合计算），又能实现前后端一体化（复用数据结构体，减少序列化开销），完美适配千卡级训练任务的监控需求。

