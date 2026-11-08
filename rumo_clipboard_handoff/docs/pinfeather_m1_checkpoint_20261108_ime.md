# M1 本地检查点：IME 调查与工具审阅（2026-11-08）

> 后续状态见[诊断收敛与有限交付来源检查点](pinfeather_m1_checkpoint_20261108_delivery_reconciliation.md)；本文保留提交时的历史状态。

本记录承接[热测量与进程归属检查点](pinfeather_m1_checkpoint_20261107_process_attribution.md)。当前 IME 完成输入仍受阻，产品交付及 M1 整体验收未完成。工具的离线测试通过不代表现场执行资格。

## 当前状态

- 调查方向已收敛为旧成功诊断候选与当前候选的一次有界配对比较提案，不继续开放式扩建底层诊断体系。真实比较尚未执行；完整性能和进程归属工作不在当前交付主线上。
- 工具 v3 修正了两侧前置记录快照关联、启动前登记摘要核对、部分读线程启动失败时的管道清理。冻结实现的 84 项离线测试通过，三个故意恢复遗漏的变体被行为断言识别。
- 独立只读审阅使用实时 Rumo 配置中的 Codex / gpt-6.1-sol / high，报告已完成。负责人采纳 1 项 P2：就绪在核验或 EXE 哈希期间超过 120 秒后，首个进程查询或启动仍可能发生。
- 两条假时钟回归已复现：首次核验耗时跨期后，假查询仍在第 121 秒发生；启动前哈希跨期后，假启动仍在第 121 秒发生。没有调用 Windows 或启动产品。该问题尚未修正，v3 暂不能进入现场，旧冻结版本保持原样。

工具 v3 输入冻结 SHA256：`234954010f2c484d24041ceabec2d2f9d8330d1aaf466839aecdd6b094c2f641`。独立审阅 run：`20261108-002548-codex-e49a96`；session：`01a1b6d4-9b45-7bb3-b51d-4ce5e16e2d39`。Astra 的历史点名不是后续评审默认值；下一次评审重新读取实时 Rumo 配置。

## 本机证据索引

归档根为 `D:/Develop/Misc Workspace/archive`。本提交只保存正式状态记录；工具、测试、完整审阅及运行证据仍在本机归档，关机和迁移时应保留该目录。

| 内容 | 归档相对路径 |
| --- | --- |
| 当前总状态 | `pinfeather-m1-p2-sol-coordination/run-20261105-v1/HOT-NATIVE-BRIDGE-CURRENT-STATE.json` |
| 调查方向及可比性边界 | `pinfeather-ime-astra-convergence-review/run-20261107-v1/OWNER-DECISION.md`、`COMPARABILITY.md` |
| v3 离线实现及验证 | `pinfeather-ime-astra-convergence-review/run-20261107-v1/TOOL-V3-OWNER-RESULT.md`、`minimal-tooling-v3/evidence/FINAL-VERIFICATION.json` |
| v3 审阅裁定及跨期复现 | `pinfeather-ime-astra-convergence-review/run-20261107-v1/tool-review-v3/OWNER-JUDGMENT.md`、`OWNER-DECISION.json`、`OWNER-READINESS-PROBES.json` |
| 当前 IME 续接入口 | `pinfeather-m1-ime-pending-diagnosis/run-20261107-v1/owner/CURRENT-STATUS.md` |

## 续接边界

下一步仅修正首次进程查询前与实际启动前的两处新鲜度检查，并补充假时钟跨期测试；先保留失败回归，再做离线修正。保留永久登记、失败不自动重试及现有证据协议，不新增产品改动或原生探针。修正尚未开始。

真实 Windows 启动、身份、管道、退出、Runtime 映像观察及两候选 IME 行为仍未验证。实际运行需新的具体范围、简短操作卡和用户当时就绪，旧场范围及就绪不能复用。休息期间不派工、不启动产品、不执行新测试或测量。

本次仅本地提交本检查点和前一检查点的续接链接，不推送。原有 18 项修改、19 项未跟踪文件及原诊断工作树保留；不将原代码、debug.log、副本、数据库或归档脚本纳入提交。已完成的审阅与合成复现均已退出。
