---
name: super-training-monitor
description: |
  从超级训练监控面板获取节点信息、堆栈信息、step指标等数据的技能。当用户需要监控分布式训练集群的状态、性能指标、堆栈火焰图或step数据时使用此技能。这个技能专门用于与基于Leptos的超级训练监控面板交互，通过其提供的server functions API获取数据。

  确保使用此技能当用户提到以下任何内容时：训练监控、节点信息、GPU利用率、NCCL延迟、堆栈火焰图、step指标、训练进度、健康状态、慢节点、rank详情、拓扑视图、调用栈分析。
---

# 超级训练监控技能

这个技能让你能够与超级训练监控面板交互，获取分布式训练集群的各种监控数据。该监控面板基于Leptos + Axum构建，提供了丰富的server functions API来获取节点信息、性能指标、堆栈数据和step指标。

## 快速开始

1. **获取服务地址**：用户需要提供监控面板的IP地址和端口（通常是 `http://127.0.0.1:3000`）
2. **构建API URL**：所有API端点都在 `/api/{function_name}` 路径下
3. **发送POST请求**：所有API都是HTTP POST请求，需要 `Content-Type: application/json` 头部

## API端点参考

### 核心监控API

#### 1. 全局聚合指标 (Level 1)
```rust
// API端点: /api/get_global_metrics
// 方法: POST
// 请求体: {}
// 返回: GlobalMetrics 结构体
```

获取整个训练集群的全局聚合指标，包括：
- 总节点数和总rank数
- 健康状态分布（健康/警告/故障的节点和rank数）
- 全局性能指标（P50/P99 step time、平均GPU利用率、慢节点占比）
- 训练进度（当前step、训练速度、预计剩余时间）

#### 2. 节点列表 (Level 2)
```rust
// API端点: /api/get_nodes
// 方法: POST
// 请求体: {"sort_field": "SlowRatio", "sort_order": "Desc", "status_filter": "All"}
// 返回: NodesResponse 结构体
```

获取所有节点的详细列表，支持排序和筛选：
- `sort_field`: `SlowRatio`, `StepTime`, `GpuUtilization`, `NcclLatency`
- `sort_order`: `Asc`, `Desc`
- `status_filter`: `All`, `Healthy`, `Warning`, `Critical`

#### 3. 节点Rank详情 (Level 3)
```rust
// API端点: /api/get_node_ranks
// 方法: POST
// 请求体: {"ip": "192.168.1.100"}
// 返回: NodeRanksResponse 结构体
```

获取指定节点的所有rank的详细指标，包括每个GPU的性能数据。

#### 4. 拓扑视图
```rust
// API端点: /api/get_topology
// 方法: POST
// 请求体: {}
// 返回: Topology 结构体
```

获取机柜-节点的拓扑结构，用于可视化展示。

### 堆栈分析API

#### 5. 所有节点Callstack信息
```rust
// API端点: /api/get_all_nodes_callstack_info
// 方法: POST
// 请求体: {}
// 返回: Vec<(String, u8, u16)> （IP, rank数量, 基础端口）
```

获取所有节点及其rank的callstack URL信息，用于堆栈采集。

#### 6. 节点火焰图
```rust
// API端点: /api/get_node_flamegraph
// 方法: POST
// 请求体: {"ip": "192.168.1.100"}
// 返回: String (SVG格式的火焰图)
```

获取指定节点的合并火焰图SVG，可视化所有rank的调用栈。

#### 7. 所有节点火焰图
```rust
// API端点: /api/get_all_nodes_flamegraph
// 方法: POST
// 请求体: {}
// 返回: String (SVG格式的火焰图)
```

获取所有节点所有rank的合并火焰图SVG，全局视角的调用栈分析。

#### 8. 节点堆栈信息
```rust
// API端点: /api/get_node_stacks
// 方法: POST
// 请求体: {"ip": "192.168.1.100"}
// 返回: NodeStacksResponse 结构体
```

获取指定节点的详细堆栈信息，包括合并后的调用栈树。

### Step指标API (Phase 2)

#### 9. Step显示功能状态
```rust
// API端点: /api/get_step_show_enabled
// 方法: POST
// 请求体: {}
// 返回: bool (是否启用Step显示)
```

