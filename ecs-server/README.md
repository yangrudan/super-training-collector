# ECS 汇聚服务器

独立的多 Collector 汇聚展示服务，用于接收多台 `super-training-collector` 实例推送的监控数据，并通过 Web UI 集中展示。

---

## 架构

```
Collector A (port 3000) ─┐
                          │  HTTP POST /push
Collector B (port 3000) ──►  ECS Server (port 4000)  ──►  Web Dashboard
                          │
Collector C (port 3000) ─┘
```

- 每台 Collector 每隔 N 秒将全局指标、节点列表、HANG 状态以 JSON POST 推送至 ECS。
- ECS 使用 payload 中的 `job_id` 区分任务；为空时按来源 IP 自动分配 `未命名任务N`。
- ECS 保留每台 Collector 的最新数据快照，Web 仪表盘每 30 秒自动刷新。
- 火焰图通过 ECS 代理从对应 Collector 实时拉取（SVG）。

---

## 快速启动

### 构建与运行

```bash
cd ecs-server

# 开发模式
cargo run

# 生产构建
cargo build --release
./target/release/ecs-server
```

默认监听 `0.0.0.0:4000`，通过 `ECS_ADDR` 环境变量修改：

```bash
ECS_ADDR=0.0.0.0:8080 ./target/release/ecs-server
```

### 环境变量

| 变量名 | 默认值 | 说明 |
| ------ | ------ | ---- |
| `ECS_ADDR` | `0.0.0.0:4000` | 服务监听地址（`host:port`）|
| `RUST_LOG` | `info` | 日志级别（`trace`/`debug`/`info`/`warn`/`error`）|
| `JOB_PLATFORM_API_URL` | _(空)_ | 训练平台 API 地址，配置后 HANG 时自动查询任务信息 |
| `JOB_PLATFORM_APP_KEY` | _(空)_ | 训练平台 appKey |
| `JOB_PLATFORM_APP_SECRET` | _(空)_ | 训练平台 appSecret |
| `JOB_PLATFORM_USER_ID` | _(空)_ | 训练平台 userId（查询任务详情时使用）|

---

## 配置 Collector 推送

在每台 Collector 机器上，通过环境变量或 `config/collector.json` 启用推送：

```bash
# 必填：ECS 推送端点
export PUSH_TARGET_URL="http://<ecs-host>:4000/push"

# 可选：推送间隔（秒，最小 10，默认 30）
export PUSH_INTERVAL_SECS=30

# 可选：训练任务 ID，ECS 使用它作为任务标识
export JOB_ID="training-job-12345"
```

或写入 `config/collector.json`：

```json
{
  "callstack_base_port": 9933,
  "push_target_url": "http://10.0.0.100:4000/push",
  "push_interval_secs": 30
}
```

> **优先级**：环境变量 > `collector.json` 文件

---

## API 参考

### 推送接口（Collector → ECS）

#### `POST /push`

接收 Collector 的指标数据推送。

**请求头**

| 请求头 | 必须 | 说明 |
| ------ | ---- | ---- |
| `Content-Type` | 是 | `application/json` |

**请求体**

```json
{
  "timestamp": 1715000000,
  "global": {
    "total_nodes": 8,
    "healthy_nodes": 8,
    "warning_nodes": 0,
    "critical_nodes": 0,
    "avg_step_time_ms": 1200,
    "avg_gpu_utilization": 95.5,
    "slow_rank_ratio": 0.0
  },
  "nodes": [
    {
      "ip": "10.0.0.1",
      "hostname": "node-0",
      "status": "Healthy",
      "step_time_ms": 1180,
      "gpu_utilization": 96.0,
      "nccl_latency_ms": 12.5,
      "slow_ranks": 0,
      "total_ranks": 8
    }
  ],
  "hang": {
    "is_hanging": false,
    "hang_duration_secs": 0,
    "last_check_time": 1715000000,
    "similarity_score": 0.98
  },
  "job_id": "training-job-12345"
}
```

**响应**

- `200 OK`：数据接收成功
- `400 Bad Request`：JSON 解析失败

---

### 查询接口（Web UI → ECS）

#### `GET /api/collectors`

返回所有已注册 Collector 的摘要列表。

**响应示例**

```json
[
  {
    "id": "gpu-cluster-A",
    "addr": "http://10.0.0.1:3000",
    "last_seen": 1715000000,
    "total_nodes": 8,
    "healthy_nodes": 8,
    "warning_nodes": 0,
    "critical_nodes": 0,
    "is_hanging": false
  }
]
```

#### `GET /api/collector/{id}`

返回单个 Collector 的完整快照（全局指标 + 节点列表 + HANG 状态）。

#### `GET /api/collector/{id}/flamegraph/all`

代理拉取该 Collector 的全量火焰图（SVG），转发至 `{collector_addr}/rest/flamegraph/all`。

#### `GET /api/collector/{id}/flamegraph/{node_ip}`

代理拉取指定节点的火焰图，转发至 `{collector_addr}/rest/flamegraph/{node_ip}`。

---

### 页面接口

| 路径 | 说明 |
| ---- | ---- |
| `GET /` | 仪表盘首页，展示所有 Collector 状态卡片 |
| `GET /collector/{id}` | 单个 Collector 详情页（指标、节点表格、火焰图）|

---

## 项目结构

```
ecs-server/
├── Cargo.toml              # 依赖声明（axum, tokio, dashmap, reqwest, serde）
├── README.md
├── src/
│   ├── main.rs             # Axum 路由注册与服务启动
│   ├── state.rs            # 共享状态定义（DashMap、JobPlatformConfig、JobInfo）
│   ├── handlers.rs         # 所有 HTTP 处理器（push/api/page/proxy）
│   └── job_info_client.rs  # 训练平台任务信息查询客户端
└── templates/
    ├── dashboard.html      # 首页：Collector 汇总卡片，30s 自动刷新
    └── collector.html      # 详情页：全局指标、HANG 状态、节点表格、火焰图
```

### 关键设计

- **并发安全**：使用 `DashMap`（无锁分段并发 Map）存储 Collector 数据，无需额外 Mutex。
- **任务识别**：使用 payload 中的 `job_id` 作为 ECS 任务标识；为空时按来源 IP 分配稳定的 `未命名任务N`。
- **火焰图代理**：ECS 将火焰图请求转发给对应 Collector 的 REST 端点（`/rest/flamegraph/*`），超时 120 秒。
- **HTML 嵌入**：模板文件通过 `include_str!()` 在编译时嵌入二进制，无需外部文件。

---

## 端口说明

| 端口 | 服务 | 说明 |
| ---- | ---- | ---- |
| `3000` | Collector（Web UI）| 每台训练集群节点的 Leptos 监控面板 |
| `4000` | ECS Server | 本服务，汇聚多 Collector 数据 |
| `9933+` | Python 侧 API | 训练进程调用栈采集端口（每个 rank 一个端口）|

---

## 注意事项

- **数据保留**：ECS 仅保存每台 Collector 的**最新快照**，无历史记录。
- **网络要求**：ECS 代理火焰图时会根据推送来源 IP 访问 `http://<来源IP>:3000`，请确保该地址从 ECS 可达。
- **无鉴权**：当前无内置认证机制，建议部署在内网或通过 Nginx 加 TLS + Basic Auth。
- **Collector 下线**：Collector 停止推送后，ECS 仍保留其最后一次数据，不会自动清除。
