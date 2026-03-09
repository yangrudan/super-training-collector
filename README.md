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

### 手动启动

```bash
cd /opt/collector
export LEPTOS_SITE_ROOT="target/site"
export LEPTOS_SITE_ADDR="0.0.0.0:3000"
export LEPTOS_ENV="PROD"
./server
```

### Systemd 服务（可选）

创建 `/etc/systemd/system/collector.service`：

```ini
[Unit]
Description=Super Training Collector
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/collector
Environment="LEPTOS_SITE_ROOT=target/site"
Environment="LEPTOS_SITE_ADDR=0.0.0.0:3000"
Environment="LEPTOS_ENV=PROD"
ExecStart=/opt/collector/server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

启用服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable collector
sudo systemctl start collector
```

### 注意事项

- `LEPTOS_SITE_ROOT` 必须指向包含 `pkg/` 的目录，路径相对于二进制运行目录
- 生产环境建议将 `LEPTOS_SITE_ADDR` 设为 `0.0.0.0:3000` 以允许外部访问
- 可通过 Nginx 反向代理实现 HTTPS 和负载均衡

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
│       └── mock.rs         # Mock 数据
├── frontend/               # 前端 WASM 入口
│   └── src/
│       └── lib.rs
├── server/                 # 后端服务
│   └── src/
│       └── main.rs
├── config/                 # 配置文件
│   └── collector.json
├── style/                  # 样式文件
│   └── main.scss
├── public/                 # 静态资源
│   └── favicon.ico
├── end2end/                # E2E 测试
│   ├── tests/
│   └── playwright.config.ts
├── run_server.sh           # 启动脚本
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

`config/collector.json`：

```json
{
  "callstack_base_port": 9933
}
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
