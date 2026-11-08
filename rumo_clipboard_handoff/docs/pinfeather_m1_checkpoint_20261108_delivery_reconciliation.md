# M1 本地检查点：诊断收敛与有限交付来源

本记录承接 [IME 工具检查点](pinfeather_m1_checkpoint_20261108_ime.md)。本机受限手动版、完整 M1/T5 和完整性能验收均未完成。普通产品候选与诊断成品分别绑定，单次正向观察不替代产品验收。

## 当前结论

- 最新 Astra 单轮只读评审未报告本轮范围内 P0–P3 问题：离线工具窄契约收敛，停止继续扩建诊断；IME 修复和产品验收仍未成立。保留原失败场、pending、claim 和未知项，不自动追加探针、Runtime 调查或重试。
- 普通产品基线仍为 `0fd8848085ca49f025935a993c0dd9dda050227ccb73c02318a59477521a8e90`，12,386,304 bytes。历史构建是 `a0add666` 加20项已接受输入的 dirty build；175项来源与历史副本、当前 clean `e410a452` 工作树原字节匹配，不能称为 clean e410 重建。
- 最新 `4cc7470438685c4602fd5a1e82ae3788c8047ecc5d6ab2e382b9d9183d3ef29f` 是启用 `ime-diagnostic` 的诊断成品，12,506,624 bytes，不作为普通交付候选。它的启动构建错误已修正并通过一次零输入启动/明确退出验证；该结果不证明普通输入问题修复。
- 普通175来源到事件诊断186输入有6项原文件变化和11项新增，主要为事件观察和诊断接线；输入类只增加只读快照方法，`acknowledge`、`observeIme`、`compositionEnd` 的判定未改变。焦点诊断212到修正构建216输入，原212项逐字相同。不存在可直接称为普通版 IME 修复的产品补丁。

## 已有现场与验证边界

| 范围 | 结果与限制 |
| --- | --- |
| 普通候选 V5 | 中文首段出现左上角候选，选择后点击完成仍等待；原场未通过，分页、换查询清状态和明确复制等后续项未执行。旧失败不改判。 |
| 最新诊断 IME 场 | 用户报告候选在搜索框附近、得到“样本”、诊断完成且无额外动作；254行终段 ready，完成回报已绑定。可信 DOM/application end 均 false，来源未定，不证明原生 IME 结束或因果修复。 |
| 最新诊断场收尾 | 采集器以 `identity-diagnostics` 停止，原 tail=false、根退出码未知。用户退出后仅复查已记录12身份，均消失；不能补造原退出码、尾部排空或完整后代关闭。 |
| 错误保全 | 新隔离工具保存已有内部拒绝原因，27项输入冻结；最终离线201项测试无失败、无跳过，两项就绪错误变体被拦截。它不放宽身份门，没有新增原生API或产品行为。新工具未现场运行。 |
| 性能与历史功能 | 原分项收录、搜索、显式复制、限定热测量及 Store 三档结果继续按各自候选和范围保留；不外推为当前普通候选全链路、100k产品支持、完整P1–P4或首轮完成。 |

`UnexpectedException` 在本场对应本地 `Stop(identity-diagnostics)` 的精确类型白名单编码，不足以认定 Windows 抛出未知异常。旧回执没有保存具体 observer issue，无法离线追回；新导出只防止以后再次丢失已有原因。

## 后续规划仍需决定

长期 Windows 优先、本地剪贴板主体及后续集合/模板/CLI/MCP/同步目标保留。首轮原自动回贴目标与当前手动版分开：当前闭环是完成输入、独立浏览、明确复制及用户手动粘贴；完整自动回贴、完整性能和日常稳定版没有验收结论。

“明确确认可见搜索词，再独立浏览”的交互仅为待决定提案，尚未批准、实现或构建。它会改变输入完成契约，不能将用户询问方案或继续离线资料收口视为授权。后续项目规划须重新权衡原快捷体验、现有两步流程、待解决普通版输入问题和验收成本，不预设采用该提案。

普通版输入阻塞及后续缺口未解除，当前不签新现场范围、不要求当场就绪、不原样重复V5。未来产品实现、构建及现场按具体决定执行；旧范围和就绪不复用。错正文、非明确写入、隐私泄露、静默丢失、退出保存假成功等义务继续阻塞交付。

## 本机证据索引

下表相对 `D:/Develop/Misc Workspace/archive`。Git 检查点不包含归档源码、工具、EXE、原始日志或数据；迁移机器须另行保留归档。

| 内容 | 归档相对路径 |
| --- | --- |
| 总状态 | `pinfeather-m1-p2-sol-coordination/run-20261105-v1/HOT-NATIVE-BRIDGE-CURRENT-STATE.json` |
| 普通候选与构建 | `pinfeather-m1-p2-release-candidate/run-20261104-v1/BUILD-IDENTITY.json`、`SOURCE-MANIFEST.json` |
| 普通V5失败 | `pinfeather-local-manual-delivery/run-20261107-v1/owner/ACTUAL-V5-RESULT.md` |
| 最新诊断包启动接受 | `pinfeather-ime-build-url-fix/run-20261108-v1/field-control-20261108-v1/OWNER-FIELD-RESULT.json` |
| 最新IME场 | `pinfeather-ime-collector-errors/run-20261108-v1/field-control-20261108-v1/OWNER-FIELD-RESULT.json` |
| 错误保全与冻结 | `pinfeather-ime-observer-issues/run-20261108-v1/ACCEPTANCE.json`、`tooling/INPUT-FREEZE.json` |
| Astra收敛裁定 | `pinfeather-ime-astra-final-review/run-20261108-v1/OWNER-JUDGMENT.md`、`OWNER-DECISION.json` |
| 有限交付静态核对 | `pinfeather-local-manual-delivery/run-20261107-v1/reconciliation-20261108-v1/DELIVERY-READINESS.md`、`RESULT.json`、`INVENTORY-AFTER.json` |
| 待决定输入方案 | 上述 reconciliation 目录的 `NEXT-PRODUCT-STEP.md` |

本检查点仅保存状态和证据入口。原18项修改、19个未跟踪名称和原诊断工作树保留；不将它们、真实数据库或归档工具混入本次文档提交。后续规划评审另存归档，不在本记录中预填结论。
