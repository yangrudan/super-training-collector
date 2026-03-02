# Super Training Collector

基于 Rust + Leptos 构建的千卡级分布式训练任务 Web 监控面板。

## 功能特性

- **分层视图**：采用「全局 → 节点 → Rank」三级下钻架构
- **热力编码**：通过颜色直观展示健康状态（绿色正常/黄色警告/红色故障）
- **实时监控**：追踪 Step Time、GPU 利用率、NCCL 延迟等关键指标
- **高效定位**：聚焦问题节点，快速定位千卡规模下的性能瓶颈

## 视图层级

| 层级 | 视图 | 描述 |
|------|------|------|
| Level 1 | 全局态势 | 健康分布、全局 KPI、拓扑热力图 |
| Level 2 | 节点列表 | 节点性能表格，支持排序/筛选 |
| Level 3 | 节点详情 | 单节点 Rank 级别指标详情 |

## 技术栈

- **框架**: [Leptos](https://leptos.dev/) 0.8 (全栈 Rust Web 框架)
- **后端**: Axum
- **前端**: WebAssembly (wasm32-unknown-unknown)
- **样式**: SCSS

## 环境要求

- Rust nightly toolchain
- `wasm32-unknown-unknown` target
- `cargo-leptos` CLI 工具

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

## 项目结构

```
super-trainning-collector/
├── app/                    # 共享应用代码 (组件、模型、API)
│   └── src/
│       ├── components/     # UI 组件
│       │   ├── level1.rs   # 全局态势视图
│       │   ├── level2.rs   # 节点列表视图
│       │   ├── level3.rs   # 节点详情视图
│       │   └── common.rs   # 通用组件
│       ├── models.rs       # 数据模型定义
│       ├── api.rs          # API 接口
│       └── mock.rs         # Mock 数据
├── frontend/               # 前端 WASM 入口
├── server/                 # 后端服务入口
├── style/                  # SCSS 样式
├── public/                 # 静态资源
└── end2end/                # E2E 测试
```

## 数据模型

### 核心指标

- **Step Time**: 训练步骤耗时 (ms)
- **GPU Utilization**: GPU 利用率 (%)
- **NCCL Latency**: 集合通信延迟 (ms)
- **Slow Ratio**: 慢 Rank 占比

### 健康状态

```rust
pub enum HealthStatus {
    Healthy,   // 正常
    Warning,   // 性能下降
    Critical,  // 故障
}
```

## 配置

主要配置位于 `Cargo.toml` 的 `[workspace.metadata.leptos]` 部分：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `site-addr` | `127.0.0.1:3000` | 服务监听地址 |
| `reload-port` | `3001` | 热重载端口 |
| `style-file` | `style/main.scss` | 样式入口文件 |

## License

[Unlicense](LICENSE) - Public Domain
