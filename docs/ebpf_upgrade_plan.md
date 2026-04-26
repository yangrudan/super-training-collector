# eBPF / uprobe HANG 检测升级实施计划

> 目标: 把现有基于 Jaccard 的 HANG 检测升级为基于 eBPF/uprobe 的事件驱动检测，并为后续故障归因（straggler、NCCL、CUDA API 分解）打底
> 状态: 待实施
> 配套评审: `plan.md`（HANG 检测）、`straggler_review.md`

---

## 0. 目标与非目标

### 目标 (In Scope)

1. **零开销常开**：检测器不再周期性主动采样，事件驱动，无故障时 CPU 占用接近 0
2. **更准的 HANG 判据**：从"堆栈集合 Jaccard"改为"线程长时间阻塞 + 现场堆栈"
3. **更细的归因数据**：CUDA API、NCCL API 的 entry/exit 时间戳作为 straggler 检测和 step_time 分解的来源
4. **和现有 collector 解耦**：eBPF agent 作为独立组件，通过 HTTP/SQL 喂数据进 collector，不与现有 Leptos 服务耦合
5. **平滑迁移**：新旧检测器可并行运行一段时间，灰度切换

### 非目标 (Out of Scope)

- GPU kernel 内部性能（那是 CUPTI/Nsight 的事）
- 完整的 Python 栈 unwind（第一阶段先抓 C 栈；Python 栈通过 probing 配合）
- 替换 probing —— probing 继续承担业务侧（loss/step/comm_metrics）
- 替换 DCGM —— 硬件 counter 仍由 DCGM 提供

---

## 1. 总体架构

```
┌──────────────────────────────────────────────────────────────┐
│ Training Node                                                 │
│                                                                │
│  ┌─────────────────┐    ┌──────────────────────────────────┐  │
│  │ Training Process│    │ stc-probe (新增, eBPF agent)       │  │
│  │  + probing      │    │                                   │  │
│  │  agent          │    │  - aya 加载 eBPF 程序              │  │
│  │                 │    │  - uprobe: futex / pthread_cond  │  │
│  │  暴露 :9933     │    │  - uprobe: libcuda/libnccl        │  │
│  │  callstack/SQL  │    │  - ringbuf 收事件                 │  │
│  │                 │    │  - 本地聚合 + HTTP 暴露 :9934      │  │
│  └────────┬────────┘    └─────────────┬────────────────────┘  │
│           │                            │                        │
└───────────┼────────────────────────────┼───────────────────────┘
            │                            │
            ↓                            ↓
   ┌──────────────────────────────────────────────────────────┐
   │ super-trainning-collector (master)                         │
   │                                                            │
   │  - 订阅 :9933 (堆栈) 和 :9934 (eBPF 事件)                  │
   │  - 规则引擎: 长 wait + 跨 rank 关联 = HANG 事件             │
   │  - 取代现 hang_detector::runner 的 Jaccard 路径             │
   │  - UI 展示                                                 │
   └──────────────────────────────────────────────────────────┘
```

**关键设计原则**：

- **stc-probe 是独立二进制**（DaemonSet / systemd unit），失败不影响训练
- **collector 是被动消费者**，不需要权限（CAP_BPF 在 stc-probe 一侧）
- **新旧通路并存**：新通路 = stc-probe 上报事件；旧通路 = `hang_detector` Jaccard。两者结果都进 UI，运行 1-2 周后下线旧通路

---

## 2. 技术栈决定

| 维度 | 选择 | 备选 | 理由 |
|---|---|---|---|
| eBPF 框架 | **aya (Rust)** | libbpf-rs / BCC | 纯 Rust，与 collector 技术栈一致；CO-RE 支持；无 LLVM runtime 依赖 |
| 用户态语言 | **Rust** | Go | 同上 |
| 数据通路 | **ringbuf** | perf event array | 现代内核首选，更高效，单消费者 |
| 跨节点协议 | **HTTP + JSON** (MVP) → **HTTP + Arrow** (后期) | gRPC | 和现有 callstack endpoint 一致，UI/curl 友好 |
| 本地存储 | **环形内存表** (MVP) → **DataFusion / DuckDB** (后期) | sqlite | 评估和 probing 对齐 |
| 部署 | **K8s DaemonSet** (生产) / **systemd** (开发机) | sidecar container | DaemonSet 一节点一份，符合 eBPF 模型 |
| Rust 仓位置 | **本仓 workspace 新增 crate** `stc-probe` | 独立仓 | 共享 models/types，迭代快 |

