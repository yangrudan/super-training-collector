# Changelog

本文件记录项目的所有重要变更，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Added
- 添加版本管理系统
- 添加 CHANGELOG.md 记录变更历史
- 添加版本发布和部署脚本
- **问题 Rank 自动检测**：基于 StackTrie 分叉检测算法，遍历合并堆栈的 Trie 分叉点，识别覆盖率低于阈值的少数派 Rank
  - HANG 检测确认后自动触发分析
  - Dashboard「问题 Rank 分析」Tab 支持手动触发
  - API 端点 `AnalyzeProblematicRanks`（手动触发）和 `GetProblematicRanks`（缓存结果）
  - 可配置少数派阈值（`RANK_ANALYSIS_MINORITY_THRESHOLD`，默认 30%）
  - Level 1 首页自动展示分析摘要，Level 2 新增完整分析 Tab

## [0.1.0] - 2026-03-12

### Added
- 初始版本
- 支持多节点训练数据收集
- 火焰图生成功能
- Web Dashboard 界面
- TCP 连接重试功能
- 节点信息可选字段支持

### Changed
- 将 "机柜" 标签重命名为 "主机名"

---

[Unreleased]: https://gitlab.zhejianglab.com/research-center-for-high-efficiency-computing-infrastructure/nhhal/supertrainningcollector/compare/v0.1.0...HEAD
[0.1.0]: https://gitlab.zhejianglab.com/research-center-for-high-efficiency-computing-infrastructure/nhhal/supertrainningcollector/releases/tag/v0.1.0
