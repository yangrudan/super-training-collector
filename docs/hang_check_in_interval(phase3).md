# HANG 监测与分析

目标：以 40~70s 的随机间隔采集训练任务堆栈，默认每轮从训练节点中随机选择 8 个节点（节点数不足 8 时全部采集），并在同一轮内保持相同节点连续采样 3 次。系统按 rank 维度比较相邻采样的堆栈变化：当某节点至少 75% rank 连续达到高相似度或持续采集失败时，该节点判为 HANG；全局再要求至少 6 个节点 HANG，且全局 HANG rank 数至少 60（8 节点 × 8 rank 时为 60/64）才确认训练任务 HANG。若确认 HANG，Dashboard 首页显示红灯，并触发日志、火焰图、问题 Rank 分析和告警通知。

## 设计说明

- 基于已有的堆栈采集和合并逻辑，按节点采集该节点下所有 rank 的堆栈；
- 按 rank 维护历史堆栈、连续高相似计数、连续采集失败计数；
- 把堆栈看作集合，用 Jaccard 计算相似度；
- 节点级判定使用 rank 比例阈值，默认至少 75% rank 表现出 HANG 证据；
- 全局级判定同时使用 HANG 节点数和 HANG rank 总数，避免少数节点或少数 rank 偶发异常直接触发全局 HANG。

## Jaccard相似度

把堆栈变成集合，比较"重叠程度"

J(A, B) = |A ∩ B| / |A ∪ B|
       = 交集大小 / 并集大小

取值范围：[0, 1]
1.0 = 完全一样（两个集合完全相同）
0.0 = 完全不一样（没有共同元素）
0.5 = 一半重叠

## 检测步骤

1. 每个 tick 在 `[40s, 70s]` 内随机等待，减少与训练 step 周期共振。
2. 若需要开启新一轮检测，则随机选择最多 8 个节点，并在本轮连续采样中保持节点集合不变。
3. 对每个节点采集所有 rank 堆栈，将每个 rank 的堆栈转换为集合；默认保留行号，用于识别函数内代码是否仍在推进。
4. 对每个 rank 计算当前堆栈与上次堆栈的 Jaccard 相似度。
5. 单个 rank 满足任一条件时计入 HANG 证据：
   - 连续 3 次相似度 `>= 0.95`；
   - 曾经成功采集过，但后续连续 3 次采集失败或超时。
6. 单个节点满足 HANG rank 数 `>= ceil(rank_count * HANG_NODE_RANK_QUORUM)` 时判为 HANG，默认 `0.75`；例如 8 rank 节点至少 6 个 rank 有 HANG 证据才算节点 HANG。
7. 全局 HANG 需同时满足：
   - 有效节点中 HANG 节点数占比 `>= 50%`；
   - HANG 节点数 `>= HANG_GLOBAL_MIN_HANG_NODES`，默认 `6`，小规模采样会按有效节点数自动夹紧；
   - HANG rank 总数 `>= HANG_GLOBAL_MIN_HANG_RANKS`，默认 `60`，小规模采样会按有效 rank 总数自动夹紧。
8. HANG 状态下只有连续 `HANG_RECOVERY_NORMAL_ROUNDS` 轮 Normal（默认 3）才认为当前采样未满足 HANG 条件，并发送保守恢复提示。

---

## 实现详情

### 模块结构

```
app/src/hang_detector/
├── mod.rs           # 模块导出
├── config.rs        # 配置管理（环境变量）
├── jaccard.rs       # Jaccard 相似度计算
├── detector.rs      # 核心检测逻辑
├── runner.rs        # 服务端异步运行与告警编排
├── scheduler.rs     # 检测器封装与调度辅助
├── state.rs         # 全局状态管理
├── logger.rs        # HANG 日志记录（SSR only）
├── notifier.rs      # 钉钉/内网后台告警
└── job_info_client.rs # 作业信息客户端
```

