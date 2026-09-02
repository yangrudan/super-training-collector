# Agent 学习与实践能力路线规划

> 针对 **super-training-collector**（Rust + Leptos 千卡级分布式训练监控面板）场景，
> 将「让 Agent 学会并实践」拆解为 6 个阶段、21 个关键节点，并标注重点等级。
>
> 重点图例：⭐ 一般 &nbsp;|&nbsp; 🔑 关键 &nbsp;|&nbsp; ⚠️ 高风险/卡点

---

## 总览

| 阶段 | 主题 | 目标 | 预估占比 |
|------|------|------|----------|
| 0 | 环境认知与工具链 | 能跑起来、能改、能验证 | 10% |
| 1 | 语言与框架基础 | 读懂 Rust / Leptos 代码 | 20% |
| 2 | 项目结构与数据流 | 知道改哪里、为什么 | 20% |
| 3 | 核心功能实践 | 独立完成小需求 / Bugfix | 25% |
| 4 | 测试与质量保障 | 会写测试、会跑 CI | 15% |
| 5 | 进阶：性能与架构 | 能参与设计、做优化 | 10% |

---

## Phase 0：环境认知与工具链 ⭐

**目标**：让 Agent 在本地把项目跑起来，建立「改 → 编译 → 验证」的最小闭环。

| # | 任务 | 重点 | 验收标准 |
|---|------|------|----------|
| 0.1 | 安装 Rust nightly + `wasm32-unknown-unknown` target | 🔑 | `rustup show` 包含 nightly 与 wasm32 |
| 0.2 | 安装 `cargo-leptos` | 🔑 | `cargo leptos --version` 正常输出 |
| 0.3 | `cargo leptos watch` 启动开发模式 | 🔑 | 浏览器访问 `http://127.0.0.1:3000` 能看到面板 |
| 0.4 | 修改一行 UI 文案并热重载验证 | ⭐ | 页面无刷新更新，Agent 确认变更生效 |

> ⚠️ 常见卡点：nightly 版本与 `cargo-leptos` 版本不匹配；wasm32 target 未安装导致编译失败。

---

## Phase 1：语言与框架基础 ⭐

**目标**：掌握 Rust 与 Leptos 的核心概念，能读懂项目代码。

| # | 任务 | 重点 | 验收标准 |
|---|------|------|----------|
| 1.1 | Rust 基础：所有权、生命周期、Result/Option | 🔑 | 能解释 `&str` vs `String`、`?` 运算符作用 |
| 1.2 | Leptos 响应式：`create_signal` / `create_effect` / `create_resource` | 🔑 | 能写出简单的计数器组件 |
| 1.3 | Leptos 组件与属性传递 | ⭐ | 能拆分一个复杂组件为父子组件 |
| 1.4 | Server Functions 与 SSR 概念 | 🔑 | 能区分 `#[server]` 与普通函数，理解 hydration |
| 1.5 | 阅读 `app/src/lib.rs` 与 `app/src/api.rs` | ⭐ | 能画出前端 → API → 后端的数据流图 |

> ⚠️ 高风险：混淆客户端/服务端代码边界；误用 `create_effect` 导致无限循环。

---

## Phase 2：项目结构与数据流 🔑

**目标**：建立全局认知，知道「改某个指标应该动哪些文件」。

| # | 任务 | 重点 | 验收标准 |
|---|------|------|----------|
| 2.1 | 精读 `app/src/models.rs` 核心数据结构 | 🔑 | 能说出 `HealthStatus`、`NodeInfo`、`RankMetrics` 字段含义 |
| 2.2 | 理解三级视图：Level1 → Level2 → Level3 | 🔑 | 能描述从全局热力图到单节点 Rank 详情的下钻路径 |
| 2.3 | 掌握 `app/src/adapter.rs` 数据适配层 | ⭐ | 能指出原始采集数据如何转换为前端模型 |
| 2.4 | 熟悉 `app/src/flamegraph/` 模块 | ⭐ | 能说明火焰图数据生成流程（采集 → 合并 → 渲染） |
| 2.5 | 阅读 `config/collector.json` 与环境变量 | ⭐ | 能独立调整 `RANK_ANALYSIS_ENABLED` 等开关 |