**aya vs libbpf-rs**：aya 在 2024 后已成熟，CO-RE、ringbuf、uprobe 都支持完备，社区活跃；libbpf-rs 更接近 C 生态但要 bindgen。给 Rust 项目用 aya 是更顺的选择。

---

## 2.5 uprobe vs eBPF —— 不是二选一

> 这一节单独写出来防止理解偏差。**uprobe 不是 eBPF 的替代品，它是 eBPF 的一种挂载点（attach type）。**

### 概念关系

```
            ┌──────────── eBPF（框架/运行时）────────────┐
            │                                              │
            │   程序类型(挂在哪)        Map / 通信            │
            │   ─────────────          ──────────             │
            │   • kprobe / kretprobe   • hash map           │
            │   • uprobe / uretprobe   • array map          │
            │   • tracepoint           • ringbuf            │
            │   • perf_event           • perf event array   │
            │   • XDP / TC / LSM ...   • stack trace map    │
            └──────────────────────────────────────────────┘

uprobe   ⊂   eBPF 的 attach type
```

### 两条候选路线

| 路线 | 实现 | 何时合适 |
|---|---|---|
| **A. 纯 uprobe（不走 eBPF）** | `perf_event_open` + uprobe，事件直接送回用户态处理 | 只 hook 极少数低频函数，且不需要内核侧信号；适合一次性诊断脚本 |
| **B. eBPF 框架 + uprobe / kprobe / tracepoint（本计划采用）** | aya 加载，多种 attach 类型混合，内核态 map 聚合 | 高频点 + 跨进程聚合 + 需要内核侧根因（futex / 网络 / 调度） |

**为什么本项目必须选 B**：

1. **`cuLaunchKernel` 调用频率极高**（每秒上万次） —— 纯 uprobe 全量回用户态会直接 OOM；eBPF map 在内核态做"超阈值才上报"才扛得住
2. **HANG 根因常在内核侧** —— 卡在 `futex_wait` / TCP 重传 / D state，纯 uprobe 看不到内核栈
3. **跨 rank/进程聚合** —— eBPF map 天然跨进程共享，纯 uprobe 要自己在用户态合并

### Attach 点矩阵（贯穿 Phase 1–4）

下表是本计划要挂的所有探针。**uprobe 抓用户态库 API，kprobe / tracepoint 抓内核侧根因**，两类同时用，都是 eBPF 程序。

