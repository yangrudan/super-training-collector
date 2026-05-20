# Super Training Collector

基于 Rust + Leptos 构建的千卡级分布式训练任务 Web 监控面板。

---

## 目录

- [功能特性](#功能特性)
- [视图层级](#视图层级)
- [技术栈](#技术栈)
- [环境要求](#环境要求)
- [快速开始](#快速开始)
- [部署指南](#部署指南)
- [ECS 汇聚服务器](#ecs-汇聚服务器)
- [项目结构](#项目结构)
- [配置说明](#配置说明)
- [数据模型](#数据模型)
- [License](#license)

---

## 功能特性

- **分层视图**：采用「全局 → 节点 → Rank」三级下钻架构
- **热力编码**：通过颜色直观展示健康状态（绿色正常/黄色警告/红色故障）
- **实时监控**：追踪 Step Time、GPU 利用率、NCCL 延迟等关键指标
- **高效定位**：聚焦问题节点，快速定位千卡规模下的性能瓶颈
- **火焰图分析**：支持调用栈采集与火焰图可视化
- **HANG 检测**：通过堆栈相似度连续采样自动判断训练是否 HANG
- **问题 Rank 分析**：基于 StackTrie 分叉检测算法，自动识别偏离多数执行路径的异常 Rank
- **多集群汇聚**：支持多台 Collector 向统一 ECS 汇聚服务器推送数据，集中可视化

---

## 视图层级

| 层级    | 视图     | 描述                           |
| ------- | -------- | ------------------------------ |
| Level 1 | 全局态势 | 健康分布、全局 KPI、拓扑热力图 |
| Level 2 | 节点列表 | 节点性能表格，支持排序/筛选    |
| Level 3 | 节点详情 | 单节点 Rank 级别指标详情       |

---

## 技术栈

| 类别   | 技术                                               |
| ------ | -------------------------------------------------- |
| 框架   | [Leptos](https://leptos.dev/) 0.8 (全栈 Rust Web)  |
| 后端   | Axum 0.8                                           |
| 前端   | WebAssembly (`wasm32-unknown-unknown`)             |
| 样式   | SCSS                                               |
| 测试   | Playwright (E2E)                                   |

![技术栈](./docs/LeptosWebUIInteraction.png)

---

## 环境要求

- Rust nightly toolchain
- `wasm32-unknown-unknown` target
- `cargo-leptos` CLI 工具

---

## 快速开始

### 1. 安装依赖

```bash
# 安装 Rust nightly
rustup toolchain install nightly
rustup default nightly

# 添加 WebAssembly target
rustup target add wasm32-unknown-unknown

# 安装 cargo-leptos
cargo install cargo-leptos
```

### 2. 开发模式

```bash
cargo leptos watch
```

访问 http://127.0.0.1:3000

### 3. 生产构建

```bash
cargo leptos build --release
```

---

## 部署指南

### 构建产物

生产构建后，主要产物位于：

```
target/
├── release/
│   └── server              # 服务器二进制文件
└── site/
    └── pkg/                # 前端静态资源 (JS/WASM/CSS)
```

### 部署步骤

1. **准备部署目录**

```bash
mkdir -p /opt/collector
cp target/release/server /opt/collector/
cp -r target/site /opt/collector/
cp run_server.sh /opt/collector/
```

2. **目录结构**

```
/opt/collector/
├── server                  # 可执行文件
├── run_server.sh           # 启动脚本
└── target/
    └── site/
        └── pkg/            # 静态资源
```

3. **启动服务**

```bash
cd /opt/collector
chmod +x run_server.sh
./run_server.sh
```

### 环境变量

| 变量名             | 默认值            | 说明                     |
| ------------------ | ----------------- | ------------------------ |
| `LEPTOS_SITE_ROOT` | `target/site`     | 静态资源目录（相对路径） |
| `LEPTOS_SITE_ADDR` | `127.0.0.1:3000`  | 服务监听地址             |
| `LEPTOS_ENV`       | `DEV`             | 运行环境 (`DEV`/`PROD`)  |
| `RANK_ANALYSIS_ENABLED` | `true`       | 是否启用问题 Rank 分析   |
| `RANK_ANALYSIS_MINORITY_THRESHOLD` | `0.3` | 少数派阈值 (0.05-0.5)，低于此覆盖率的分支视为异常 |
| `PUSH_TARGET_URL`  | _(空，禁用)_      | ECS 推送端点，设置后启用推送（例：`http://ecs-host:4000/push`） |
| `PUSH_INTERVAL_SECS` | `30`           | 推送间隔（秒），最小 10  |
| `JOB_ID`           | _(空)_            | 训练任务 ID，ECS 使用它作为任务标识；为空时 ECS 自动分配未命名任务 |

### 手动启动

```bash
cd /opt/collector
export LEPTOS_SITE_ROOT="target/site"
export LEPTOS_SITE_ADDR="0.0.0.0:3000"
export LEPTOS_ENV="PROD"
./server
```

### deb打包和发布

```bash
# 生产环境（安全）- 仅打包已有产物
./scripts/build-deb.sh

# 开发机器 - 编译 + 打包（需确认）
./scripts/build-deb.sh --with-build
```

增加环境变量
```
sudo systemctl edit super-training-collector
```
在打开的编辑器中添加：
```bash
[Service]
Environment=MASTER_ADDR=192.168.1.100
Environment=OTHER_VAR=value

# 保存后重启服务：

sudo systemctl restart super-training-collector
```

### 注意事项

- `LEPTOS_SITE_ROOT` 必须指向包含 `pkg/` 的目录，路径相对于二进制运行目录
- 生产环境建议将 `LEPTOS_SITE_ADDR` 设为 `0.0.0.0:3000` 以允许外部访问
- 可通过 Nginx 反向代理实现 HTTPS 和负载均衡

---

## ECS 汇聚服务器

`ecs-server/` 是独立的汇聚展示程序，用于接收多台 Collector 推送的数据并集中可视化。

### 架构

```
Collector A (3000) ─┐
Collector B (3000) ──► ECS Server (4000) ──► Web 仪表盘
Collector C (3000) ─┘
```

每台 Collector 每隔 `PUSH_INTERVAL_SECS` 秒通过 HTTP POST 将指标数据推送到 ECS，ECS 使用 `JOB_ID` 作为任务标识并保留每个任务的最新快照。未提供 `JOB_ID` 时，ECS 按来源 IP 自动分配 `未命名任务1`、`未命名任务2` 等名称。

### 快速启动 ECS 服务器

```bash
cd ecs-server
cargo build --release
ECS_ADDR=0.0.0.0:4000 ./target/release/ecs-server
```

访问 `http://<ecs-host>:4000` 查看所有 Collector 的汇总仪表盘。

### 配置 Collector 推送

在每台 Collector 上设置以下环境变量（或写入 `config/collector.json`）：

```bash
export PUSH_TARGET_URL="http://<ecs-host>:4000/push"
export PUSH_INTERVAL_SECS=30
export JOB_ID="training-job-12345"             # 可选；为空时 ECS 自动命名
```

| 字段 | config/collector.json 键 | 对应环境变量 | 说明 |
| ---- | ------------------------ | ------------ | ---- |
| 推送目标 | `push_target_url` | `PUSH_TARGET_URL` | ECS `/push` 端点，空则禁用 |
| 推送间隔 | `push_interval_secs` | `PUSH_INTERVAL_SECS` | 秒，最小 10 |
| 训练任务 ID | _(无)_ | `JOB_ID` | ECS 使用此 ID 管理任务；为空时自动分配未命名任务 |

### ECS 服务器 API

| 方法 | 路径 | 说明 |
| ---- | ---- | ---- |
| `POST` | `/push` | 接收 Collector 推送数据（JSON Body + 请求头）|
| `GET` | `/api/collectors` | 返回所有 Collector 摘要列表 |
| `GET` | `/api/collector/{id}` | 返回单个 Collector 详情 |
| `GET` | `/api/collector/{id}/history` | 返回该任务的历史时序指标（用于趋势图）|
| `GET` | `/api/collector/{id}/events` | 返回该任务的关键事件（HANG 起止、故障节点变化、任务信息抓取）|
| `GET` | `/api/collector/{id}/flamegraph/all` | 代理拉取全量火焰图 SVG |
| `GET` | `/api/collector/{id}/flamegraph/{node_ip}` | 代理拉取指定节点火焰图 SVG |
| `GET` | `/` | Web 仪表盘首页（所有 Collector 卡片 + 24h HANG 时间线）|
| `GET` | `/collector/{id}` | 单个 Collector 详情页（含 6h 趋势折线 + 事件日志）|

`history` / `events` 端点接受以下查询参数：

| 参数 | 默认 | 说明 |
| ---- | ---- | ---- |
| `since` | _(无)_ | 起始时间戳（unix epoch 秒）|
| `until` | _当前时间_ | 结束时间戳 |
| `window_secs` | `21600`（6h） | 当未指定 `since` 时使用的回溯窗口，单位秒 |

### ECS 服务器环境变量

| 变量名 | 默认值 | 说明 |
| ------ | ------ | ---- |
| `ECS_ADDR` | `0.0.0.0:4000` | 服务监听地址 |
| `ECS_DB_PATH` | `./ecs_history.db` | SQLite 历史数据库文件路径（启动时自动创建）|
| `ECS_STALE_SECS` | `90` | 距上次推送超过该秒数 → 标记为「数据陈旧」（前端显示黄色徽章） |
| `ECS_OFFLINE_SECS` | `600` | 距上次推送超过该秒数 → 标记为「已离线/任务停止」（前端灰化卡片，并写入 `collector_offline` 事件） |
| `RUST_LOG` | `info` | 日志级别 |
| `JOB_PLATFORM_API_URL` | _(空)_ | 训练平台 API 地址，配置后 HANG 时自动查询任务信息 |
| `JOB_PLATFORM_APP_KEY` | _(空)_ | 训练平台 appKey |
| `JOB_PLATFORM_APP_SECRET` | _(空)_ | 训练平台 appSecret |
| `JOB_PLATFORM_USER_ID` | _(空)_ | 训练平台 userId |

### 历史数据持久化

ECS Server 内置 SQLite 历史存储，无需外部依赖：

- **保留窗口**：24 小时滚动；后台任务每 10 分钟自动清理过期数据
- **三类数据**：
  - `metrics_ts`：每次 push 写一行精简指标（healthy/warning/critical 节点数、P50/P99 步速、平均 GPU 利用率、HANG 标记），驱动趋势折线
  - `events`：稀疏事件流，记录 HANG 起止、故障节点出现/消失、`job_info` 抓取
  - `latest_snapshot` + `unnamed_alloc`：最新一帧完整 payload 与未命名任务计数，**重启不丢数据，不重新分配未命名任务编号**
- **DB 写入失败**仅打 `warn` 日志，不阻塞 push 响应链路

### 推送 Payload 格式

```json
{
  "timestamp": 1715000000,
  "global": { "total_nodes": 8, "healthy_nodes": 8, "avg_step_time_ms": 1200, ... },
  "nodes": [ { "ip": "10.0.0.1", "status": "Healthy", ... }, ... ],
  "hang": { "is_hanging": false, "hang_duration_secs": 0, ... }
}
```

ECS 使用 payload 中的 `job_id` 作为任务标识；为空时按来源 IP 自动分配未命名任务。火焰图代理地址由 ECS 根据请求来源 IP 推算为 `http://<来源IP>:3000`。

---



## 项目结构

```
super-trainning-collector/
├── Cargo.toml              # Workspace 配置
├── app/                    # 共享应用代码
│   └── src/
│       ├── components/     # UI 组件
│       │   ├── level1.rs       # 全局态势视图
│       │   ├── level2.rs       # 节点列表视图
│       │   ├── level3.rs       # 节点详情视图
│       │   ├── stack_view.rs   # 调用栈视图
│       │   └── common.rs       # 通用组件
│       ├── flamegraph/     # 火焰图模块
│       │   ├── flamegraph_generator.rs
│       │   ├── process_data.rs
│       │   ├── stack_collector.rs
│       │   └── stack_merger.rs
│       ├── adapter.rs      # 数据适配器
│       ├── api.rs          # API 接口
│       ├── models.rs       # 数据模型
│       ├── push_scheduler.rs  # 推送调度器（向 ECS 推送数据）
│       └── mock.rs         # Mock 数据
├── frontend/               # 前端 WASM 入口
│   └── src/
│       └── lib.rs
├── server/                 # 后端服务（Collector 监控面板）
│   └── src/
│       └── main.rs
├── ecs-server/             # ECS 汇聚服务器（多 Collector 汇总展示）
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs         # Axum 路由与启动
│   │   ├── state.rs        # 共享状态（DashMap）
│   │   └── handlers.rs     # HTTP 处理器
│   └── templates/
│       ├── dashboard.html  # 汇总仪表盘页面
│       └── collector.html  # 单 Collector 详情页面
├── config/                 # 配置文件
│   └── collector.json
├── style/                  # 样式文件
│   └── main.scss
├── public/                 # 静态资源
│   └── favicon.ico
├── end2end/                # E2E 测试
│   ├── tests/
│   └── playwright.config.ts
├── run_server.sh           # Collector 启动脚本
└── update_collector.sh     # 更新脚本
```

---

## 配置说明

### Leptos 配置

主要配置位于 `Cargo.toml` 的 `[workspace.metadata.leptos]`：

| 配置项         | 默认值              | 说明               |
| -------------- | ------------------- | ------------------ |
| `site-addr`    | `127.0.0.1:3000`    | 服务监听地址       |
| `site-root`    | `target/site`       | 静态资源输出目录   |
| `site-pkg-dir` | `pkg`               | 编译产物子目录     |
| `reload-port`  | `3001`              | 热重载端口         |
| `style-file`   | `style/main.scss`   | 样式入口文件       |
| `assets-dir`   | `public`            | 静态资源源目录     |
| `env`          | `DEV`               | 运行环境           |

### 应用配置

`config/collector.json` 完整字段：

```json
{
  "callstack_base_port": 9933,
  "step_query_port_offset": 1,
  "batch_size": 500,
  "job_platform_api_url": "",
  "job_platform_app_key": "",
  "job_platform_app_secret": "",
  "job_platform_user_id": "",
  "push_target_url": "",
  "push_interval_secs": 30
}
```

| 字段 | 默认值 | 说明 |
| ---- | ------ | ---- |
| `callstack_base_port` | — | Python 侧调用栈 API 基础端口（rank0 = base, rank1 = base+1 …）|
| `step_query_port_offset` | `1` | Step 查询端口偏移（相对 base port）|
| `batch_size` | `500` | 批量拉取调用栈的并发数 |
| `push_target_url` | `""` | ECS 推送端点（空表示禁用）|
| `push_interval_secs` | `30` | 推送间隔（秒）|

### 测试连通性

```python
# 换上真实的 MASTER_ADDR
python3 -c "import urllib.request; print(urllib.request.urlopen('http://ji-aitrain-155227064063522496-master-0.ji-aitrain-155227064063522496:9933/apis/nodes').read().decode())" | grep -o '"host"' | wc -l
```

---

## 数据模型

### 核心指标

| 指标            | 说明                 | 单位 |
| --------------- | -------------------- | ---- |
| Step Time       | 训练步骤耗时         | ms   |
| GPU Utilization | GPU 利用率           | %    |
| NCCL Latency    | 集合通信延迟         | ms   |
| Slow Ratio      | 慢 Rank 占比         | %    |

### 健康状态

```rust
pub enum HealthStatus {
    Healthy,   // 正常
    Warning,   // 性能下降
    Critical,  // 故障
}
```

---

## License

[Unlicense](LICENSE) - Public Domain
