# HANG 检测模块设计评审报告

> 仓库: `super-trainning-collector`
> 范围: `app/src/hang_detector/` + `docs/hang_check_in_interval(phase3).md` + `hang_logs/` 样本
> 性质: 只做现状评审 —— 不改代码

---

## 1. 现状速览（它在做什么）

一句话: **每隔 30s，从集群里随机挑 4 个节点，连续采样 3 次堆栈，用 Jaccard 相似度判断"堆栈是否不动"，≥50% 节点不动就判定任务 HANG，落盘 JSON+SVG 并发钉钉。**

关键链路:

| 层 | 文件 | 职责 |
|---|---|---|
| 配置 | `config.rs` | 环境变量驱动（阈值 0.95、间隔 30s、节点 4、采样 3、白名单 checkpoint/save_model/load_data/DataLoader） |
| 状态 | `state.rs` | 全局 `RwLock<HangDetectorState>`，每节点保存 `VecDeque<HashSet<String>>` 历史 + `high_similarity_count` |
| 相似度 | `jaccard.rs` | 把一个 rank 的 frames 列表转 `HashSet<String>`，去除行号，然后 `|A∩B| / |A∪B|` |
| 判定 | `detector.rs` | 单节点逐帧合并多 rank→ 集合；与上一次做 Jaccard；连续 ≥ sample_count 次 ≥ 阈值 且 不命中白名单 ⇒ 节点 HANG |
| 聚合 | `detector.update_global_status` | ≥50% 节点 HANG ⇒ 全局 HANG（二值：Hang / Normal） |
| 调度 | `runner.rs` | tokio 定时器；一旦 `sample_round > sample_count` 或节点列表空就重选节点并 `reset_round` |
| 外发 | `logger.rs` + `notifier.rs` | 落 `hang_YYYYMMDD_HHMMSS.json/.svg`、钉钉 webhook（硬编码） |

---

## 2. 优点（做得对的地方）

1. **判据简单且可解释**。Jaccard 就是"堆栈是否还在动"的直觉量化，阈值和计数都可由环境变量调，运维能自己搬旋钮。
2. **采样开销可控**。只抽 N=4 节点、每 30s 一次，避免给千卡规模的 profiling endpoint 造成 fan-in 风暴；千卡全采会立刻把 master 打爆。
3. **白名单兜底**。checkpoint/DataLoader/save_model 这类"真的会长时间阻塞但不是故障"的场景明确排除，降低误报 —— 这是经验之谈。
4. **事件触发全局火焰图 + 防抖**。`hang_logged/hang_notified` 双标记保证一次 HANG 事件只落一次盘一次告警，直到状态 recovery 才 reset，避免连环刷屏。
5. **采集失败 fail-safe 正确**。`current_set.is_empty()` 不入状态；Jaccard 两侧任一空集直接返回 0.0 —— 避免"服务挂了被当成 HANG"的典型二义性错误。
6. **模块边界清晰**。config/state/jaccard/detector/scheduler/runner/logger/notifier 分层合理，`#[cfg(feature = "ssr")]` 把 I/O 模块隔离在服务端，WASM 端只带纯逻辑。
7. **有单元测试覆盖核心路径**。Jaccard、select_nodes、merge_rank_stacks、env parsing 都有 case。

---

## 3. 缺陷与风险（按严重度从高到低）

### P0 — 会导致漏报或误判的根本性问题

**P0-1 多 rank 堆栈在节点内被 union，丢失"谁卡住"信息**
`merge_rank_stacks` 把一个节点上所有 rank 的堆栈并成一个大集合。后果:
- 8 卡节点里只有 rank 3 卡在 NCCL allreduce，其他 7 卡在正常训练 —— merge 后集合持续变化，Jaccard 被稀释，**漏报**。
- 而真正的大模型 HANG 往往就是**个别 rank 卡死导致集合通信死锁**，这恰恰是系统最该抓的场景。
- 建议的分析维度：per-rank 计算 Jaccard，再在节点内取"有 X% rank 不动"作为节点级信号。

**P0-2 Jaccard 对"stuck in a busy loop"无能为力**
Jaccard 只看集合，不看调用链顺序，也不看栈顶。但真实 HANG 场景里有一类是**tight CPU spin / retry loop**（例如 NCCL watchdog 重试、CUDA 驱动 busy-wait），栈顶函数每次采样稳定，但集合几乎不变 → 会被正确识别；反之，一个正常的推理循环在相同代码路径里反复执行也会 Jaccard=1.0 → **误报**。
换句话说，Jaccard=1.0 在短间隔下**既可能是 HANG，也可能是"循环稳定运行"**，判据不完备。
业界通用做法：比较栈顶 N 帧的 pointer/IP 是否一致，或引入**外部 liveness 信号**（step counter / iteration tick / loss update）做 cross-check。本项目有 Step Time 这个指标（README 提到）但 HANG 检测没有利用。

