# IME schema9 I00：离线修复与单次现场结果

记录日期：2026-11-01（UTC+08:00）。本记录覆盖归档中的独立诊断包，不代表诊断代码已集成产品，也不解除自动回贴限制。F2 仍为 partial，首轮基础完成度维持 4/7，`automatic_paste_allowed=false`。

## 结论

v13 单次 I00 启动一个原生诊断进程，取得有效 schema9 协议记录，但未取得有效焦点归属。首次 UIA 元素的键盘焦点属性为 false，第二次为 true；`CompareElements` 调用成功，但比较结果为不同元素。归属拒绝原因是 `element_keyboard_focus_false`，父流程返回 `recorded_without_valid_observation`。

本次没有调用 TextEditPattern、GetActiveComposition 或 GetConversionTarget，没有形成 IME 组词判断。不能把未调用的范围查询解释为 null，也不能由此得出 NoComposition。证据不能确定首次焦点为 false 的原因，不能归因于用户操作、provider、缓存或采样时序中的任一项。

## 单次事务与身份

| 事务 | 已核实结果 | 边界 |
| --- | --- | --- |
| v12 I00 | 打开既定 PID 33428 时，OpenProcess 返回 Win32 错误 87；目标访问调用 1 次，原生探针 0 次 | 未确认目标当前存活状态；不据此断言进程退出原因 |
| 目标身份更新 v2 | 选择流程 1 次，记录 Edge 156.0.4314.58 x64，PID 41124，创建时间 134379340914883903；IME 探针 0 次 | 只确认该次进程与磁盘版本快照；活跃 IME/provider 未查询 |
| v13 I00 | 使用新目标、独立批准与准备消息；人工准备等待 8016ms，原生探针 1 次 | 协议有效，焦点归属失败；不是有效 idle 或组词样本 |

v13 scope 为 `pinfeather-f2-ime-I00-v13`，round 为 `53c0c1f3-ebd4-4558-b2a8-98d4d9dfaebe`。批准原时间为 `2026-10-31T16:58:23.071803+00:00`，独立准备原时间为 `2026-10-31T16:59:10.596393+00:00`，均保留原字节与摘要。

## v13 观测与退出证据

| 字段 | 记录值 |
| --- | --- |
| Win32 前后目标身份匹配、锚点稳定 | true |
| UIA 父关系遍历 | 9 层，到达锚点；上限 32 层 |
| 首次、第二次 HasKeyboardFocus | false、true |
| CompareElements HRESULT、same | 0x00000000、false |
| protocol_valid、attributed | true、false |
| TextEdit、composition、conversion invoked | 均为 false |
| begin、result、projection 字节数 | 49、1808、753，均不超过 4096 |
| 原生进程 | PID 47204，创建时间 134379395596143896 |
| 自持进程退出证据 | 已确认退出，退出码 0，未强制终止 |
| 正常原生资源清理已验证 | false；进程退出不能替代资源清理验收 |

原生 `r2e_model.h:198` 在首次元素没有键盘焦点时拒绝归属；`uia_r2e_diag.cpp:373–374` 仅在初始归属成功后进入 TextEdit 查询。父流程 `runner.py:527–530` 在 `attributed=false` 时拒绝接受该次观测。这些拒绝条件未放宽。

## 离线与包证据

v10 修复严格消费者的 skip 互斥约束，v11 修正历史 `prior_source` 摘要。原 MiMo/Gemini 分轮源码复评已完成，两项 P2 闭环，保留两项非阻断 P3：畸形 RID 的结构化错误报告、usage 中的旧 schema 提示。评审未独立证明 ZIP 或现场验收；旧报告不计作 v13 新两路评审。

v13 保持 15 个原生资产与 v12 逐字节相同，controller 不变，runner 仅更换 scope/round，candidate contract 仅更换真实元数据摘要。新目标的 profile 和派生清单从已封存元数据生成。旧探针输出、夹具和现场证据保留原样；新副本中的合成夹具重绑定身份，不声称重新运行了原生 serializer。

| 实际检查 | 结果 |
| --- | --- |
| 新目标绑定回归 | 改前 3 项断言失败，改后 3 项通过；覆盖新证据绑定和旧 metadata 拒绝 |
| guarded offline_suite.py | 160 方法，0 失败、错误、跳过；阻止真实 DLL 加载及原生探针启动 |
| packager.py / verify_artifact.py | ZIP 31 成员、30 内部摘要、28 runtime；提取字节一致，6 项组装检查和 6 个坏 gate 变体拒绝通过 |
| 现场后完整性核对 | 原冻结 2342 项中，2341 项保持原摘要，仅 gate 发生已授权流程的预期变更；原 ZIP 不变 |

以上离线检查没有替代现场焦点归属、provider、IME 状态或正常原生资源清理验收。

## 本机证据索引

原始材料位于 `D:/Develop/Misc Workspace/archive/`，未随本文提交诊断脚本、EXE、原始日志或 ZIP。

