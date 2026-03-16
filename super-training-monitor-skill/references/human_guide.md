# 超级训练监控技能

这是一个Claude Code技能，用于与基于Leptos的超级训练监控面板交互，获取分布式训练集群的监控数据。

## 功能特性

- **全局状态监控**：获取集群总体健康状态、性能指标和训练进度
- **节点分析**：查看所有节点列表，支持排序和筛选，识别慢节点
- **详细诊断**：获取特定节点的rank级别详细指标
- **堆栈分析**：生成节点火焰图，可视化调用栈热点
- **Step监控**：获取训练step的详细指标（如果启用）
- **拓扑视图**：查看机柜-节点的物理拓扑结构

## 技能结构

```
super-training-monitor-skill/
├── SKILL.md                    # 主要技能文档
├── README.md                   # 本文件
├── references/
│   └── data_structures.md      # 数据结构参考文档
├── scripts/
│   └── example_usage.py        # Python API使用示例
└── evals/
    └── evals.json              # 测试用例
```

## 快速开始

### 1. 安装技能
将整个 `super-training-monitor-skill` 目录复制到你的Claude Code技能目录中，或使用技能包管理器安装。

### 2. 确保监控面板运行
确保超级训练监控面板服务正在运行。默认地址是 `http://127.0.0.1:3000`。

### 3. 使用技能
在Claude Code中，当需要监控训练集群时，技能会自动触发。例如：

```
用户：我的训练监控面板运行在3000端口，帮我看看集群状态
Claude：好的，我将使用超级训练监控技能获取数据...
```

## API支持

技能支持以下API端点：

| API | 功能 | 请求示例 |
|-----|------|----------|
| `get_global_metrics` | 全局聚合指标 | `{}` |
| `get_nodes` | 节点列表（可排序筛选） | `{"sort_field": "SlowRatio", "sort_order": "Desc"}` |
| `get_node_ranks` | 节点Rank详情 | `{"ip": "192.168.1.100"}` |
| `get_node_flamegraph` | 节点火焰图 | `{"ip": "192.168.1.100"}` |
| `get_global_step_metrics` | 全局Step指标 | `{}` |
| `get_topology` | 拓扑视图 | `{}` |

## 使用示例

### 示例1：全面集群诊断
```bash
# 使用Python示例脚本
python scripts/example_usage.py --url http://127.0.0.1:3000
```

### 示例2：获取特定节点数据
```bash
python scripts/example_usage.py --url http://127.0.0.1:3000 --ip 192.168.1.100
```

## 依赖要求

- Claude Code 支持 WebFetch 工具
- 运行中的超级训练监控面板服务
- Python 3.7+（仅用于示例脚本）

## 配置选项

### 默认地址
技能默认使用 `http://127.0.0.1:3000`，但会先询问用户确认。

### 环境变量
监控面板支持的环境变量：
- `COLLECTOR_MOCK_MODE=true`：启用模拟数据模式
- `STEP_SHOW=true`：启用Step指标功能

## 开发与测试

### 测试用例
技能包含5个测试用例，覆盖主要使用场景：

1. 获取全局状态
2. 查找慢节点
3. 分析特定节点
4. 检查Step指标
5. 集群诊断

### 运行测试
使用技能创建工具的测试框架运行测试用例。

## 故障排除

### 常见问题

1. **连接失败**
   - 检查监控面板服务是否运行：`curl http://127.0.0.1:3000`
   - 确认IP和端口正确

2. **API返回错误**
   - 检查请求体格式是否正确
   - 确认API端点名称正确
   - 查看服务日志

3. **Step功能未启用**
   - 需要设置环境变量 `STEP_SHOW=true`
   - 重启监控面板服务

### 日志查看
监控面板日志通常输出到控制台，包含API调用和错误信息。

## 技能优化建议

### 性能优化
- 对于大型集群，考虑分批次获取数据
- 缓存频繁访问的全局指标
- 使用异步请求提高响应速度

### 功能扩展
- 添加历史数据对比功能
- 实现自动告警规则
- 支持自定义指标阈值
- 集成到CI/CD流水线

## 相关项目

- [超级训练监控面板](https://gitlab.zhejianglab.com/research-center-for-high-efficiency-computing-infrastructure/nhhal/supertrainningcollector)：基于Leptos的训练监控系统
- [Leptos框架](https://github.com/leptos-rs/leptos)：用于构建Web应用的Rust框架

## 许可证

MIT

## 支持与反馈

如有问题或建议，请提交Issue或联系维护者。