| 检测目标 | Attach Type | 具体挂载点 | 用途 / 输出表 | 所属 Phase |
|---|---|---|---|---|
| 线程长等待（HANG 主信号） | **uprobe** | `libpthread.so:pthread_cond_wait` entry/return | 等待时长 + 用户栈 → `stuck_threads` | P1 |
| futex 等待（覆盖未走 pthread 的等待） | **kprobe** / tracepoint | `syscalls:sys_enter_futex` / `sys_exit_futex` | 同上，作为兜底 | P1 |
| Python 栈快照（HANG 现场可读性） | **uprobe** | `libpython3.X.so:_PyEval_EvalFrameDefault` *(可选，路 B)* | Python 调用链 | P2 路 B |
| Python 栈（推荐路径） | **HTTP 回调** | 不挂 eBPF，调 probing `python.backtrace` | 同上 | P2 路 A |
| Kernel launch CPU 侧耗时 | **uprobe** | `libcudart.so:cudaLaunchKernel` entry/return | `cuda_api_calls(api='cudaLaunchKernel')` | P3 |
| GPU 等待（CPU 侧 stall） | **uprobe** | `libcuda.so:cuStreamSynchronize` entry/return | `cuda_api_calls(api='cuStreamSynchronize')` | P3 |
| Event 等待 | **uprobe** | `libcuda.so:cuEventSynchronize` | 同上 | P3 |
| H2D / D2H 传输 | **uprobe** | `libcudart.so:cudaMemcpyAsync` | `cuda_api_calls(api='cudaMemcpyAsync')` | P3 |
| 集合通信入队耗时 | **uprobe** | `libnccl.so:ncclAllReduce` / `ncclBroadcast` ... | `nccl_ops(op, comm_id, count, dtype, duration_ns)` | P3 |
| 点对点通信 | **uprobe** | `libnccl.so:ncclSend` / `ncclRecv` | 同上 | P3 |
| 网络抖动 | **tracepoint** | `tcp:tcp_retransmit_skb` | `tcp_retrans(ts, saddr, daddr)` | P4 |
| CPU 抢占 / 多租户干扰 | **tracepoint** | `sched:sched_switch` (filter: prev_state=R) | `sched_contention(pid, cpu, runtime_ns)` | P4 |
| 主机内存压力 | **tracepoint** | `exceptions:page_fault_user` + `kmem:mm_compaction_*` | `mem_pressure` | P4 |
| Checkpoint IO 长尾 | **tracepoint** | `block:block_rq_issue` / `block_rq_complete` | `block_io(dev, bytes, latency_ms)` | P4 |
| On-CPU 火焰图（按需） | **perf_event** | 99 Hz 采样 | 火焰图原始数据 | P4 (可选) |

**几个工程决策**：

- **uprobe 入口要 + 返回 entry/uretprobe 配对**，才能算 duration；只挂入口的话只能算频率
- **uprobe attach 用 glob 模式**（aya 支持 `Uprobe::attach(target="libnccl.so", symbol="ncclAllReduce")`），不要写死绝对路径——容器镜像、conda 环境路径都不一样
- **tracepoint 优先于 kprobe**：tracepoint 是稳定 ABI，kprobe 跨内核版本可能改函数名；只有 tracepoint 不存在时才退回 kprobe
- **高频 uprobe 必须有"全局开关 + 阈值过滤"**：`cuLaunchKernel` / `cudaMemcpyAsync` 默认只记录 duration > 1ms 的事件，避免事件爆炸（在 eBPF 程序里就做过滤，到不了用户态）
- **栈深统一限制 32 帧**：`bpf_get_stack` 默认上限就是 32，再深也意义不大

### 一句话总结

> 选的不是 uprobe 还是 eBPF，而是**用 eBPF 框架 + uprobe (用户态库 API) + kprobe/tracepoint (内核侧根因)** 的组合。aya 一个程序里同时挂这几类。

---

## 3. 分阶段实施

### Phase 0 — 环境与可行性验证（先行）

目标：在动手写代码前，先把所有"环境是否支持"的不确定性消掉。

todos:
- `p0-kernel-version`: 调研所有目标训练节点的内核版本，确认 ≥ 5.8（CO-RE 友好）；< 5.4 的节点要排除或单独处理
- `p0-cap`: 确认容器运行时是否能给 `CAP_BPF` / `CAP_PERFMON`，K8s 场景确认是否能跑 privileged DaemonSet
- `p0-libpath`: 在样本节点上 `ldd` 训练进程，记录 `libcuda.so.1` / `libcudart.so.X` / `libnccl.so.X` 的真实路径和版本
- `p0-symbol`: 确认这几个 .so 是否带符号；`readelf -s libcudart.so.12 | grep cuLaunchKernel` 等
- `p0-aya-poc`: 写一个 200 行的 PoC，对 `pthread_cond_wait` 挂 uprobe，在测试机上跑 `python -c 'import threading; threading.Event().wait()'`，验证能抓到事件和 C 堆栈

**门槛**：上述全部 ✅ 才进入 Phase 1。任一项 ❌ 要先解决（换内核、申请权限、调整路径策略）

### Phase 1 — MVP：futex/cond_wait 长等检测（核心 HANG 替代）

目标：交付一个能直接替代 Jaccard 的 HANG 检测器。

