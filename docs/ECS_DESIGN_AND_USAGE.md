# ECS 汇聚服务设计与使用说明

本文档说明 `ecs-server` 的设计目标、数据流、部署方式、Collector 推送配置和常用排查方法。ECS 在本项目中指多 Collector 汇聚展示服务，用于把多台 `super-training-collector` 实例的最新监控快照集中到一个 Web 仪表盘中查看。

## 1. 设计目标

ECS 解决单个 Collector 只能查看本集群或本节点监控视图的问题，核心目标是：

- 支持多台 Collector 主动向一个中心服务推送监控快照。
- 在中心 Web UI 中按训练任务维度展示集群健康状态、节点指标和 HANG 状态。
- 保留轻量部署模型：ECS 不依赖数据库，只保存每个 Collector 的最新快照。
- 通过 ECS 反向代理火焰图请求，用户无需分别访问各 Collector UI。
- 在检测到 HANG 且配置训练平台信息时，异步查询训练任务详情并在详情接口中返回。

## 2. 总体架构

```text
Collector A ─┐
             │  POST /push
Collector B ─┼────────────► ECS Server ───────────► Web Dashboard
             │              :4000                  /
Collector C ─┘                                     /
                                                   /api/collector/{id}/flamegraph/*
                                      ECS 反向代理 ─────────────► 对应 Collector :3000
```

组件职责如下：

| 组件 | 位置 | 职责 |
| --- | --- | --- |
| Collector 推送调度器 | `app/src/push_scheduler.rs` | 定时采集全局指标、节点列表、HANG 快照和 `JOB_NAME`，POST 到 ECS `/push` |
| ECS HTTP 服务 | `ecs-server/src/main.rs` | 注册 `/push`、JSON API、HTML 页面和火焰图代理路由 |
| ECS 共享状态 | `ecs-server/src/state.rs` | 使用 `DashMap<String, CollectorEntry>` 保存各 Collector 最新快照 |
| ECS 请求处理器 | `ecs-server/src/handlers.rs` | 处理推送、查询、页面渲染和火焰图代理 |
| 任务平台客户端 | `ecs-server/src/job_info_client.rs` | HANG 时按 `job_id` 查询训练平台任务详情 |
| Web 模板 | `ecs-server/templates/` | 总览页和 Collector 详情页，前端每 30 秒刷新 |

## 3. 数据流

1. Collector 启动后读取推送配置。环境变量优先级高于 `config/collector.json`。
2. 如果 `PUSH_TARGET_URL` 或 `push_target_url` 为空，推送调度器禁用。
3. 推送启用后，Collector 每隔 `PUSH_INTERVAL_SECS` 秒采集一次当前数据并发送到 ECS。
4. ECS 根据 payload 中的 `job_id` 识别任务。
5. 如果 `job_id` 为空，ECS 按来源 IP 分配稳定的 `未命名任务1`、`未命名任务2` 等名称。
6. ECS 根据推送来源 IP 推算该 Collector 的 REST 基础地址：`http://<来源IP>:3000`。
7. ECS 将该任务的旧快照替换为新快照，并更新 `last_seen`。
8. Web UI 通过 `/api/collectors` 和 `/api/collector/{id}` 获取数据展示。
9. 用户请求火焰图时，ECS 转发到对应 Collector 的 `/rest/flamegraph/*` 端点实时生成 SVG。

## 4. 数据模型

Collector 推送到 ECS 的 payload 结构如下：

```json
{
  "timestamp": 1715000000,
  "global": {
    "total_nodes": 8,
    "healthy_nodes": 8,
    "warning_nodes": 0,
    "critical_nodes": 0,
    "current_step": 1234,
    "global_avg_gpu_utilization": 95.5,
    "global_p99_step_time_ms": 1200,
    "total_ranks": 64
  },
  "nodes": [
    {
      "node_ip": "10.0.0.1",
      "hostname": "node-0",
      "rank_count": 8,
      "status": "Healthy",
      "avg_gpu_utilization": 96.0,
      "p99_step_time_ms": 1180,
      "slow_ratio": 0.0
    }
  ],
  "hang": {
    "status": "Normal",
    "is_hanging": false
  },
  "job_id": "training-job-12345"
}
```

ECS 不对 payload 做复杂转换，详情接口会返回原始 `payload`。总览接口会从 `payload.global` 和 `payload.hang.status` 中提取摘要字段。

## 5. 启动 ECS 服务

在仓库根目录构建：

```bash
cargo build --release -p ecs-server
```

启动默认监听 `0.0.0.0:4000`：

```bash
./target/release/ecs-server
```

也可以进入子目录开发运行：

```bash
cd ecs-server
cargo run
```

修改监听地址：

```bash
ECS_ADDR=0.0.0.0:8080 ./target/release/ecs-server
```

访问 `http://<ecs-host>:4000` 查看汇总仪表盘。

## 6. ECS 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `ECS_ADDR` | `0.0.0.0:4000` | ECS 服务监听地址 |
| `RUST_LOG` | `info` | 日志级别，例如 `debug`、`info`、`warn` |
| `JOB_PLATFORM_API_URL` | 空 | 训练平台 API 地址 |
| `JOB_PLATFORM_APP_KEY` | 空 | 训练平台 appKey |
| `JOB_PLATFORM_APP_SECRET` | 空 | 训练平台 appSecret |
| `JOB_PLATFORM_USER_ID` | 空 | 查询任务详情时使用的 userId |

