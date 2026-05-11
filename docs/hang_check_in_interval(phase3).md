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
├── state.rs         # 全局状态管理
└── logger.rs        # HANG 日志记录（SSR only）
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
| `HANG_LOG_ENABLED` | `true` | 是否启用 HANG 日志记录（需 HANG_CHECK_ENABLED=true） |
| `OUTPUT_DIR` | - | 输出目录（hang日志存储在 `$OUTPUT_DIR/hang_logs`） |
| `HANG_LOG_DIR` | `hang_logs` | HANG 日志保存目录（OUTPUT_DIR 优先级更高） |

### HANG 日志记录

当检测到 HANG 时，系统会自动将堆栈信息导出到本地文件：

- **存储路径优先级**: `$OUTPUT_DIR/hang_logs` > `$HANG_LOG_DIR` > `hang_logs`（相对路径）
- **日志路径**: `{日志目录}/hang_YYYYMMDD_HHMMSS.json`
- **全局火焰图**: `{日志目录}/hang_YYYYMMDD_HHMMSS.svg`
- **防重复机制**: 持续处于 HANG 状态时只记录一次，状态解除后才能再次记录

**全局火焰图**: 检测到 HANG 时，系统会采集**所有节点的所有 rank** 的堆栈数据，合并生成一张全局火焰图 SVG，可在浏览器中打开查看 HANG 时的完整堆栈分布。

**日志文件内容**:
```json
{
  "timestamp": "2024-01-01T12:00:00.000+0800",
  "hang_nodes": ["192.168.1.1", "192.168.1.2"],
  "node_similarities": {
    "192.168.1.1": 0.98,
    "192.168.1.2": 0.96
  },
  "node_stacks": {
    "192.168.1.1": [["frame1", "frame2"], ["frame3", "frame4"]]
  },
  "consecutive_high_similarity": 3,
  "config": {
    "sample_interval_secs": 30,
    "sample_count": 3,
    "node_count": 4,
    "jaccard_threshold": 0.95
  }
}
```

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

---

## 问题 Rank 自动检测（StackTrie 分叉分析）

当 HANG 检测确认后，系统会自动分析所有 Rank 的堆栈，找出偏离多数执行路径的「问题 Rank」。Dashboard 上的手动分析入口会先检查当前是否已经 HANG，未检测到 HANG 时不会继续分析。

### 算法原理

利用已有的 **StackTrie** 数据结构（合并堆栈 + RoaringBitmap rank 归属），在每个分叉点（节点有多个 children）检查各分支的 rank 覆盖率：

1. 采集所有节点全部 Rank 的堆栈，构建 StackTrie
2. 从 root 遍历，在每个多 children 节点检查各分支覆盖率
3. `覆盖率 = 该分支 rank 数 / 父节点 rank 数`
4. 覆盖率 < 阈值（默认 30%）的分支中的 Rank 被标记为「少数派」
5. 统计每个 Rank 出现在少数派分支的次数，作为 `anomaly_score`
6. `anomaly_score > 0` 的 Rank 即为问题 Rank，分数越高异常越严重

### 触发方式

| 触发方式 | 说明 |
|---------|------|
| HANG 自动触发 | HANG 检测确认后，自动采集全局堆栈并分析 |
| Dashboard 手动触发 | Level 2「问题 Rank 分析」Tab 中点击按钮；仅在当前状态为 HANG 时可执行 |

### 手动分析阈值

Dashboard 上的手动分析支持调整少数派阈值，默认是 `30%`。阈值越低，越容易把更多 Rank 标记为问题 Rank，从而展示更多异常分叉信息；阈值越高，则只保留更明显的异常。

- 调整范围: `5%` ~ `50%`
- 适用场景: 想看更细的分叉差异时降低阈值；想收敛到更明确异常时提高阈值
- 生效方式: 在「问题 Rank 分析」面板里调整后再点击分析按钮

### 配置

| 环境变量 | 默认值 | 说明 |
|---------|--------|------|
| `RANK_ANALYSIS_ENABLED` | `true` | 是否启用问题 Rank 分析 |
| `RANK_ANALYSIS_MINORITY_THRESHOLD` | `0.3` | 少数派阈值 (0.05-0.5) |

### API 端点

- `AnalyzeProblematicRanks` - 手动触发实时分析（会先检查当前是否 HANG；未 HANG 时直接跳过）
- `GetProblematicRanks` - 获取最近一次分析结果（缓存）

### 前端组件

- `RankAnalysisPanel` - 完整分析面板（Level 2 Tab：按钮 + 结果表格 + 分叉详情）
- `RankAnalysisSummary` - 紧凑摘要（Level 1 首页：有问题 Rank 时自动显示）

### 模块结构

```
app/src/rank_analyzer/       # SSR-only 模块
├── mod.rs                   # 模块导出
├── config.rs                # 配置管理（环境变量）
├── analyzer.rs              # 核心分叉检测算法
├── types.rs                 # 类型重导出
└── state.rs                 # 全局结果缓存

app/src/rank_analysis_types.rs  # 共享类型（SSR + Client）
app/src/components/rank_analysis.rs  # UI 组件
```