**eBPF 程序**（在 stc-probe 内，挂载点见 §2.5 矩阵）:
```
uprobe  @ libpthread.so:pthread_cond_wait entry  → map.insert(tid, t_enter)
uretprobe @ libpthread.so:pthread_cond_wait      → wait_ms = now - t_enter
                                                   if wait_ms > THRESHOLD (默认 30s):
                                                     bpf_get_stack(BPF_F_USER_STACK)
                                                     ringbuf.push(event)

tracepoint @ syscalls:sys_enter_futex            → map.insert(tid, t_enter)   // 兜底
tracepoint @ syscalls:sys_exit_futex             → 同上算 wait_ms 并上报
```

两类挂载并存：uprobe 抓走 pthread API 的 case（绝大多数 PyTorch 场景），futex tracepoint 覆盖未走 pthread API 的原生等待。**入口和返回都要配对**，单挂入口算不出 duration。

**用户态 stc-probe**:
- 消费 ringbuf
- 维护 per-tid 状态机：`Active → Waiting(t_start) → Stuck(stack)`
- 暴露 HTTP `/api/probe/stuck_threads`，返回当前所有 wait > N 秒的线程及其堆栈
- 暴露 `/api/probe/health` 给 collector 探活

**collector 侧改动**:
- 新增 `app/src/hang_detector_v2/` 模块（或 `app/src/probe_client.rs`）
- 启动时如果检测到节点上 stc-probe 可用（探 :9934）→ 走新通路
- 新通路逻辑：
  - 每 5 秒 poll 各节点 `stuck_threads`
  - 一个节点 ≥ 50% rank 的主线程 stuck → 节点级 HANG
  - 投票判定全局（沿用现有 50% 阈值）
- 告警走现有 notifier
- 落盘走现有 logger（格式扩展，加 `detection_method: "ebpf_futex"` 字段）

**验证**:
- 写一个故意 HANG 的 PyTorch 脚本（一个 rank 在 barrier 前 sleep）
- 验证 stc-probe 能在 35 秒内识别（30s 阈值 + 5s polling）
- 验证 collector 收到事件并落盘
- 对比 Jaccard 通路：新通路应**更早识别 + 不漏报元凶**

todos:
- `p1-aya-skeleton`: aya 项目骨架，含 CI、cross-build (musl/gnu)
- `p1-bpf-futex`: futex/cond_wait uprobe 程序，过滤短 wait
- `p1-stack-capture`: bpf_get_stack 抓 user stack（C 帧），栈深 32
- `p1-userspace-agent`: ringbuf 消费 + 状态机 + HTTP server
- `p1-symbol-resolve`: 把堆栈 IP 解析成函数名（读 /proc/PID/maps + procfs）
- `p1-collector-client`: collector 侧的 stc-probe 客户端
- `p1-collector-v2-detector`: 新 HANG 检测器，灰度开关 `HANG_DETECTOR_V2=true`
- `p1-e2e-test`: 端到端故意 HANG 脚本验证

**Phase 1 完成 = 能在生产替代 Jaccard 的 80% 场景**。

### Phase 2 — Python 栈联动（HANG 现场更可读）

C 栈对工程师有帮助但不够。HANG 现场最有用的是 Python 调用链。

**两条路二选一**：

**路 A（推荐，便宜）**：stc-probe 检测到 stuck 时，**回调 probing 拉一次 `python.backtrace`**
- 实现：stc-probe 收到 stuck 事件 → HTTP 调本节点 probing 的 SQL endpoint：`SELECT * FROM python.backtrace WHERE pid = ? LIMIT 1`
- 优点：不用自己写 Python unwinder
- 缺点：需要 probing 已经在该进程上 attach

**路 B（重投入）**：在 eBPF 里直接 walk Python 栈
- 参考 py-spy / Parca 的实现，需要识别 `_PyEval_EvalFrameDefault` 帧并手动 unwind PyFrameObject 链
- Rust 侧大概 1500 行
- 优点：零依赖
- 缺点：跨 Python 版本兼容性差，自己维护 PyFrameObject 偏移表

**建议先做路 A**，后期如果 probing 不可用再考虑路 B。