任务平台变量全部配置后，ECS 在收到 HANG 状态且 payload 中有 `job_id` 时，会异步查询任务详情。查询失败不会影响 `/push` 接收，详情信息为空。

## 7. 配置 Collector 推送

推荐使用环境变量配置每台 Collector：

```bash
export PUSH_TARGET_URL="http://<ecs-host>:4000/push"
export PUSH_INTERVAL_SECS=30
export JOB_NAME="training-job-12345"
```

也可以写入 `config/collector.json`：

```json
{
  "callstack_base_port": 9933,
  "step_query_port_offset": 1,
  "batch_size": 500,
  "push_target_url": "http://<ecs-host>:4000/push",
  "push_interval_secs": 30
}
```

配置优先级：

| 配置项 | 环境变量 | 配置文件字段 | 说明 |
| --- | --- | --- | --- |
| 推送目标 | `PUSH_TARGET_URL` | `push_target_url` | 为空时禁用推送 |
| 推送间隔 | `PUSH_INTERVAL_SECS` | `push_interval_secs` | 最小 10 秒，默认 30 秒 |
| 训练任务名 | `JOB_NAME` | 无 | ECS 使用此值管理任务；为空时自动分配未命名任务 |

火焰图代理地址由 ECS 根据推送来源 IP 推算为 `http://<来源IP>:3000`。如果 ECS 无法访问该地址，总览和详情仍可显示，但火焰图代理会失败。

## 8. API 说明

### `POST /push`

Collector 推送入口。

请求头：

| Header | 必须 | 说明 |
| --- | --- | --- |
| `Content-Type` | 是 | `application/json` |

响应：

| 状态码 | 说明 |
| --- | --- |
| `200 OK` | 接收成功 |
| `400 Bad Request` | JSON 解析失败 |

### `GET /api/collectors`

返回所有 Collector 摘要列表，按 `id` 排序。

示例：

```json
[
  {
    "id": "gpu-cluster-A",
    "source_ip": "10.0.0.1",
    "collector_addr": "http://10.0.0.1:3000",
    "last_seen": 1715000000,
    "last_seen_ago_secs": 12,
    "global": {},
    "hang_status": "Normal",
    "node_count": 8,
    "critical_count": 0
  }
]
```

### `GET /api/collector/{id}`

返回单个 Collector 详情，包括来源 IP、Collector 地址、最新 payload、`job_id` 和可选 `job_info`。

Collector 不存在时返回 `404`。

### `GET /api/collector/{id}/flamegraph/all`

代理到：

```text
{collector_addr}/rest/flamegraph/all
```

### `GET /api/collector/{id}/flamegraph/{node_ip}`

代理到：

```text
{collector_addr}/rest/flamegraph/{node_ip}
```

火焰图代理超时时间为 120 秒，返回内容类型为 `image/svg+xml`。

## 9. Web UI

| 页面 | 说明 |
| --- | --- |
| `/` | 总览页，展示所有 Collector 的状态卡片、节点计数、GPU 利用率、当前 step 和 HANG 状态 |
| `/collector/{id}` | 详情页，展示全局指标、HANG 状态、节点表格和火焰图入口 |

前端页面每 30 秒自动刷新一次，也可以手动点击刷新按钮。

## 10. 部署建议

- ECS 与 Collector 建议部署在同一内网，确保 ECS 能通过推送来源 IP 访问 Collector 的 3000 端口。
- 生产环境建议设置 `JOB_NAME`，这样 ECS 可以按训练任务稳定管理；未设置时仅按来源 IP 分配未命名任务。
- 当前 ECS 不内置鉴权，建议通过内网访问控制或反向代理添加 TLS、Basic Auth、IP 白名单等保护。
- ECS 仅保存最新快照，不保存历史数据；如需历史趋势，需要额外接入时序数据库或日志链路。
- Collector 下线后，ECS 会保留其最后一次快照，不会自动清理。

## 11. 常见问题排查

| 现象 | 可能原因 | 排查建议 |
| --- | --- | --- |
| ECS 首页没有 Collector | Collector 未配置 `PUSH_TARGET_URL`，或网络不通 | 检查 Collector 日志中的 `[push_scheduler]`，确认目标 URL 和推送状态 |
| Collector 卡片一直显示旧时间 | Collector 停止推送或推送失败 | 检查 Collector 进程、ECS 地址、防火墙和 `/push` 返回状态 |
| 多个任务被合并成一个 | 多个 Collector 使用相同 `JOB_NAME`，或未设置 `JOB_NAME` 且来源 IP 相同 | 为不同训练任务设置不同 `JOB_NAME` |
| 火焰图加载失败 | ECS 无法通过来源 IP 访问 Collector | 在 ECS 机器上访问 `http://<来源IP>:3000/rest/flamegraph/all` 验证连通性 |
| HANG 时没有任务详情 | 未配置任务平台变量，或 `JOB_NAME` 为空，或平台 API 查询失败 | 检查 ECS 的 `JOB_PLATFORM_*` 环境变量和 `[job_info]` 日志 |
| `/push` 返回 400 | 请求体不是合法 JSON | 检查推送 payload 或手工请求中的 JSON 格式 |
