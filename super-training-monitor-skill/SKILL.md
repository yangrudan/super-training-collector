---
name: super-training-monitor
description: |
  从超级训练监控面板获取节点信息、堆栈信息、step指标等数据的技能。当用户需要监控分布式训练集群的状态、性能指标、堆栈火焰图或step数据时使用此技能。这个技能专门用于与基于Leptos的超级训练监控面板交互，通过其提供的server functions API获取数据。

  确保使用此技能当用户提到以下任何内容时：训练监控、节点信息、GPU利用率、NCCL延迟、堆栈火焰图、step指标、训练进度、健康状态、慢节点、rank详情、拓扑视图、调用栈分析、分布式训练、性能分析、火焰图、GPU显存、训练速度。

  当用户询问训练集群状态、性能问题诊断、节点健康状况、堆栈分析或step数据时，立即使用此技能。即使用户没有明确提到"监控面板"，只要涉及分布式训练监控，就应使用此技能。
---

# 超级训练监控技能

这个技能让你能够与超级训练监控面板交互，获取分布式训练集群的各种监控数据。该监控面板基于Leptos + Axum构建，提供了丰富的server functions API来获取节点信息、性能指标、堆栈数据和step指标。

## 快速开始

### 第一步：确认服务地址
1. 询问用户监控面板的IP地址和端口，例如："监控面板运行在哪个地址？通常是 http://127.0.0.1:3000"
2. 如果用户没有提供，使用默认地址 `http://127.0.0.1:3000`
3. 构建基础URL：`{用户提供的地址}/api/{function_name}`

### 第二步：选择合适的API
根据用户需求选择对应的API端点：

| 用户需求 | 推荐API | 请求体示例 |
|---------|---------|-----------|
| 总体状态概览 | `get_global_metrics` | `{}` |
| 节点列表（可排序筛选） | `get_nodes` | `{"sort_field": "SlowRatio", "sort_order": "Desc", "status_filter": "All"}` |
| 特定节点详情 | `get_node_ranks` | `{"ip": "192.168.1.100"}` |
| 拓扑结构 | `get_topology` | `{}` |
| 节点火焰图 | `get_node_flamegraph` | `{"ip": "192.168.1.100"}` |
| 全局火焰图 | `get_all_nodes_flamegraph` | `{}` |
| Step指标（如果启用） | `get_global_step_metrics` | `{}` |
| 特定Rank的Step指标 | `get_rank_step_metrics` | `{"ip": "192.168.1.100", "local_rank": 0, "rank_id": 0}` |

### 第三步：发送HTTP请求
使用WebFetch工具发送POST请求：

```javascript
// 示例：获取全局指标
const response = await fetch('http://127.0.0.1:3000/api/get_global_metrics', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
  },
  body: JSON.stringify({}),
});
const data = await response.json();
```

### 第四步：解析和展示结果
根据API响应类型，提取关键信息并以清晰格式展示：

1. **表格展示**：对于列表数据（节点、rank、step记录）
2. **摘要卡片**：对于关键指标（KPI）
3. **状态指示器**：使用颜色编码表示健康状态
4. **建议分析**：基于数据提供诊断建议

## API端点详细说明

### 核心监控API