检查Step显示功能是否启用（需要环境变量 `STEP_SHOW=true`）。

#### 10. 全局Step指标
```rust
// API端点: /api/get_global_step_metrics
// 方法: POST
// 请求体: {}
// 返回: GlobalStepMetrics 结构体
```

获取全局的Step指标，包括当前step、最近duration、显存分配等。

#### 11. Rank Step指标
```rust
// API端点: /api/get_rank_step_metrics
// 方法: POST
// 请求体: {"ip": "192.168.1.100", "local_rank": 0, "rank_id": 0}
// 返回: RankStepMetrics 结构体
```

获取指定rank的详细Step指标。

## 数据结构参考

### 主要数据结构

1. **GlobalMetrics** - 全局聚合指标
2. **NodeMetrics** - 节点聚合指标
3. **RankMetrics** - 单个rank的指标
4. **HealthStatus** - 健康状态枚举（Healthy, Warning, Critical）
5. **StepRecord** - Step记录（step, module, stage, duration, allocated）
6. **GlobalStepMetrics** - 全局Step指标
7. **RankStepMetrics** - Rank Step指标
8. **MergedStackFrame** - 合并的堆栈帧（用于火焰图）

详细字段定义见 `references/data_structures.md`。

## 使用流程

当用户请求获取训练监控数据时：

1. **确认服务地址**：询问用户监控面板的IP和端口，或使用默认值 `http://127.0.0.1:3000`
2. **确定所需数据**：根据用户需求选择合适的API
3. **构建请求**：创建正确的JSON请求体
4. **发送请求**：使用WebFetch工具发送POST请求到 `/api/{function_name}`
5. **处理响应**：解析JSON响应，提取关键信息
6. **展示结果**：以清晰的格式展示数据（表格、摘要、图表等）

## 示例用例

### 用例1：获取全局状态
用户说："我想看看训练集群的总体状态"

步骤：
1. 调用 `/api/get_global_metrics`
2. 展示关键指标：
   - 健康状态分布（节点和rank）
   - 全局性能指标（P50/P99 step time, GPU利用率）
   - 训练进度（当前step, 训练速度）
3. 提供建议：如果慢节点占比高或有关键故障，建议深入排查

### 用例2：分析慢节点
用户说："有哪些节点比较慢？"

步骤：
1. 调用 `/api/get_nodes`，按 `SlowRatio` 降序排序
2. 展示慢节点列表，包含关键指标：
   - 节点IP
   - 慢rank占比
   - 平均step time
   - GPU利用率
   - NCCL延迟
3. 对于最慢的节点，提供深入分析的选项

### 用例3：查看节点详情
用户说："我想看看节点 192.168.1.100 的详细信息"

步骤：
1. 调用 `/api/get_node_ranks` 获取该节点的所有rank详情
2. 展示节点汇总信息
3. 展示每个rank的详细指标表格
4. 提供查看火焰图的选项

### 用例4：生成火焰图
用户说："给我生成节点 192.168.1.100 的火焰图"

步骤：
1. 调用 `/api/get_node_flamegraph` 获取SVG火焰图
2. 将SVG保存为文件并提供给用户
3. 解释火焰图的关键信息（热点函数、调用深度等）

### 用例5：查看Step指标
用户说："我想看看最近的step数据"

步骤：
1. 检查Step功能是否启用：`/api/get_step_show_enabled`
2. 如果启用，调用 `/api/get_global_step_metrics`
3. 展示Step指标：
   - 当前step
   - 最近duration
   - 显存分配
   - 历史记录表格

## 错误处理

- **连接失败**：检查服务是否运行，IP端口是否正确
- **API错误**：检查请求体格式是否正确
- **Mock模式**：如果环境变量 `COLLECTOR_MOCK_MODE=true`，API会返回模拟数据
- **Step功能未启用**：如果需要Step数据但未设置 `STEP_SHOW=true`，会返回错误

## 高级功能

### 批量操作
可以同时获取多个节点的数据进行分析比较。

### 趋势分析
通过定期获取数据，可以分析性能趋势和变化。

### 自动诊断
基于指标数据自动诊断常见问题（GPU利用率低、NCCL延迟高、内存泄漏等）。

---

**注意**：使用此技能前，确保超级训练监控面板服务正在运行，并且你知道正确的IP地址和端口号。