**P0-3 `frame.to_string()` 把整条 JSON（含 `ip`、`locals`）当字符串入集合**
看 `runner.rs::fetch_callstack` 把 `serde_json::Value` 的整个 frame 直接 `to_string()` 存，**ip 是地址、lineno 是行号、locals 是字典**。
- `jaccard.rs::stack_to_set` 设计的"去行号"正则是基于 `func (file:line)` 文本格式的，**对 JSON 字符串根本不匹配**（`rfind('(')` 找到的是 JSON 里其他位置的括号）。
- 实际效果：两次采样的 `ip` 地址相同、lineno 相同时才会判相等。对 Python 解释器循环完全可能每次 ip 微妙不同（JIT/eval 帧），或 locals 字典内容变动，Jaccard 严重偏低 → **漏报**。
- 看 `hang_logs/hang_20260414_144151.json` 就能印证：存下来的就是那种带 `"locals":{}` 的 JSON 字符串。
- 这是**设计与实现不一致**的典型：文档说比较 `func (file)`，实现在比 JSON 字符串。必须统一，要么 runner 格式化后入栈，要么 jaccard 解析 JSON 抽 `func+file`。

### P1 — 影响准确性和可运维性

**P1-1 随机抽 4 节点，覆盖率随集群规模线性下降**
1024 节点抽 4 ≈ 0.4% 采样率。若 HANG 只发生在 1~2 个节点（真实故障常态），抽中概率极低。
- 投票阈值 50% 也不合理：4 抽 2 才触发；真实场景 1024 节点里 2 个卡死就足以让整个训练 hang。
- 更合理：**所有节点都采 1 个 rank**（轻量），或**按拓扑分桶采样**（每 PP/TP group 挑一个），或**对 straggler 优先采样**（Step Time 异常的节点）。
- 现有架构 `callstack_base_port` endpoint 本就 per-rank，压力主要在结果聚合，可评估增大 N。

**P1-2 节点在一轮里固定 3 次采样，HANG 被确认最少需要 30s × 3 = 90s**
对大模型训练这已经不算长，但 + 下一轮换节点的话，如果本轮没抽中 HANG 节点，最坏要 **6×30s = 180s 以上**才可能报出。可考虑：轮次间**保留** "怀疑节点" 而不是完全随机。

**P1-3 白名单是子字符串匹配，易被绕过/误匹配**
`frame.contains("load_data")` 会命中任何带这个子串的路径，包括用户自己命名的函数。反过来，checkpoint 操作若函数名叫 `_save_state_dict_impl` 就不会命中。建议结构化规则（func 完全匹配、file path 前缀等）。

**P1-4 全局状态是单例 `static Lazy<RwLock<...>>`**
意味着一个进程只能监控一个训练任务。若以后想多租户（N 个 job 共用一个 collector）就要重构。目前场景够用，但记一笔。

**P1-5 钉钉 webhook token 硬编码在源码里**
`notifier.rs:8` 明文 token。一旦代码泄漏（GitLab 权限、第三方镜像），任何人可以给该群发消息。应走环境变量 + Secret。这条也许你们内部已知，但评审里必须点出来。

**P1-6 `update_global_status` 里当 `hang_count*2 >= total_count` 会把 `consecutive_high_similarity` 强制写成 `sample_count`**
这是展示字段而非判据字段，影响不大，但语义含糊：某节点可能刚刚 high_similarity_count=1，就因为多数投票被覆盖成 3。建议明确它代表"全局"还是"单节点"。

### P2 — 设计细节 / 可维护性

- `NodeStackHistory` 有 `previous()` 方法但 detector 用的是 `last()`（取刚 push 进去的那个与自己比？），实际逻辑是**先算 `last()` 再 push**，所以是对的 —— 但读起来反直觉，值得加一行注释或改名（`latest_committed()` 等）。
- `sample_count` 在 detector 中当"阈值"用（`high_similarity_count >= sample_count`），但在 history 里是 `sample_count+1` 的保留长度，语义耦合深，未来改一处容易漏改另一处。
- `HangStatus` 只有 Normal/Hang/Disabled/Warning，其中 Warning 在 detector 里**从未被产生**，属于僵尸状态。
- 文档里 `HANG_BLOCKING_PATTERNS` 默认值大小写处理和实现里不同（`DataLoader` vs 匹配时区分大小写），小心 FP。
- runner 里的 `rank_count.unwrap_or(4)` 是个魔数，集群里真实 rank_count 拿不到时用 4 会造成 URL 构造错。
- 没有对"节点堆栈采集 endpoint 本身不可用（端口 9933 超时）"和"节点真的 HANG 导致不响应"做区分。这两种情况都会走到 `fetch_stacks` 失败路径，被当成"本轮没采到"默默丢弃 —— **真 HANG 节点可能恰好表现为无响应，却被当成噪声过滤**。

### P3 — 可观测性

