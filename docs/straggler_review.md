# Straggler 检测专项评审报告

> 仓库: `super-trainning-collector`
> 范围: `app/src/models.rs`、`app/src/adapter.rs` 中与 step_time / slow_ratio / nccl_latency 相关的逻辑
> 性质: 只做现状评审 + 设计建议，不改代码
> 配套: 另见 `plan.md`（HANG 检测评审）

---

## 1. 仓库现状（straggler 侧）

### 已采集的指标（`models.rs`）

| 层级 | 指标 |
|---|---|
| Rank | `step_time_ms`, `step_time_ratio`（相对全局 P50 的倍数）, `gpu_utilization`, `nccl_latency_ms`, `nccl_bandwidth_gbps` |
| Node | `slow_ratio`, `avg/p50/p99_step_time_ms`, `avg_gpu_utilization`, `avg_nccl_latency_ms` |
| Global | `global_p50/p99_step_time_ms`, `global_avg_gpu_utilization` |

### 现有 straggler 判据（`adapter.rs`）

- **Node 层**：`slow_ratio = warning_count / rank_count`（warning 由单 rank 的 `step_time_ratio >= 2.0` 一类硬阈值触发）
- **Global 层**：`slow_node_count = nodes.filter(|n| n.slow_ratio > 0.1).count()`

### 一句话问题

判据是**固定倍数阈值 + 单时刻快照**，没有时序、没有拓扑、没有统计显著性、没有和 HANG 检测联动。

---

## 2. Straggler 在大模型训练里到底是什么

### 定义

一个 rank/node 的有效算力或通信吞吐持续低于同组 peers，导致整个 collective 或 pipeline 被它卡到。它**不卡死**（所以 HANG 检测抓不到），但会让集群实效吞吐按最慢那个走。

### 常见成因（按频次粗排）

1. **热点/降频**：GPU 温度墙、PSU 供电不足、SMBus 风扇异常 → SM clock 从 1980 MHz 掉到 1200 MHz，算力损失 40%
2. **PCIe/NVLink 降速**：链路协商到 Gen3 x8 而非 Gen4 x16，或 NVLink lane 掉到 half-width
3. **ECC 错误率高**：driver 开启 retirement/重试，kernel launch 延迟上升
4. **网络**：一条 IB 链路丢包重传、RoCE ECN 背压、交换机端口 buffer 异常 → allreduce 延迟从 5ms 拉到 50ms
5. **CPU/IO 邻居干扰**：数据预处理进程被共享节点上的其他 job 挤压、NFS/对象存储 metadata 锁竞争
6. **内存压力**：host memory 紧张触发 THP 合并 / swap，DataLoader 长尾
7. **混合精度/调度**：某个 rank 上的 autograd 触发了意外的 fp32 回退
8. **软件 bug**：某个 rank 的 CUDA graph capture 失败 fallback 到 eager

### 分类视角（对检测策略影响很大）

- **持续型 vs 间歇型**：持续型靠"均值 + 长时窗"就能抓；间歇型必须靠"P99/尾延迟 + 短时窗"
- **独占型 vs 相对型**：某 rank 独自慢（硬件故障）vs 多 rank 相对偏慢（TP group 都慢，其中一个拖住了别人）
- **计算型 vs 通信型**：
  - step_time 涨 + NCCL 延迟正常 → 计算侧
  - NCCL 延迟涨 + 单卡 compute 正常 → 通信侧
  - 两者都涨 → 通常是**被别人拖的受害者**

### 最反直觉的陷阱

**慢的那个 rank 往往指标最正常，因为别人都在等它；被它拖住的那些 rank 反而显示成"NCCL 慢"。**

集合通信 + 同步 SGD 下，所有 rank 的 step_time 会被 barrier 同步，最终**全部趋近于最慢那个**。原始 step_time 里"谁是元凶"会被掩盖。这是 straggler 检测最核心的难点。

---

## 3. 现有判据的缺陷（按严重度）

### P0 — 会系统性误诊

**P0-1 `step_time_ratio ≥ 2.0` 是"被动指标"，抓不到元凶**

真正慢的那个 rank 的 step_time 里**包含等待时间**（因为上一步的 allreduce 在等它），其他 rank 的 step_time 里**也包含等待时间**（在等它的 allreduce 完成）。结果所有 rank 的 step_time 趋同 → 用比值检测慢 rank 反而**漏掉元凶，误标受害者**。

