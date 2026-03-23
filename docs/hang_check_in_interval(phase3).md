# HANG 监测与分析

目标: 间隔30s采集一次，采集n个节点的堆栈3次(n=4, 从采集的node信息里随机挑选4个, 若节点数小于4个则全部采集, 注意连续的3次采集中要保证是4个相同的node, 下一轮的3次采集再随即更换node)。判断每个节点的堆栈间隔30s的3次采样是否发生变化，用来判断训练任务是不是Hang住, 若判断HANG, 则在Dashboard首页显示一个警告红灯.

## 设计说明

- 基于已有的堆栈采集和合并逻辑, 针对单个节点的定时采集, 得到各个节点下各个rank的堆栈;
- 通过数据结构如VecDeque来存储历史数据;
- 把堆栈看作集合，用 Jaccard 计算相似度;

## Jaccard相似度

把堆栈变成集合，比较"重叠程度"

J(A, B) = |A ∩ B| / |A ∪ B|
       = 交集大小 / 并集大小

取值范围：[0, 1]
1.0 = 完全一样（两个集合完全相同）
0.0 = 完全不一样（没有共同元素）
0.5 = 一半重叠

## 步骤

- 将各个节点的堆栈转换程集合;
- 计算和上一次采集的交集;
- 计算和上一次采集的并集;
- 计算Jaccard;
- 连续 3 次：Jaccard = 1.0（完全相同)或大于0.95则判断HANG住.

---

## 实现详情

### 模块结构

```
app/src/hang_detector/
├── mod.rs           # 模块导出
├── config.rs        # 配置管理（环境变量）
├── jaccard.rs       # Jaccard 相似度计算
├── detector.rs      # 核心检测逻辑
├── scheduler.rs     # 定时任务调度
└── state.rs         # 全局状态管理
```

### 环境变量配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `HANG_CHECK_ENABLED` | `false` | 是否启用 HANG 检测 |
| `HANG_SAMPLE_INTERVAL` | `30` | 采样间隔（秒） |
| `HANG_SAMPLE_COUNT` | `3` | 连续采样次数 |
| `HANG_NODE_COUNT` | `4` | 采样节点数 |
| `HANG_JACCARD_THRESHOLD` | `0.95` | Jaccard 判定阈值 |
| `HANG_BLOCKING_PATTERNS` | `checkpoint,save_model,load_data,DataLoader` | 白名单模式（逗号分隔） |

### 设计决策

1. **Jaccard 计算粒度**: 按帧拆分，忽略行号，只比较 `func (file)`
2. **多 Rank 聚合**: 合并同一节点所有 rank 的堆栈为一个集合
3. **节点失败处理**: 该轮次无效，下轮重新选择节点
4. **投票机制**: >= 50% 节点被判定为 HANG，则全局状态为 HANG
5. **白名单**: 配置已知长阻塞模式（checkpoint/dataloader等），匹配时不判定为 HANG

### API 端点

- `GET /api/hang_status` - 获取当前 HANG 检测状态
- `GET /api/hang_check_enabled` - 检查 HANG 检测是否启用

### 前端组件

- `HangIndicator` - 完整版 HANG 状态指示灯（显示图标、文字和详情）
- `HangIndicatorCompact` - 简化版（只显示图标，用于标题栏）