todos:
- `p2-probing-bridge`: stc-probe 调 probing SQL 的客户端
- `p2-fused-stack`: 合并 C 栈（eBPF）+ Python 栈（probing）成一份完整堆栈
- `p2-flamegraph`: 复用现有 flamegraph 模块生成 SVG

### Phase 3 — CUDA / NCCL API tracing（straggler 数据底座）

挂 uprobe 到关键 API：

| 库 | API | 用途 |
|---|---|---|
| libcudart | cudaLaunchKernel | kernel 发射频率、CPU 侧 launch latency |
| libcuda | cuStreamSynchronize | CPU 等 GPU 时间 |
| libcuda | cuEventSynchronize | event 等待 |
| libcudart | cudaMemcpyAsync | PCIe 传输 |
| libnccl | ncclAllReduce | 集合通信真实耗时 |
| libnccl | ncclSend / ncclRecv | 点对点 |

**输出**：
- 新 SQL 表（在 stc-probe 内存里）：
  - `cuda_api_calls(ts, pid, tid, api, duration_ns, [args])`
  - `nccl_ops(ts, pid, tid, op, comm_id, count, dtype, duration_ns)`
- 暴露为 HTTP `/api/probe/sql?q=...`（DataFusion 接管）
- collector 拉这些表算 `compute_ms` / `comm_wait_ms` / `comm_bus_ms`

**注意点**:
- `ncclAllReduce` 是异步入队，duration 反映的是 host 侧 enqueue 时间，不是 on-wire 时间。要拿真实 on-wire，需要 hook `ncclLaunchKernel` 或 stream sync 上的对应 event。先做 enqueue 时间，后续优化
- `cuLaunchKernel` 第一参数是 function ptr，不知道 kernel name。先记 ptr，后期通过 CUPTI 或离线 symbol 表 correlate

todos:
- `p3-cuda-uprobes`: libcuda/libcudart uprobe 套件
- `p3-nccl-uprobes`: libnccl uprobe 套件
- `p3-sql-store`: 用 DataFusion 在 stc-probe 内存里建表，环形保留最近 N 分钟
- `p3-sql-endpoint`: HTTP SQL endpoint
- `p3-step-decompose`: collector 侧用 SQL 查询计算 compute/comm_wait/comm_bus 替代 mock 的 step_time

**Phase 3 完成 = straggler 检测有了真实数据，可以写归因规则**

### Phase 4 — OS 层侧信号（straggler 归因证据）

补充 eBPF 程序：
- `tcp_retransmit` tracepoint → 网络抖动
- `sched_switch` → CPU 抢占（多租户嫌疑）
- `page_fault_user` + THP compaction → host 内存压力
- block I/O (`block_rq_issue` / `block_rq_complete`) → checkpoint IO 长尾

每个对应一张 SQL 表。collector 写规则匹配："最近 60s tcp_retransmit > 100 + 该节点 nccl_latency 上涨" → 标签 `NETWORK_ANOMALY`。

todos:
- `p4-tcp-retrans`
- `p4-sched-contention`
- `p4-mem-pressure`
- `p4-block-io`
- `p4-attribution-rules`: 归因规则 YAML 化

### Phase 5 — 旧 Jaccard 通路下线 + 生产硬化

todos:
- `p5-shadow-comparison`: 新旧通路并行运行 2 周，对比报警一致性
- `p5-deprecate-jaccard`: 删除 `app/src/hang_detector/`（保留模块入口给 fallback），改名 `hang_detector_legacy`
- `p5-systemd-unit` / `p5-k8s-daemonset`: 部署 manifest
- `p5-prometheus-metrics`: stc-probe 暴露 /metrics（事件率、ringbuf drop 率、uprobe miss 率）
- `p5-overhead-bench`: 在 4 卡 8 卡 64 卡集群分别测 stc-probe 开销，目标 < 0.5%
- `p5-secret-cleanup`: 顺手把现有钉钉 webhook token 移到环境变量

---

## 4. 风险与未决问题

### 高风险