正确做法是分解：
- `compute_time_ms`（kernel 执行，不含等待）
- `comm_wait_time_ms`（barrier/allreduce 等待，不含 on-wire）
- `comm_bus_time_ms`（NCCL 数据真正在线的时间）

元凶的特征：`compute_time` 高 + `comm_wait` 低；受害者：`compute_time` 正常 + `comm_wait` 高。
现有 schema 只有 `nccl_latency_ms`，不足以区分。

**P0-2 全局 P50 做分母，小范围故障反而看不见**

`step_time_ratio = step_time / global_p50`。如果 40% 的 rank 都慢了（机架级故障），P50 本身被拉高，这些慢 rank 的 ratio 接近 1.0 → **故障越普遍越看不见**。
应该用**同组 peer 的 P10** 或**历史 baseline P50** 做分母。

**P0-3 `slow_ratio > 0.1` 的 0.1 是魔数**

硬编码阈值在 8 卡节点上意味着"1 卡慢就报"，在 2 卡节点上意味着"永远不报"（2×0.1=0.2 < 1/2=0.5）。应按绝对 rank 数或统计显著性判。

### P1 — 不够灵敏 / 没拿到该拿的信号

- **没有时序**：单次快照触发报警会抖动。需要 rolling window + hysteresis（进入需 K 连胜、离开需 M 连败）
- **没有 per-(DP/TP/PP) group 视角**：真正的 straggler 要在 parallel group 内比。仓库没有 `parallel_group_id` 这种拓扑元信息
- **没有和 GPU 利用率交叉判据**：
  - step_time 高 + GPU 利用率低 → 外部等待（数据、通信、锁）
  - step_time 高 + GPU 利用率 100% → 纯算力不足（降频）

  干预手段完全不同，但现有告警不分
- **没有硬件 counter**：SM clock、温度、功耗是降频的 ground truth，DCGM 能采，本项目未接入
- **NCCL 带宽是 mock 的 100 Gbps**：真实部署需要 `NCCL_DEBUG=INFO` 或 `ncclLaunchKernel` 时长
- **没有 baseline**：同一 job 第二次跑时，没有"上次这个 step 花了多久"的对比

### P2 — 和 HANG 检测没联动

最可惜的一点：HANG 检测在盲抽 4 个节点采堆栈；如果 straggler 检测能给出"rank 57 疑似慢"，HANG 检测就可以**定向采样 rank 57 的堆栈**。
当前两个模块完全独立，HANG 检测只能瞎抽，straggler 检测只有硬阈值。

---

## 4. 分级设计建议（务实路线，不改代码）

### L0 — 采集补强（前提）

不补这一层，后面的算法再漂亮都是空中楼阁：

1. **拆分 step_time**：`compute_ms` / `comm_wait_ms` / `comm_bus_ms`（PyTorch 可通过 `torch.profiler` 或 CUDA event 采）
2. **拓扑元信息**：每 rank 上报 `dp_rank` / `tp_rank` / `pp_rank`
3. **引入 DCGM**：SM clock、温度、功耗、ECC 计数、XID
4. **NCCL 实测带宽**：每次 allreduce 的 size/time

### L1 — 在线统计检测（算法层）

| 算法 | 适用 | 优点 | 缺点 |
|---|---|---|---|
| **Rolling Z-score** on compute_ms | 同 group 内比较 | O(N) 简单 | 对尾部敏感度差 |
| **MAD**（median absolute deviation） | 有离群点时 | 抗污染 | 需排序 |
| **P99/P50 比** on step_time | 间歇型 | 捕捉尾延迟 | 需足够 step |
| **Page-Hinkley / CUSUM** | 检测"突变" | 在线、无需窗口 | 对渐变不灵敏 |
| **EWMA control chart** | 持续型 | 工业界标准 | λ 要调 |
| **Isolation Forest** on 多维向量 | 综合异常 | 无需先验 | 解释性差 |

**推荐组合**：per-group MAD z-score（抓元凶） + 全局 P99/P50 比（抓尾延迟） + Page-Hinkley（抓突变），三路并联，任意触发即进入 suspect。

### L2 — 归因分层（让告警可执行）