| 材料 | archive 下的任务目录 |
| --- | --- |
| v11 修复、评审记录 | `pinfeather-ime-provenance-fix/run-20261031-232958-codex-01a19233/` |
| v12 目标打开失败 | `pinfeather-ime-approval-rearm/run-20261101-002346-codex-01a19233/` |
| 目标身份更新 v2 | `pinfeather-ime-target-refresh-owner/run-20261101-003557-codex-01a19233/` |
| v13 离线包及现场结果 | `pinfeather-ime-target-rebound/run-20261101-004433-codex-01a19233/` |

v13 目录中的 `FIELD-RESULT.json` 和 `FIELD-EVIDENCE-CHECKSUMS.json` 封存结果及 68 个现场文件；原始观测位于 `delivery-final/attempts/71c2fe67-f534-43d2-bbfc-2bfe33fe9d4b-0310a8d9d5c54fa98c7dafa679abc759/`。

| 摘要对象 | SHA256 |
| --- | --- |
| v13 ZIP | `8BB3B2C47934FA4D2679B97FF4E2CAA3F11551710F5FB7FD79D4CFE77CFA157C` |
| v13 原生 result.json | `C5F51095F56F0AF2DD4CE4E98F2F330948EF92242F0DFF499CBEE7D0C71743A0` |
| v13 现场证据清单 | `1D60A055804734EA51DD977CE74A74139B94E13B4F28477478E9E221A31ECD1C` |

## 最终状态接受修复：v14 离线候选

离线对照发现父流程的接受缺口：首次焦点归属成功、随后 UIA 元素改变时，严格消费者正确给出 `protocol_valid=true`、`attributed=true`、原生最终状态 `stale`，旧父流程仍返回 `metadata_recorded_awaiting_human_phase`。协议合法只表示字段与派生事实自洽，不能替代最终观测状态。此问题由合成夹具通过真实 PS 消费者、Python adapter 和父流程复现，不是新增现场样本；v13 实采因首次归属失败提前拒绝，没有触发该接受路径，自动回贴始终关闭。

v14 在同一 Python 模块内传递并检查最终状态：`check_projection` 从同一份已核对 SHA 的原始字节中读取状态，校验枚举，返回内部 `native_state`；adapter 保留该字段，父流程仅在其为 `metadata_recorded` 时接受。合法 `stale` 仍保留协议合法、归属和已确认退出等事实，结果为 `recorded_without_valid_observation`，不误报进程失控。缺失或未知状态不能成为有效观测。

此内部字段不加入外部 bridge JSON，外部注入同名字段仍因固定形状被拒绝。原生 wire 9、PS bridge wire 2、controller 和 15 个原生资产不变；adapter 与父流程位于同一文件，由整包摘要约束原子更新。旧内部 adapter 缺失状态时拒绝，不混装旧父流程。未放宽归属、身份、所有权、4KiB、32 层、IME unknown 或回贴否决规则。

新增 8 项离线检查中，修复前 5 项在接受边界断言失败；修复后完整 168 方法通过，0 失败、错误或跳过。检查覆盖正常观测、元素变化、属性变化、v13 保存 raw 的拒绝重放、状态取自绑定字节、外部字段注入、未知枚举和内部漏字段。实际 ZIP 的 31 成员、30 内部摘要、28 runtime 以及提取、默认与拒绝路径校验通过。v14 没有新增现场查询、IME 探针或外部模型评审，不代表原焦点问题已修复，也不代表产品已集成诊断代码。

v14 目录为 `pinfeather-ime-observation-state-fix/run-20261101-082009-codex-01a19233/`。最终 ZIP SHA256 为 `D0C43AD5A44B5881C26F74B527D429F10548188FF4E363127405F54113500AC3`；`RED-REPRODUCER.json`、`RED-OBSERVATION-STATE.log`、`OFFLINE-SUITE.json`、`PACKAGE-AUDIT.json` 和 `CHANGES.diff` 保留复现、修复和包证据。新 gate 关闭，没有批准、准备或 claim。

## 后续边界

三次事务均有终结记录，批准、准备和 claim 均已消耗；批准 gate 原字节作为摘要证据保留，不能复用其表面状态重开执行。没有自动重试、换目标、输入、剪贴板操作或产品集成。

下一步复用历史 R2f 的同元素补读设计，并评估外部观察路线能支持的范围。已只读核对其在持有首次元素期间补读 ControlType、IsKeyboardFocusable 和该对象的键盘焦点，并冻结补读前后 Win32 快照；补读不覆盖首次读数，也不产生 TextEdit 或范围调用。其 schema9/experiment/mode 与当前 R2e wire 不同，不能直接混装。新候选的 4KiB 最坏边界、消费者及所有权尚待离线验证，旧 R2f EXE 和批准均不能直接使用。

具体对照和执行前检查记于 v14 目录的 `FOCUS-DISCRIMINATION-PLAN.md`。现有证据不能区分用户操作、provider/cache 状态或采样时序，不足以支持通过延长等待或只采用第二个元素解除拒绝。若需要新的现场样本，须另行冻结确切 scope、round、目录、包与读取范围，再取得新批准及另一条独立准备消息；不继续盲目重复同一采样，不解除 IME 未知时禁止自动回贴的规则。