- 没有历史时间序列持久化。只有"HANG 时写一个快照"，但无法回看"过去 1 小时每个节点的 Jaccard 曲线"。对事后定位根因很可惜。
- 没有暴露 Prometheus / metrics endpoint，只有 `/api/hang_status` 单点查询，告警方只能轮询。
- JSON 日志里 `node_stacks` 存的是原始 JSON 字符串数组，不便人工阅读；SVG 是好的，JSON 里可以再冗余一份 human-readable 的 `func@file`。

---

## 4. 对大规模模型训练故障的理解 —— 这套判据覆盖到了哪些，漏了哪些

大规模（千卡+）训练的常见故障，按"本检测器能否发现"分类：

### 能抓住的 ✅
- **纯进程死锁**：所有 rank 都停在同一个 wait（NCCL barrier、mutex、futex）。Jaccard=1.0 持续 3 轮，90s 可检出 —— 这是本设计的目标场景。
- **个别节点卡死 + 多数节点也停在 allreduce 等待**：只要被抽中的 4 个节点里有 2+ 个停在等待，就能报。

### 容易漏 ❌
- **单点故障（1 个 rank 卡死）导致 N-1 个 rank 在 allreduce 超时前一直忙等**：抽 4 中 2 的概率太低。且 N-1 个节点的堆栈可能每次都在重试路径的不同分支，Jaccard 不会稳定 1.0。
- **慢 straggler**：不是 hang，只是某 rank 变慢 10 倍，step time 从 1s 拉到 10s，集群吞吐崩掉。Jaccard 看不出来，得看 Step Time / 拓扑热力图（README 里提到了，但没联动）。
- **NCCL ring 局部撕裂**：某个 PP stage 停住，其他 stage 还在推进但 pipeline bubble 越来越大。各节点堆栈差异大，Jaccard 波动。
- **CUDA driver/硬件故障**：GPU XID error、ECC uncorrectable、NVLink 断连。这类通常**堆栈正常但 step time 停更**；外部 dmesg / DCGM 信号才是金标准。
- **Checkpoint/IO 长尾**：白名单帮你忽略了 —— 但反过来，**真的卡在 save_checkpoint 半小时没写完**（对象存储挂了）也不会报，得靠独立 IO 超时监测。
- **OOM-kill 引发的 rank 消失**：进程都没了，端口连不上，本检测器走 fail-safe 丢弃 —— 反而**把最严重的故障当成噪声**。

### 业界/经验上更鲁棒的互补手段（不是要求你实现，只是评审视角）
1. **`torch.distributed` monitored barrier / heartbeat**：每 N 秒每个 rank 上报自己的 step/iter 编号，master 比对是否所有 rank 都前进了。**这是工业界（Meta / NVIDIA）实际用的主力机制**。本项目 Step Time 指标已经采了，但没做联动。
2. **基于 py-spy / 自带 stack sampler 的 diff 算法**：和本项目一致，但通常比较**栈顶 K 帧**（k=5~10）而不是整个栈的集合，噪声小且判据更强。
3. **异常检测**：对 Step Time 做 rolling mean + 3σ，超阈值先标记 suspect 再去采栈 —— 用快信号驱动慢信号，降低随机抽样的盲目性。
4. **拓扑感知采样**：让每个 DP/TP/PP group 至少被覆盖一个节点，而非全局随机。

---

## 5. 总体评分（主观）

| 维度 | 评分 | 备注 |
|---|---|---|
| 思路合理性 | ⭐⭐⭐⭐ | Jaccard + 白名单 + 防抖 是务实的 MVP |
| 实现一致性 | ⭐⭐ | stack_to_set 和 JSON frame 格式不匹配，P0-3 要修 |
| 误报控制 | ⭐⭐⭐ | 白名单 + 采集失败 fail-safe 做得好 |
| 漏报控制 | ⭐⭐ | 多 rank union + 4 节点随机 + 无 liveness 联动，漏报面大 |
| 可观测性 | ⭐⭐ | 只在事件点落盘，缺历史曲线和 metrics |
| 可运维性 | ⭐⭐⭐⭐ | 环境变量配置完备 |
| 安全性 | ⭐⭐ | webhook token 硬编码 |

---

## 6. 一句话结论

**这是一套"能工作、误报可控、但漏报盲区大"的 MVP**。对"所有 rank 一起卡死"这种入门级 hang 足够用；对"真实千卡集群里最该抓的——少数 rank 卡死拖垮全局"这种场景，命中率偏低，核心瓶颈在于 (a) 随机抽样 4 节点覆盖不足、(b) 多 rank union 稀释信号、(c) 堆栈表示与比较算法不匹配、(d) 没有与 Step Time 等 liveness 信号联动。要迈到"生产级训练故障检测"，建议把 HANG 检测从"孤立的堆栈采样器"升级为"liveness 信号驱动的分级诊断器"。
