# 焦点补读 wire10 与 F00 离线结果

更新：2026-11-01。F2 仍为 partial，首轮基础 4/7，自动回贴关闭。本次验证限独立诊断源码与协议消费者，没有新现场采样，也未集成产品。

[v13 I00](pinfeather_ime_i00_schema9_results.md) 的首次焦点归属未通过，后续选择复用历史 R2f 的“对同一个首次元素补读”路径，区分对象属性随时间变化与两次 focused element 不同。首次读数、基线归属与补读分别保存；补读 true 不能取消首次 false 的否决。本次完成该路径的输出大小与严格消费者验证。

## 大小与版本

原 R2f expanded schema9 的有效 32 层纯模型输出为 9315 字节，原消费者判协议合法，超过要求的 4096 字节。新 R2f compact wire10 用固定 tuple 无损保留 getter 的 invoked/HRESULT/known/value、两次元素、最多 32 层父链、三份 Win32 快照、四项释放声明与独立补读时间；超限拒绝，不截断。

新有效宽字段样例为 3851 字节。结合固定 Windows x64 字段宽度和采集分支，UIA 路径保守包络为 4023 字节，无 UIA 路径为 2176 字节；另有独立的最终 4096 字节保护。4023 字节包络不是合法观测，3851 也不是所有模型的最大值。

本次只改变 R2f 焦点诊断 wire；R2e 的 compact schema9 保持原样。新旧 R2f 的 experiment/mode 相同，消费者必须连同版本一起识别。旧证据与旧消费者保留，新旧混配均拒绝，没有转换历史 raw。

## 验证与修复

实际运行的新建纯模型/serializer helper 和读取 fixture 的 PowerShell 消费者得到：

- 4 项大小/超限方法通过，旧实现对应的 3 项失败反例保留。
- 24 组新旧 producer 对照的逻辑字段全部一致。
- 新消费者 27 个正例通过、44 个反例拒绝；四种 producer/consumer 版本配对符合预期。
- 4 个隔离消费者变异全部被测试抓住，覆盖字节上限、版本、skip 耦合与身份原因。
- 4 项 RID 模型对照通过。旧模型会把负 RID 判普通权限；新生产者与消费者同步改为未知权限，采集入口在 UIA 前跳过。

消费者另补齐 skip 与元素采集、父链、比较、释放声明的矛盾检查，独立重算身份原因，并精确校验状态等枚举的大小写。首次 false、同一元素补读 true 的对照仍为 unattributed；失败或未知 getter 不转换为 false。

实际诊断器只编译到 OBJ，没有链接或运行新诊断 EXE。helper 不含 Windows/UIA 采集入口；导入表检查未见 UIA/COM/user32/目标查询/输入/剪贴板采集导入。编译和模型释放账本不证明真实采集及原生清理通过。本次未进行外模型评审，历史 v11 的两路结论不计入本次。

## 未完成范围

wire10 阶段之后需要补齐独立 focus-only 单次 owned 父流程、预算/取消/退出结算、raw/SHA 绑定并封包；后续 F00 的完成范围见下节。焦点诊断中合法但 unattributed 的记录只能保留诊断证据，不能沿用 I00 的接受状态或产生 IME/回贴许可。同元素引用与逆序释放的本轮证明仍限实际源码和编译，没有真实 COM 验证。

wire10 检查点没有完整执行包；后续已封存默认关闭的 F00 包，仍没有新批准或“已准备”。未来须对确切新包与读取范围单独取得批准和独立准备确认；不自动重复 I00 或更换目标。目标历史 PID/创建时间不代表当前存活。

## 本地证据

归档：`D:\Develop\Misc Workspace\archive\pinfeather-focus-wire-bound\run-20261101-085955-codex-01a19233`。

- `REPORT.md`、`WIRE10-CONTRACT.md`、`BOUND-PROOF.md`：结论、线格式与大小边界。
- `RED-BOUND.log`、`RED-RID.log`、`RED-SKIP-CONTRADICTIONS.log`、`RED-REASON-CANONICAL.log`：修复前反例。
- `FINAL-BOUND.log`、`GREEN-RID.log`、`WIRE-TESTS.json`、`MUTATIONS.json`：实际离线运行结果。
- `SOURCE-PINS.json`、`CHANGES.diff`、`PROVENANCE-VERIFIED.json`、`CHECKSUMS.json`：来源、差异及冻结摘要。

本轮重新核对原交接 426 项和 v14 冻结 2901 项的字节数/SHA256，全部匹配，原 ACK 保持不变。实验源码与二进制仅在归档，正式文档提交不代表诊断实现已进入产品。

## F00：独立父流程与封包

