# Rumo 的剪贴板技术设计候选方案

更新日期：2026-10-22（UTC+08:00）

这组文档由 Rumo 整理，供片羽／Pinfeather 项目评估。当前属于初步设计，具体选型、实现细节与性能指标待定，需通过原型验证后确定。

先阅读[项目定位、规划与技术评审交接](docs/pinfeather_project_handoff.md)，再按专题查阅候选设计。Rumo 是方案署名，片羽／Pinfeather 是产品名称；本文不使用另一个产品代号。

## 1. 七类候选设计

下列方案可以分别评估，不要求全部合并。模块名、类名、命令、协议字段及接口均为暂定示例；代码块是示意骨架，不代表已有应用或 SDK。

| 编号 | 技术专题 | 文档 | 重点评估内容 |
| --- | --- | --- | --- |
| A | 原生 Win32 | [候选方案 A](docs/rumo_plan_native_win32.md) | 格式快照、焦点恢复、修饰键与权限差异、多格式存储 |
| B | 脚本与 CLI | [候选方案 B](docs/rumo_plan_scriptable_cli.md) | ECMAScript 绑定、本地 IPC、Windows 排除标记与变化检测 |
| C | 同步、OCR 与 MCP | [候选方案 C](docs/rumo_plan_sync_mcp.md) | 设备发现、地址交换、配对加密、离线识别与材料访问 |
| D | 模板与卡片交互 | [候选方案 D](docs/rumo_plan_parametric_templates.md) | 参数插槽、表单预览、重复复制手势与按需加载 |
| E | 动态宏与规则 | [候选方案 E](docs/rumo_plan_dynamic_rules.md) | Roslyn 编译、规则条件、清洗与执行边界 |
| F | 输入感知与队列粘贴 | [候选方案 F](docs/rumo_plan_queue_injection.md) | 钩子、输入法状态、FIFO／LIFO 与跨架构适配 |
| G | 轻量驻留与历史存储 | [候选方案 G](docs/rumo_plan_lightweight_storage.md) | 分页索引、摘要与载荷分离、内部写入过滤及内存测量 |

## 2. 综合材料

- [候选方案对比矩阵](docs/rumo_clipboard_architecture_matrix.md)：按格式、监听、回贴、扩展、同步和交互比较设计思路；表中能力和数值均需验证。
- [待修订的综合设计草案（原 MVP）](docs/rumo_clipboard_mvp_specification.md)：保留综合架构与代码示意，正文尚未完成技术修订。已确认的问题见交接文档 R1–R8，不能直接当作可实施规格。
- [设计研究任务参考](clipboard_re_prompt.txt)：后续分析与验证的任务说明，不假定当前工作区已有源码、二进制或实现原型。

## 3. 状态与使用边界

当前目录包含产品说明、品牌资源与设计材料，没有可运行产品。候选设计中的能力、依赖、平台范围和性能数字不代表已经实现、采用或实测通过。

已有技术评审问题继续保留。文档整理不代表修复示例代码，也不代替真实客户端接入、两机同步及 Windows 回贴测试。