> 🔑 本阶段是后续所有实践的基础，建议通过「给某个指标加单位」等 micro-task 验证理解。

---

## Phase 3：核心功能实践 🔑

**目标**：独立完成一个小需求或 Bugfix，走完「需求 → 开发 → 验证」全流程。

| # | 实践任务（由易到难） | 重点 | 涉及文件示例 |
|---|----------------------|------|--------------|
| 3.1 | 给 Level2 节点列表增加「GPU 利用率排序」 | ⭐ | `app/src/components/level2.rs` |
| 3.2 | 在 Level3 增加 Step Time 趋势图（Mock 数据） | 🔑 | `app/src/components/level3.rs`, `mock.rs` |
| 3.3 | 修复 HANG 检测误报：调整堆栈相似度阈值 | 🔑 | `app/src/flamegraph/`, `server/` |
| 3.4 | 实现「问题 Rank 分析」开关的前端控制 | ⭐ | `app/src/components/common.rs`, `api.rs` |
| 3.5 | 新增一个健康状态指标（如网络带宽） | ⚠️ | `models.rs`, `adapter.rs`, `level1.rs` |

> 🔑 每个任务必须包含：Mock 数据验证 → 真实数据联调 → E2E 测试通过。

---

## Phase 4：测试与质量保障 ⭐

**目标**：让 Agent 养成「先写测试、再写实现」的习惯，能通过 CI。

| # | 任务 | 重点 | 验收标准 |
|---|------|------|----------|
| 4.1 | 单元测试：Rust 原生 `#[test]` | ⭐ | 新增模块覆盖率 ≥ 80% |
| 4.2 | E2E 测试：Playwright 脚本 | 🔑 | `cargo leptos end-to-end` 通过 |
| 4.3 | 编译检查：`cargo check` / `cargo clippy` | 🔑 | 无 warning（或解释豁免理由） |
| 4.4 | 构建验证：`cargo leptos build --release` | ⭐ | 生成 `target/release/server` 与 `target/site/pkg/` |
| 4.5 | deb 打包验证：`./scripts/build-deb.sh` | ⭐ | 生成 `.deb` 包并能安装启动 |

> ⚠️ 卡点：E2E 测试在 CI 环境不稳定；deb 打包需区分「生产仅打包」与「开发带编译」模式。

---

## Phase 5：进阶 —— 性能与架构 🔑

**目标**：从「实现功能」升级到「优化体验、参与设计」。

| # | 方向 | 重点 | 产出示例 |
|---|------|------|----------|
| 5.1 | 前端性能：WASM 体积优化、虚拟滚动 | 🔑 | Level2 节点列表 1000+ 节点流畅滚动 |
| 5.2 | 后端性能：采集频率与内存占用平衡 | ⭐ | 火焰图采集不阻塞主训练流程 |
| 5.3 | 架构设计：插件化指标采集器 | ⚠️ | 新增指标无需改核心代码，通过配置注册 |
| 5.4 | 可观测性：自身监控与告警 | ⭐ | Collector 自身健康状态暴露 `/healthz` |
| 5.5 | 文档与交接：更新 `docs/` 与 README | ⭐ | 新同事 30 分钟能跑通开发环境 |

---

## 实践建议

1. **小步快跑**：每个 Phase 结束必须有一个可演示的 Demo 或可合并的 PR。
2. **Mock 优先**：在 `app/src/mock.rs` 中构造极端数据（如 1000 节点、全 Critical），提前暴露 UI 问题。
3. **结对评审**：涉及 `flamegraph/` 与 HANG 检测的修改，必须人工 review 算法逻辑。
4. **记录卡点**：在 `docs/` 新增 `troubleshooting.md`，沉淀 Agent 遇到的典型编译/运行时错误。

---

## 附录：快速参考

| 常用命令 | 说明 |
|----------|------|
| `cargo leptos watch` | 开发模式（热重载） |
| `cargo leptos build --release` | 生产构建 |
| `cargo leptos end-to-end` | E2E 测试 |
| `./scripts/build-deb.sh` | deb 打包（生产） |
| `./scripts/build-deb.sh --with-build` | deb 打包（开发，含编译） |

> 最后更新：2026-09-02