| 风险 | 缓解 |
|---|---|
| 内核版本异质 → 部分节点跑不了 eBPF | Phase 0 先调研；< 5.4 节点保留 Jaccard fallback |
| 容器拿不到 CAP_BPF | 走 host-level DaemonSet，不在训练 pod 里 |
| libcuda/libnccl 路径每个镜像都不一样 | uprobe 用 `glob` 模式 attach，启动时扫描 `/proc/*/maps` 找 .so |
| 符号被 strip | nccl 通常带符号；libcuda 部分版本要装 debug 包；准备 fallback 用 IP+offset |
| Python 栈 unwind 复杂度 | Phase 2 先走"调 probing 拉栈"路径，避开自己写 unwinder |

### 中风险

- ringbuf 在事件爆发时 drop → 监控 drop counter，必要时降采样
- uprobe 对 hot path API（如 `cudaLaunchKernel`）开销不可忽视 → 必须有"全局开关 + 低频采样"模式
- 多 Python 进程 / fork 后 attach 时机 → 用 `bpf_lsm` 或定期重扫 PID 列表

### 待用户确认（影响计划细节）

1. **部署形态**：K8s DaemonSet 还是 bare metal systemd？影响 Phase 0 / Phase 5 的 manifest 工作量
2. **stc-probe 仓位置**：本仓 workspace 加 crate vs 单独开仓？影响 CI 和发布流程
3. **节点内核版本下限**：能否要求生产环境 ≥ 5.8？影响是否需要写老内核 fallback
4. **probing 是否一定在每个训练进程上**？影响 Phase 2 的路径选择

---

## 5. 迁移策略（旧 Jaccard → eBPF）

**不要直接删旧代码**，按以下顺序：

```
Week 0-2  Phase 0 + Phase 1 开发
Week 3    Phase 1 联调, 在 1-2 个测试 job 上灰度
Week 4-5  shadow mode: 新旧并行, 报警都进 UI 但只发钉钉旧的
Week 6    切换 primary: 钉钉发新的, 旧的降级为 secondary check
Week 7-8  Phase 2/3 开发
Week 9    旧通路只保留 fallback 路径 (节点 < 5.4 时启用)
Week 10+  Phase 4/5
```

每周输出对比报告：新通路 vs 旧通路 的 TP/FP/FN 对照。

---

## 6. 验收标准

### Phase 1 验收

- [ ] 故意 HANG 脚本能在 35s 内被识别（vs 旧的 ≥ 90s）
- [ ] 单 rank HANG（其他正常）也能识别（旧的会漏报）
- [ ] 节点 stc-probe 静态开销 < 0.1% CPU（无故障时）
- [ ] 故障时 ringbuf 无 drop（千卡规模事件率应 < 1k/s）
- [ ] HTTP API 兼容现 hang_status JSON schema

### Phase 3 验收

- [ ] 能给出每 rank 的 compute_ms / comm_wait_ms / comm_bus_ms 时序
- [ ] step_time 拆解后三段相加 ≈ wallclock step time（误差 < 5%）
- [ ] 在 64 卡训练上跑 1 小时，stc-probe 开销 < 0.5%

### Phase 5 验收

- [ ] 旧 Jaccard 通路代码移除（保留模块名给 fallback）
- [ ] K8s DaemonSet manifest 通过安全 review
- [ ] 文档完整：部署、故障排查、添加新 uprobe 的 howto

---

## 7. 不在本计划但建议同步推进

这些虽然不属于 eBPF 升级，但和这条线强相关，做了能放大本计划价值：

1. 把 `notifier.rs` 的钉钉 token 移到环境变量
2. 把 collector 的 mock straggler 数据替换为查 probing/stc-probe 的真实数据
3. 引入 parallel group 元信息（dp/tp/pp rank），UI 按 group 展示
4. 训练脚本里加 `optimizer_step_post_hook` 上报 step（Phase 1 完成后做，作为 liveness ground truth 的补充）

---

## 8. 一句话总结

**Phase 0 验环境 → Phase 1 用 futex/cond_wait uprobe 直接替代 Jaccard（最高 ROI）→ Phase 2 接 Python 栈 → Phase 3 上 CUDA/NCCL 数据底座 → Phase 4 OS 侧信号补归因 → Phase 5 旧通路下线**。技术栈用 aya + Rust，部署形态独立 DaemonSet/systemd，与 collector 解耦。