#### 1. 全局聚合指标 (Level 1)
**端点**: `/api/get_global_metrics`
**方法**: POST
**请求体**: `{}`
**返回**: [GlobalMetrics](../references/data_structures.md#globalmetrics---全局聚合指标)

获取整个训练集群的全局聚合指标，包括健康状态分布、性能指标和训练进度。

**使用场景**：
- 用户想知道训练集群总体状态
- 需要快速了解是否有问题
- 查看训练进度和性能基线

**关键指标展示**：
```markdown
## 全局状态摘要
- **节点健康**: 45/50 健康, 3/50 警告, 2/50 故障
- **Rank健康**: 350/400 健康, 30/400 警告, 20/400 故障
- **性能指标**: P50 Step Time: 125ms, P99: 240ms
- **GPU利用率**: 平均 78%
- **训练进度**: Step 12500, 速度: 42 steps/s, 剩余: 12.5小时
```

#### 2. 节点列表 (Level 2)
**端点**: `/api/get_nodes`
**方法**: POST
**请求体**:
```json
{
  "sort_field": "SlowRatio",  // SlowRatio, StepTime, GpuUtilization, NcclLatency
  "sort_order": "Desc",       // Asc, Desc
  "status_filter": "All"      // All, Healthy, Warning, Critical
}
```
**返回**: [NodesResponse](../references/data_structures.md#nodesresponse---节点列表响应)

获取所有节点的详细列表，支持排序和筛选。

**使用场景**：
- 查找性能最差的节点
- 按特定指标排序分析
- 筛选特定状态的节点

**表格展示建议**：
```
| IP地址 | 主机名 | 状态 | 慢Rank占比 | 平均Step Time | GPU利用率 | NCCL延迟 |
|--------|--------|------|------------|---------------|-----------|----------|
| 192.168.1.100 | node-01 | 🟢 健康 | 12% | 130ms | 85% | 2.1ms |
| 192.168.1.101 | node-02 | 🟡 警告 | 38% | 210ms | 62% | 5.4ms |
```

#### 3. 节点Rank详情 (Level 3)
**端点**: `/api/get_node_ranks`
**方法**: POST
**请求体**: `{"ip": "192.168.1.100"}`
**返回**: [NodeRanksResponse](../references/data_structures.md#noderanksresponse---节点rank详情响应)

获取指定节点的所有rank的详细指标。

**使用场景**：
- 深入分析特定节点的性能问题
- 查看节点内各个GPU的差异
- 诊断rank级别的故障

#### 4. 拓扑视图
**端点**: `/api/get_topology`
**方法**: POST
**请求体**: `{}`
**返回**: [Topology](../references/data_structures.md#topology---拓扑视图数据)

获取机柜-节点的拓扑结构，用于可视化展示。

### 堆栈分析API

#### 5. 节点火焰图
**端点**: `/api/get_node_flamegraph`
**方法**: POST
**请求体**: `{"ip": "192.168.1.100"}`
**返回**: `String` (SVG格式的火焰图)

获取指定节点的合并火焰图SVG，可视化所有rank的调用栈。

**处理流程**：
1. 获取SVG数据
2. 保存为临时文件（如 `/tmp/flamegraph_{ip}.svg`）
3. 提供文件路径给用户
4. 解释火焰图的关键发现（热点函数、调用深度等）

#### 6. 所有节点火焰图
**端点**: `/api/get_all_nodes_flamegraph`
**方法**: POST
**请求体**: `{}`
**返回**: `String` (SVG格式的火焰图)

获取所有节点所有rank的合并火焰图SVG，全局视角的调用栈分析。

#### 7. 节点堆栈信息
**端点**: `/api/get_node_stacks`
**方法**: POST
**请求体**: `{"ip": "192.168.1.100"}`
**返回**: [NodeStacksResponse](../references/data_structures.md#nodestacksresponse---节点堆栈响应)

获取指定节点的详细堆栈信息，包括合并后的调用栈树。

### Step指标API (Phase 2)

#### 8. Step显示功能状态
**端点**: `/api/get_step_show_enabled`
**方法**: POST
**请求体**: `{}`
**返回**: `bool` (是否启用Step显示)

检查Step显示功能是否启用（需要环境变量 `STEP_SHOW=true`）。

#### 9. 全局Step指标
**端点**: `/api/get_global_step_metrics`
**方法**: POST
**请求体**: `{}`
**返回**: [GlobalStepMetrics](../references/data_structures.md#globalstepmetrics---全局step指标)

获取全局的Step指标，包括当前step、最近duration、显存分配等。

**使用场景**：
- 查看实时训练进度
- 分析step耗时趋势
- 监控显存使用情况

#### 10. Rank Step指标
**端点**: `/api/get_rank_step_metrics`
**方法**: POST
**请求体**: `{"ip": "192.168.1.100", "local_rank": 0, "rank_id": 0}`
**返回**: [RankStepMetrics](../references/data_structures.md#rankstepmetrics---rank-step指标)

获取指定rank的详细Step指标。

## 实用工作流

### 工作流1：全面集群诊断
1. 获取全局指标 → 了解总体状态
2. 获取节点列表（按SlowRatio降序） → 找到问题节点
3. 对问题节点获取rank详情 → 深入分析
4. 对问题节点获取火焰图 → 代码级诊断
5. 获取Step指标（如果启用） → 训练过程分析

### 工作流2：性能瓶颈分析
1. 获取节点列表（按StepTime降序） → 找到最慢节点
2. 获取慢节点的rank详情 → 查看GPU间差异
3. 获取慢节点的火焰图 → 分析热点函数
4. 检查NCCL延迟指标 → 网络通信分析

### 工作流3：训练进度监控
1. 检查Step功能是否启用
2. 获取全局Step指标 → 查看当前进度
3. 分析Step历史记录 → 识别趋势
4. 监控显存分配 → 预防内存泄漏

## 错误处理指南

### 常见错误及处理

| 错误现象 | 可能原因 | 解决方案 |
|---------|---------|---------|
| 连接失败 | 服务未运行/地址错误 | 1. 确认服务地址正确<br>2. 检查服务是否运行<br>3. 尝试默认地址 `http://127.0.0.1:3000` |
| 404错误 | API路径错误 | 1. 确认API端点名称正确<br>2. 确保路径为 `/api/{function_name}`<br>3. 检查服务版本 |
| 400错误 | 请求体格式错误 | 1. 检查JSON格式<br>2. 确认字段名正确<br>3. 参考本文档的请求体示例 |
| 500错误 | 服务端错误 | 1. 检查服务日志<br>2. 可能是Mock模式相关错误<br>3. 确认环境变量配置 |
| Step功能未启用 | `STEP_SHOW` 环境变量未设置 | 1. 告知用户需要设置 `STEP_SHOW=true`<br>2. 使用基础指标替代 |

### Mock模式说明
如果环境变量 `COLLECTOR_MOCK_MODE=true`，API会返回模拟数据而非真实数据。这在以下情况有用：
- 开发和测试
- 演示和展示
- 真实集群不可用时

## 高级技巧

### 批量操作脚本
对于需要获取多个节点数据的情况，可以编写简单脚本批量处理：

```bash
#!/bin/bash
# 批量获取节点火焰图
for ip in "192.168.1.100" "192.168.1.101" "192.168.1.102"; do
  curl -X POST http://127.0.0.1:3000/api/get_node_flamegraph \
    -H "Content-Type: application/json" \
    -d "{\"ip\": \"$ip\"}" > "flamegraph_$ip.svg"
done
```

### 数据持久化与比较
定期获取指标数据并保存，用于趋势分析：
1. 每小时获取全局指标并保存时间戳
2. 对比历史数据识别性能退化
3. 建立性能基线告警

### 自动化诊断规则
基于指标数据自动诊断常见问题：

```javascript
// 伪代码：自动诊断
function diagnoseNode(node) {
  if (node.slow_ratio > 0.3) {
    return "⚠️ 慢Rank占比过高，可能存在性能瓶颈";
  }
  if (node.avg_gpu_utilization < 50) {
    return "⚠️ GPU利用率低，可能存在数据加载或同步问题";
  }
  if (node.avg_nccl_latency_ms > 10) {
    return "⚠️ NCCL延迟过高，检查网络连接";
  }
  return "✅ 节点运行正常";
}
```

## 集成示例

### 与Claude Code集成
当用户在工作区中运行训练监控面板时，可以直接使用此技能：

```markdown
用户：我的训练监控面板运行在 3000 端口，帮我看看集群状态
Claude：好的，我将使用超级训练监控技能获取数据...
1. 发送请求到 http://127.0.0.1:3000/api/get_global_metrics
2. 解析响应并展示关键指标
3. 提供进一步分析建议
```

### 与开发工作流集成
在开发调试分布式训练任务时：
1. 启动训练任务
2. 启动监控面板
3. 使用本技能实时监控
4. 根据指标调整训练参数

---

## 快速参考卡片

### API速查表
```
全局指标     : POST /api/get_global_metrics
节点列表     : POST /api/get_nodes
节点详情     : POST /api/get_node_ranks {"ip": "..."}
节点火焰图   : POST /api/get_node_flamegraph {"ip": "..."}
全局火焰图   : POST /api/get_all_nodes_flamegraph
Step指标    : POST /api/get_global_step_metrics
Rank Step   : POST /api/get_rank_step_metrics {"ip": "...", "local_rank": 0, "rank_id": 0}
```

### 关键指标阈值
- 🟢 **健康**：slow_ratio < 0.2, GPU利用率 > 70%, NCCL延迟 < 5ms
- 🟡 **警告**：slow_ratio 0.2-0.4, GPU利用率 50-70%, NCCL延迟 5-10ms
- 🔴 **故障**：slow_ratio > 0.4, GPU利用率 < 50%, NCCL延迟 > 10ms

### 默认地址
- 开发环境：`http://127.0.0.1:3000`
- 生产环境：根据实际部署确定

---

**提示**：使用此技能时，始终从获取全局指标开始，以了解集群总体状态，然后根据需要深入分析特定节点或问题。