```
suspect_rank 判定:
  if gpu_util > 95 and sm_clock 明显低于 peer:  → HW_THROTTLE (热/功耗)
  elif compute_ms 高 and comm_wait_ms 低:       → COMPUTE_BOUND (元凶)
  elif comm_wait_ms 高 and compute_ms 正常:     → VICTIM (被别人拖)
  elif nccl_latency 高 and 同节点 rank 都高:     → NETWORK (链路/NIC)
  elif gpu_util 低 and compute_ms 高:           → DATA_STARVED (DataLoader/IO)
  elif ecc_errors 上涨:                         → HW_ERROR
  else:                                         → UNKNOWN → 触发定向堆栈采样
```

最后一档 UNKNOWN 正好**喂给 HANG 检测模块做定向堆栈采样**，两个模块的天然接口。

### L3 — 联动与收敛

1. **Suspect 持续 N 轮** → 降级为"已确认 straggler"，触发事件
2. 事件触发 → 落盘时序数据（最近 5 分钟所有涉事 rank 的 compute/comm/hw counter）
3. 告警分级：单 rank → warning；同 group 多 rank → critical；跨节点扩散 → page-on-call
4. 抑制策略：已知 straggler 不再重复告警（复用 HANG 的 `notified` 标记思路）

---

## 5. 容易踩的坑

1. **训练初期 step_time 本来就波动大**（warmup、autotune、CUDA graph 构建）→ 前 100 step 的 straggler 报警多半是噪声，需冷启动静默期
2. **Checkpoint/validation step 是合法的慢** → 需 step 打标签或 modulo 过滤，类似 HANG 白名单思路
3. **Pipeline parallel 里天然有 bubble** → 同 PP stage 内才可比，跨 stage 的 step_time 比较无意义
4. **"慢"是相对的** → 一个 job 的 800ms 正常，另一个 job 的 800ms 是故障。必须 per-job baseline，不能全局阈值
5. **Slow Ratio 会掩盖信息** → 8 卡节点 1 卡慢（0.125）vs 4 卡慢（0.5）故障性质完全不同，UI 应展示分布而非均值/比例
6. **不要用 GPU 利用率判 straggler** → `nvidia-smi` 的利用率只看"是否有 kernel 在跑"，不看跑多快。降频到 50% 的 GPU 利用率照样 100%。务必用 SM clock + TFLOPS

---

## 6. 和 HANG 检测的联动接口（关键设计点）

两个模块天然互补，应该互为触发器：

```
┌─────────────────────┐       suspect rank list      ┌────────────────────┐
│ Straggler 检测       │ ───────────────────────────> │ HANG 检测           │
│ (快信号, 每 step)    │                              │ (慢信号, 每 30s)   │
│                     │ <─────────────────────────── │                    │
└─────────────────────┘   hang node list (排除 UI)    └────────────────────┘
```

- Straggler → HANG：把"归因为 UNKNOWN"的 suspect rank 喂给 HANG 检测，替代现在的"随机抽 4 节点"
- HANG → Straggler：已判定 HANG 的节点从 straggler 告警里移除（避免重复噪声）

这样：
- HANG 检测的覆盖率问题（P0 级）解决 —— 不再靠 0.4% 随机抽样
- Straggler 检测的归因问题（UNKNOWN 类）解决 —— 有堆栈能看

---

## 7. 一句话结论

现有 straggler 判据处在**"硬阈值 + 单时刻快照"**阶段，能抓持续型独立 straggler，但会**系统性漏掉最重要的场景——集合通信盲区里的元凶 rank**。想做到生产级，三件事按顺序做：

1. **把 step_time 拆成 compute / comm_wait / comm_bus 并引入拓扑元信息**
2. **用 per-group MAD + CUSUM 做在线检测**
3. **用"归因标签 + UNKNOWN 触发定向堆栈采样"把 straggler 检测与 HANG 检测连成一条诊断链**

---

## 8. 评分

| 维度 | 评分 | 备注 |
|---|---|---|
| 指标采集完整度 | ⭐⭐ | 缺 compute/comm 分解、拓扑、DCGM |
| 判据算法 | ⭐⭐ | 仅硬阈值，无统计方法 |
| 时序处理 | ⭐ | 单时刻快照 |
| 归因能力 | ⭐ | 只有"慢"一个标签，不能指导处置 |
| 与 HANG 联动 | ⭐ | 完全独立 |
| UI 展示（README 描述） | ⭐⭐⭐⭐ | 热力图 + 分层下钻思路好 |
