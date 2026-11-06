# M1/P2 本地检查点（2026-11-06）

> 后续热测量、工具修复及 P3 后台基准验收见 [2026-11-07 检查点](pinfeather_m1_checkpoint_20261107.md)。下文保留为较早阶段的历史快照。

M1/P2 尚未完成。退出时序诊断工具及原生采集包 v2 完成离线验收；新增一次非空库原生诊断通过功能、事件及已记录进程身份的退出检查。原非空库失败记录仍为 FAILED，原因尚未查明。

## 已验证结果

- 普通模式与空库 opt-in 的单次隔离检查已分别验收，不能外推为正式性能验收。
- 原非空库运行记录完整的 12 条事件，包含主窗口就绪、快捷键触发、面板就绪且输入框获得焦点、run_end。根进程正常退出；采集截止时仍有两个同创建身份的子进程存活，因此整体不通过。后续查询中的消失不能改写原失败，也不能确定实际终止时刻。
- 退出时序诊断工具仅用于受控离线验证。v1 源码比对器错误忽略嵌套 `__main__` 分支的问题已在独立 v2 中修复；v1 保留为未验收证据。
- v2 冻结输入共 377 项，SHA256 为 `8367438f45ad5d3925f5a8278c44dc665a2119d9c785f77960f508ab531c1664`。执行者与主控分别完成 31 项测试，均为 0 failures、0 errors；执行者的 16 个语义变体均被指定断言捕获。独立复审完成 8 项定向测试和额外比对器变体检查，0 项发现。
- 诊断保留原有 1800/30 秒限制、控制时钟、判定顺序及原始退出收据权威。诊断完整不等于退出成功，也不授予再次启动权限。没有修复或证明实际 WebView 退出原因。

## 原生采集包与新增现场结果

- 原生采集包 v1 保留为未验收证据。v2 冻结输入共 396 项，清单 SHA256 为 `5cc3633c15bba2842b26c01f14cc72073af1621216dbace3a5c7f9d0340f2bca`。执行者与主控分别完成 36 项受控测试，均为 0 failures、0 errors；执行者的 15 个语义变体均被指定断言捕获。独立复审完成 12 项定向测试及 3 个变体检查，0 项发现。此处的原生采集包 v2 与前述 377 项输入的退出时序工具 v2 是不同归档。
- 新增运行 `diag-nonempty-sol-v1` 使用隔离的固定 3 条夹具。夹具 helper 正常退出，采集器自然退出且返回 0，严格时序读取器确认记录完整、权威退出判定通过；现场独立复审 0 项发现。
- 现场共记录 12 条连续事件，包含主窗口非空就绪、一次原生快捷键出队、输入框获得焦点的非空面板就绪及完整 run_end。两项非空就绪记录成功，没有错误事件。根进程正常退出，另外 7 个已记录创建身份的进程查询为不存在；采集以 `all_terminal` 结束，没有超时。
- 根进程创建距当前就绪回复约 2.214 秒。用户完成回复仅为“已完成”，不能据此补写未记录的原始按键、点击或逐项人工结论。
- 这是一次新的成功诊断，不证明产品缺陷已修复，也不解释原失败原因。退出保证仅覆盖已记录的进程身份，不能外推到未知或未观测子进程。正式 cold10/hot50、P4 及完整 M1/P2 验收均未完成。

## 证据位置

任务归档根目录为 `D:/Develop/Misc Workspace/archive`。

| 内容 | 归档内路径 |
| --- | --- |
| 主控验收与续接入口 | `pinfeather-m1-p2-sol-coordination/run-20261105-v1/EXIT-TIMING-V2-OWNER-ACCEPTANCE.md`、`REST-CHECKPOINT-HANDOFF.md` |
| v2 源码、报告与冻结清单 | `pinfeather-m1-p2-exit-timing-diagnostic/run-20261105-sol-v2/` |
| v2 独立复审 | `pinfeather-m1-p2-exit-timing-diagnostic-review/run-20261105-sol-v2/REVIEW.md` |
| 非空库失败处置 | `pinfeather-m1-p2-sol-coordination/run-20261105-v1/OPTIN-NONEMPTY-OWNER-DISPOSITION.json` |
| 原生采集包 v2 离线验收 | `pinfeather-m1-p2-sol-coordination/run-20261105-v1/NATIVE-V2-OFFLINE-OWNER-ACCEPTANCE.md`、`NATIVE-V2-OWNER-VERIFICATION.json` |
| 原生采集包 v2 源码与冻结清单 | `pinfeather-m1-p2-native-exit-timing-package/run-20261106-sol-v2/` |
| 原生采集包 v2 独立复审 | `pinfeather-m1-p2-native-exit-timing-package-review/run-20261106-sol-v2/REVIEW.md` |
| 新增现场验收与校验收据 | `pinfeather-m1-p2-sol-coordination/run-20261105-v1/NATIVE-V2-ACTUAL-OWNER-ACCEPTANCE.md`、`NATIVE-V2-ACTUAL-OWNER-VERIFICATION.json`、`NATIVE-V2-ACTUAL-VALIDATION.json` |
| 新增现场证据 | `pinfeather-m1-p2-native-smoke/run-20261106-sol-native-exit-timing-v1/optin-nonempty/diag-nonempty-sol-v1/` |
| 新增现场独立复审 | `pinfeather-m1-p2-native-smoke-review/run-20261106-sol-native-timing-v2/REVIEW.md` |

这些是本机归档引用；诊断脚本、测试、数据库和日志未复制进项目，也不随本次 Git 提交保存。各轮检查绑定各自记录的 main HEAD/index。文档检查点提交会使 Git 元数据有预期变化；继续时应建立新的只读基线，不能改旧冻结清单来掩盖差异。`REST-CHECKPOINT-HANDOFF.md` 等早期暂停记录只描述当时状态；当前现场结论以 `NATIVE-V2-ACTUAL-OWNER-ACCEPTANCE.md/.json` 为准。

## 提交范围与下一步

只保存这份正式记录到本地 Git，不推送；接手前已有的 18 项修改和 19 项未跟踪文件保留。原诊断工作树保持原分支和提交。本轮没有产品源码改动，产品启动仅限已验收的单次授权诊断。

下一步先离线整理正式性能验收执行卡：P1 环境、cold10/hot50 的有效及失败样本规则、进程退出前置条件，以及后续搜索规模和资源测量。已有单次成功样本不代替正式批量样本。

此次夹具、检查及启动权限均已消耗，不得重用当前命名空间重试或重新播种。下一次原生检查须先形成具体方案、取得对应范围授权及当前就绪回复。启动前一次说明全部步骤，并明确“窗口出现即开始操作；子 agent 正在采集，无需等它结束”；退出后确认实际记录到的事件。

正式 cold10/hot50、其余环境/搜索/资源验收尚未完成；不据此宣称 M1/P2 或首轮产品验收完成。