### 环境变量配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `HANG_CHECK_ENABLED` | `false` | 是否启用 HANG 检测 |
| `HANG_SAMPLE_INTERVAL_MIN_SECS` | `40` | 采样间隔下界（秒，≥10）。每 tick 在 `[min, max]` 内随机睡眠，避免与训练 step 周期共振。 |
| `HANG_SAMPLE_INTERVAL_MAX_SECS` | `70` | 采样间隔上界（秒）。若 `min > max` 自动交换。 |
| `HANG_SAMPLE_INTERVAL` | *(无)* | **向后兼容**：仅设此变量时等价于 `min = max = 该值`（固定间隔）。同时设了 `_MIN_SECS` / `_MAX_SECS` 时被覆盖。 |
| `HANG_SAMPLE_COUNT` | `3` | 连续采样次数 |
| `HANG_NODE_COUNT` | `8` | 采样节点数 |
| `HANG_JACCARD_THRESHOLD` | `0.95` | Jaccard 判定阈值 |
| `HANG_NODE_RANK_QUORUM` | `0.75` | 节点判 HANG 所需的"hang rank 比例"。例如 8 rank 节点至少 6 个 rank 连续满足 HANG 证据才会把节点判为 HANG。范围 `[0.1, 1.0]`。 |
| `HANG_KEEP_LINE_NUMBERS` | `true` | Jaccard 比较时是否保留行号。默认保留行号，以区分函数内代码推进和真实卡死。 |
| `HANG_GLOBAL_MIN_HANG_NODES` | `6` | 全局判 HANG 所需的最少 HANG 节点绝对数，与 ≥50% 节点票数共同生效；有效节点少于 6 时自动夹紧到有效节点数。 |
| `HANG_GLOBAL_MIN_HANG_RANKS` | `60` | 全局判 HANG 所需的最少 HANG rank 绝对数。8 节点、每节点 8 rank 时，64 个 rank 中至少 60 个 rank 有 HANG 证据；有效 rank 少于 60 时自动夹紧到有效 rank 总数。 |
| `HANG_RECOVERY_NORMAL_ROUNDS` | `3` | 连续多少轮 Normal 后才认为当前采样未满足 HANG 条件并发送保守的恢复提示。 |
| 恢复黑名单 | `Py_FinalizeEx`, `~ProcessGroupMCCL` | 当前处于 HANG 时，若采样堆栈命中这些退出/进程组销毁帧，不累计恢复轮次。 |
| `HANG_BLOCKING_PATTERNS` | *(空)* | 白名单模式（逗号分隔）。**默认为空**：`checkpoint` / `DataLoader` 等子串会与 Megatron activation checkpointing、PyTorch 训练栈中无处不在的 DataLoader 帧撞名，反而掩盖真 HANG。如需启用请显式配置高特异性函数名（如 `save_checkpoint_to_disk`）。 |
| `HANG_INTRANET_ALERT_DELAY_SECS` | `900` | 首次检测到 HANG 后，延迟多少秒再发送内网后台告警。钉钉告警不受此延迟影响。 |
| `INTRANET_ALERT_ENABLED` | `false` | 是否启用内网后台告警；未启用时只发钉钉告警。 |
| `USER_DINGBOT` | *(空)* | 可选用户自定义钉钉机器人 Webhook；设置后与主通知并行发送。 |
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
  "hang_duration_secs": 180,
  "stc_uptime_secs": 7385,
  "selected_node_count": 8,
  "valid_node_count": 8,
  "hang_node_count": 6,
  "hang_rank_count": 60,
  "total_rank_count": 64,
  "avg_similarity": 0.97,
  "max_similarity": 0.99,
  "config": {
    "sample_interval_secs": 55,
    "sample_interval_min_secs": 40,
    "sample_interval_max_secs": 70,
    "sample_count": 3,
    "node_count": 8,
    "node_rank_quorum": 0.75,
    "global_min_hang_nodes": 6,
    "global_min_hang_ranks": 60,
    "jaccard_threshold": 0.95
  },
  "detection_state": {
    "event_id": 1716445200,
    "hang_first_detected_at": 1716445365,
    "normal_observed_since": 1716437980,
    "stc_uptime_secs": 7385,
    "hang_duration_secs": 180,
    "selected_nodes": ["192.168.1.1", "192.168.1.2"],
    "sample_round": 3,
    "consecutive_normal_count": 0,
    "hang_notified": false,
    "hang_intranet_notified": false,
    "hang_notify_in_flight": false,
    "pending_recovery": null
  },
  "node_observations": [
    {
      "node_ip": "192.168.1.1",
      "observation": "Hang",
      "similarity": 0.98
    }
  ]
}
```

### 设计决策

1. **随机采样间隔**：默认 40~70s，避免固定间隔与训练 step 或 checkpoint 周期同频共振。
2. **采样节点数**：默认每轮采样 8 个节点；若实际节点数不足 8，则采集全部节点。
3. **per-rank 判定**：不再把一个节点的所有 rank 简单合并成一个集合，而是分别维护每个 rank 的历史和连续计数，减少少数 rank 波动对节点判断的干扰。
4. **Jaccard 粒度**：默认保留行号（`HANG_KEEP_LINE_NUMBERS=true`），以区分函数内代码仍在推进和真正卡死；可通过环境变量关闭以兼容旧行为。
5. **节点级比例阈值**：节点判 HANG 要求 HANG rank 比例达标；默认 8 rank 节点需要至少 6 个 rank 满足 HANG 证据。
6. **持续采集失败处理**：曾经成功采集过的 rank 如果连续失败/超时达到采样次数，也计入 HANG 证据；冷启动首轮全空不会误判。
7. **全局双绝对阈值**：有效节点中 HANG 节点占比需 ≥50%，同时至少 6 个节点 HANG、至少 60 个 rank 有 HANG 证据，用于抑制少数节点或少数 rank 误报。
8. **白名单默认关闭**：`HANG_BLOCKING_PATTERNS` 默认为空；`checkpoint`、`DataLoader` 等泛化关键字容易与训练栈常见帧撞名，不再默认屏蔽。
9. **恢复去抖动**：HANG 后必须连续多轮 Normal 才恢复；若命中 `Py_FinalizeEx`、`~ProcessGroupMCCL` 等退出/销毁帧，不累计恢复轮次。

### API 端点

- `GET /api/hang_status` - 获取当前 HANG 检测状态
- `GET /api/hang_check_enabled` - 检查 HANG 检测是否启用

`/api/hang_status` 的 `details` 会额外返回便于展示的运行统计：

- `hang_duration_secs`: 当前 HANG 事件估算已持续时长。进入 HANG 时会按采样窗口回溯，因此首次告警不会接近 0。
- `stc_uptime_secs`: STC 从启动到现在的守护时长。
- `selected_node_count` / `valid_node_count` / `hang_node_count`: 本轮节点覆盖面与 HANG 节点数量。
- `hang_rank_count` / `total_rank_count`: 本轮有 HANG 证据的 rank 数和有效 rank 总数。
- `avg_similarity` / `max_similarity`: 本轮有效节点的平均/最高 Jaccard 相似度。

### 告警信息

HANG 告警会同时展示两类时长：

- **HANG 已持续**: 当前 HANG 事件估算持续时长，用于判断卡住多久。
- **此次任务已守护时长**: STC 从启动到当前的守护时长，用于判断本轮任务已经被检测器覆盖多久。

告警还会展示节点判定覆盖面（例如 `HANG 6/8，有效 8/8`）、Rank 判定覆盖面（例如 `HANG 60/64`）、平均/最高相似度，以及自动问题 Rank 分析摘要。

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

少数派只作为候选。系统随后直接从 Collector 进程环境读取 Megatron 的 `TP`、`PP`、`DP`、`EP`、`CP` 和 `RANK_ORDER`（同时兼容 `*_SIZE` 别名），按 `parallel_state.py` 的 mixed-radix 规则恢复通信组，并增加两类证据：

- **通信组证据**：候选分叉签名在 PP/DP/TP/EP/CP 组内是离群，或覆盖了整组。
- **跨副本证据**：相同模型坐标的分叉签名在至少两个 DP 副本上重复出现，且覆盖率达到 50%。

采集失败直接记为高置信度；同时具备通信组和跨副本证据记为高，仅一种记为中，仅有全局少数派证据记为低。拓扑获取或校验失败时不会阻断分析，Dashboard 和告警会明确标记已降级为旧的全局少数派算法。Megatron 的 dense CP 网格与 expert EP 网格分别生成，避免把 CP/EP 错误压成一个统一坐标。

完整 world size 优先取 `/apis/nodes` 中存活 Rank 报告的 `world_size`，其次在显式配置 `DP` 时由 `TP × PP × CP × DP` 推导。当前 Rank 列表允许存在空洞：未注册或已退出的 Rank 会作为“训练进程缺失”高置信度根因加入结果，拓扑仍按完整 world size 构建；调用栈请求失败则单独标记为“调用栈采集失败或返回空栈”。

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
| `TP` | `1` | Megatron Tensor Parallel 并行度；兼容 `TP_SIZE` |
| `PP` | `1` | Megatron Pipeline Parallel 并行度；兼容 `PP_SIZE` |
| `DP` | 自动推导 | Data Parallel 并行度；兼容 `DP_SIZE`，显式配置时必须与 world size 一致 |
| `EP` | `1` | Megatron Expert Parallel 并行度；兼容 `EP_SIZE` |
| `CP` | `1` | Megatron Context Parallel 并行度；兼容 `CP_SIZE` |
| `RANK_ORDER` | `tp-cp-ep-dp-pp` | Megatron mixed-radix 排布顺序，左侧维度变化最快 |

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