后续在新归档副本复用 v14 的许可、独占 claim、launch intent、持有进程、退出结算和未结算阻止重启机制，绑定独立 case `F00`、scope `pinfeather-f2-focus-F00-v1`、round `922dfa01-805f-4648-a0c6-b51d4c39c9e6`。R2f10 严格消费者、decoder、实际诊断器和父流程共同绑定摘要，没有继承 I00 许可。

父流程现在分别记录 `focus_diagnostic_recorded_attributed` 和 `focus_diagnostic_recorded_unattributed`。后者保留首次归属 false，仅用于诊断，不能通过 I00 或形成 IME/回贴判断。两者均要求协议有效、正常 owned 退出、父子非零身份匹配，以及基线前后、补读前后的窗口稳定；第三快照漂移时拒绝，即使原生最终状态因优先级仍为 unattributed。

取消前未启动子进程时释放目标并记录取消；启动期间取消或退出不明保留审计标记并阻止再次启动。ready 事件 UUID 必须区别于批准 UUID。合法 skip 的空锚点不再被误写成“进程退出不明”，同时仍拒绝成为焦点观测；可记录状态的身份条件没有放宽。目标释放或摘要写入失败会撤销两种可记录状态。

实际最终验证为 133 项离线方法，0 失败、错误、跳过；包含 29 种真实 PS bridge/消费者与假进程场景、6 个隔离变异、15 项默认/拒绝入口检查及 6 种坏绑定文件拒绝。first false→同 first 补读 true、second 不同、未知/失败 getter、第三快照漂移和空锚点 skip 均有完整假链对照。上述数量分别计证据，不能累加为现场通过数。

新实际诊断 EXE 已重新编译/链接，静态 PE 为 x64；从未运行，包括 help/default。实际采集主体沿用固定 wire10 路径，same-first 持有及逆序释放的证据仍是实际源码和编译。进程退出/释放账本不替代正常 COM 清理证明，`normal_native_cleanup_verified=false`。

首包检查发现默认 Python 入口会生成缓存，且首版审计未在入口后复核成员。已关闭入口的字节码缓存写入，在新的 `delivery-sealed` 根重建；最终包检查前后均保持精确成员集合。首包和失败记录保留，不作为最终交付物。

最终 ZIP 为 23 成员、22 内部摘要、20 runtime、7 native/source 资产，只含一个实际诊断 EXE；独立提取与逐字节对照通过。包名 `pinfeather-focus-f00-922dfa01-sealed.zip`，170070 字节，SHA256 `6F3D781690C1F10AE4B5592C70C449E1F95F3B22CA71B86245A34EAB333C43D0`。

归档：`D:\Develop\Misc Workspace\archive\pinfeather-focus-parent-package\run-20261101-094210-codex-01a19233`，详见 `REPORT.md`、`OFFLINE-SUITE.json`、`MUTATIONS.json`、`PACKAGE-AUDIT.json` 和冻结清单。离线封存时 gate 关闭，无批准、ready 或 claim；离线阶段现场查询和探针运行均为 0，无外模型评审。F2 partial、首轮 4/7、自动回贴关闭。后续单次事务见下节，不能复用历史许可或自动重复 I00。

## F00 v1 单次事务：目标打开失败

用户随后批准确切 F00 v1 包，并另发独立“已准备”。批准首次本地接收观察时间为 `2026-11-01T02:10:18.047556+00:00`，准备为 `2026-11-01T02:12:54.970694+00:00`；记录没有伪装为平台消息时间或续期。scope/round/包/目标绑定保持原样。

controller 在 `target_access` 阶段打开固定历史 PID `41124` 时，`OpenProcess` 返回 Win32 错误 `87`。目标打开调用 1 次，诊断探针启动 0 次，焦点/UIA 查询 0 次；尚未进入 8 秒人工准备阶段，没有新的焦点或 IME 样本。失败未证明该进程当时是否存活，也不能确定进程退出原因。

本次返回码为 2，事务状态 `stopped_target_access_no_diagnostic`。批准、ready、arming 和 scope claim 已消耗，controller 终结记录为 `consumed_closed`。原始 approved gate 字节保留作摘要证据，其表面 `field_authorized=true` 不能重新授权执行。没有自动重试或更换目标。

封存 11561 项中，11560 项保持原字节/SHA256，仅执行目录 gate 发生严格匹配批准的预期转换；原 ZIP 不变。`FIELD-RESULT.json`、`FIELD-REPORT.md`、`FIELD-SNAPSHOT/` 与 `FIELD-EVIDENCE-CHECKSUMS.json` 保存 42 项证据，清单 SHA256 为 `B11E17829C7F82065BF0F89BBBB4C7ED80D78CBDD1CECAB3B1D04D6A58E4B081`。原交接 426 项重新匹配。

下次应先在另行授权的只读范围内更新目标身份，再依据新证据重封新的焦点诊断范围；本次许可不复用，不继续对历史 PID 尝试。F2 仍 partial、首轮 4/7、自动回贴关闭。
