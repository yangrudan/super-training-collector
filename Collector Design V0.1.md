> <font style="color:rgb(31, 35, 40);">[目标]: 让研发/运维快速定位每个节点（rank）状态、堆栈信息、性能瓶颈、异常趋势与故障原因，支持实时监控、告警与数据分析, 最后实现根因排查。</font>
>

# 架构设计
<!-- 这是一个文本绘图，源码为：graph LR
    subgraph Training
        A[Rank 0 - probing] -->|POST /metrics/rank=0| B(Adapter Layer)
        C[Rank 1 - probing] -->|POST /metrics/rank=1| B
        D[Rank N - probing] -->|POST /metrics/rank=N| B
    end
    B --> E[Axum Routes]
    E --> F[Leptos Dashboard]
    B --> G[Storage Layer] -->
![](https://cdn.nlark.com/yuque/__mermaid_v3/2f59f51eb6e7a302cb435627e8e1575b.svg)

# UI设计
+ <font style="color:rgb(31, 35, 40);">健康颜色：绿/黄/红（consistent，避免过多色彩）</font>
+ <font style="color:rgb(31, 35, 40);">对比度好、卡片信息层级清晰（KPI > 热图 > 表格 > 详情图）</font>

# <font style="color:rgb(31, 35, 40);">分析与告警</font>
<font style="color:rgb(31, 35, 40);">告警规则建议（MVP）</font>

+ <font style="color:rgb(31, 35, 40);">节点心跳缺失（last_seen 超过阈值） -> CRIT</font>
+ <font style="color:rgb(31, 35, 40);">内存使用 > 95% 持续 3 分钟 -> CRIT</font>
+ <font style="color:rgb(31, 35, 40);">GPU 利用过低且 training step 停滞（可能卡住） -> WARN</font>
+ <font style="color:rgb(31, 35, 40);">comm_metrics 的 p95 > 指定阈值 或 error_rate 增加 -> CRIT</font>

# 技术栈
RUST (leptos + axum)

    - Leptos 支持 SSR/CSR 混合渲染，天然适配「聚合数据轻量渲染+下钻细节按需加载」的模式；
    - 内置响应式系统，可高效绑定热力图、聚合指标的动态更新；
    - 与 Rust 后端无缝衔接，无需跨语言序列化/反序列化（直接复用 `Node`/`Topology` 等结构体）
    - 可视化：`d3-rs`（热力图/条形图）+ `thaw`（Rust 组件库，替代 React 生态）；
    - 状态管理：Leptos `create_resource`/`create_signal`（轻量且适配分布式数据更新）；
    - 网络：`reqwest`（后端）+ `leptos_axum`（前后端一体化）。

# 开发阶段
[parse1] 搭建leptos框架, 实现「**后端预聚合+前端热力编码+按需下钻**」的三级视图：

+ Level 1：全局聚合，聚焦“是否正常”；
+ Level 2：节点聚合，聚焦“问题在哪”；
+ Level 3：rank 细节，聚焦“问题根因”；

[parse2] 开发adapter层, 接入probing正式的真实数据, 解决并发和性能问题;

[parse3] 数据分析模块开发和告警, 决策;

[parse4] OTLP expoter迁移升级.

+ <font style="color:rgb(101, 123, 131);">集成 OpenTelemetry Collector</font>

<!-- 这是一个文本绘图，源码为：graph LR
    A[Rank 0 probing] -->|OTLP/HTTP| B[Axum OTLP Receiver]
    A -->|OTLP/HTTP| C[Rank N probing]
    C --> B
    B --> D[OpenTelemetry Collector]
    D --> E[In-Memory Storage]
    D --> F[Prometheus]
    D --> G[Dashboard UI] -->
![](https://cdn.nlark.com/yuque/__mermaid_v3/5a90ad8baa918359839a1ecf435d3aec.svg)

