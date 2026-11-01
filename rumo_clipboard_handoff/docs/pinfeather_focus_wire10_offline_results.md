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

## 目标身份更新 v3：离线候选

后续冻结独立 scope `pinfeather-f2-edge-target-refresh-v3`、round `7a781bff-89c4-4113-8010-93b26c7696b4`，只准备一次 Edge 进程元数据更新。查询 PS 与旧 v2 逐字节相同；新入口绑定新范围，拒绝批准/ready共用事件 ID，并将 PID 限于后续 focus 消费者的 signed31 正整数范围。所有旧许可继续消耗/失效。

42 项离线方法、10 种纯假进程 PS 场景、25 个隔离变异通过；新绑定与两项新增约束的修复前失败记录保留。实际 ZIP 的 11 成员、10 内部摘要及独立提取校验通过，另有 6 种坏绑定拒绝。包内无 EXE，提取副本默认校验后仍保持精确成员集合。

候选包为 `pinfeather-f2-edge-target-refresh-7a781bff-v3.zip`，13711 字节，SHA256 `A7FB14FA37EE63D1189C5A061794270E8A9144AD74F8411BF15C712FA7A44E6D`。本地归档为 `D:\Develop\Misc Workspace\archive\pinfeather-target-refresh-v3\run-20261101-101832-codex-01a19233`，详见 `REPORT.md`、`APPROVAL-PROPOSAL.json`、`PACKAGE-AUDIT.json` 和冻结清单。

当前待批准，无新目标查询、ready、claim或外模型评审。未来获批后最多一次 `Get-Process -Name msedge`，筛选具主窗口的唯一进程，只读身份/版本/架构等元数据；同进程内窗口/页面不额外区分，不读焦点/UIA/正文/活跃IME，不启动目标或诊断器。成功只产候选身份，随后须另封新的焦点范围并另收批准和准备，不自动衔接。批准15分钟、独立准备60秒、metadata宿主10秒加最多2秒结算的边界不变。

## 新目标身份与 F00 v2

用户随后明确变更确认方式：在确切包与范围已说明后，“已准备”同时表示批准该范围并确认准备，不再另索批准消息。此最新指令覆盖上文历史“两条独立消息”的要求；仍不允许跨范围复用、自动衔接、续期或重试。内部不同的approval/ready角色记录必须保留同一条用户原文及首次接收时间，不冒充两条独立人类消息。

按此规则，metadata v3在 `2026-11-01T02:44:42.332910+00:00` 首次本地接收观察到单条“已准备”，完成一次只读查询，记录新目标 Edge156.0.4314.58 x64、PID38152、创建时间134379716295803792、OS报告10.0.26200.0。原始response426字节，SHA256 `AF8211F9E2F70E54651D72295013B2CC3B5A1373C77D0C4EFB799B4DFD825CCF`；当次alive_at_end=true不代表未来存活。provider/活跃IME未查询。metadata进程PID43116/birth134379746824748409；成功路径要求持有对象退出和句柄关闭，不能替代原生IME清理证明。31项事务证据封存，1166项原冻结记录中仅APPROVAL发生预期转换，其余1165项不变，原ZIP不变。该scope已消耗，没有接着采焦点。

随后仅离线将新证据绑定独立 `pinfeather-f2-focus-F00-v2`，round `5446d8e0-efa2-4cfd-af57-d97c1277f03a`。METADATA-FIELD-RESULT精确摘要为 `A7A77C8EE5E4CB13CC2C2CE941635823881BFC9FB521B78BBB32AE1B93C113C5`。runner/candidate/profile及派生摘要同步，合成fixture仅重绑目标；原始raw未改。7native/source资产逐字节相同，未构建或运行实际诊断器。

新包通过137项离线方法、29种假进程/PS场景、7项隔离变异、15入口和6坏绑定拒绝；ZIP23成员/22内部摘要/20runtime及独立提取字节校验通过。包括旧v1 proof在target access前拒绝、新目标与metadata绑定、同原时的单条消息两角色路径。现有归属、三段稳定、4KiB/32层、持有进程与否决条件保持，自动回贴关闭。

最终包 `pinfeather-focus-f00-v2-5446d8e0-sealed.zip`，170607字节，SHA256 `8E092DD8BEFB24FB08584E1BF2868899859AC069108A5AD1792D510E468FC616`。归档为 `D:\Develop\Misc Workspace\archive\pinfeather-focus-rebound-v2\run-20261101-104714-codex-01a19233`，详见REPORT、PACKAGE-AUDIT、原始metadata与冻结清单。该新F00范围gate关闭，无批准/ready/claim或现场采样；metadata那条“已准备”不复用为本范围许可。说明新范围后再等新“已准备”，不会自动执行。没有外模型评审，F2partial、首轮4/7。

## F00 v2 单次结果：同一对象始终无键盘焦点

用户在新范围说明后另发“已准备”，首次本地接收观察时间为 `2026-11-01T03:00:45.299243+00:00`。该单条消息同时批准并确认准备，两个内部角色保留同源原时。父流程实际等待8000ms，启动一个native探针；controller返回0，结果为 `focus_diagnostic_recorded_unattributed`。这是可记录的焦点诊断，首次归属仍为false，不是I00或IME验收通过。

| 实测字段 | 结果 |
| --- | --- |
| 目标PID/birth与Win32前后匹配 | 38152 / 134379716295803792，三份快照一致 |
| 两次GetFocusedElement及CompareElements | 均成功，同一对象，属性一致 |
| 首次、第二次HasKeyboardFocus | false、false，均成功且已知 |
| 同一个first补读 | ControlType=50033，IsKeyboardFocusable=false，HasKeyboardFocus=false，均HRESULT=0 |
| 父链与时间 | 9层到达窗口锚点；first/second时间字段15/47ms，native总47ms |
| 原生进程 | PID17692，birth134379756543774213；owned退出确认，退出码0，未强制终止 |
| IME与清理边界 | 此focus实验无TextEdit/IME方法；ime_judgement=none、normal_native_cleanup_verified=false、自动回贴false |

ControlType50033对应Pane（窗格），50004对应Edit，见[Microsoft类型定义](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controltype-ids)。类型只在同first补读阶段测量，不能声称在两次GetFocusedElement瞬间均分别读过ControlType。采集源码保持first独立持有，父链使用另一个current指针，补读没有改用父节点。

用户随后人工确认：等待期间看到了空白输入框内的闪烁文字光标。该观察单独保存在 `OPERATOR-OBSERVATION.json`，不覆盖raw，也不是自动DOM或精确到47ms的对时证明。现有证据同时包含可见光标的人工观察和UIA的非可聚焦Pane；不能据此认定用户未点输入框、Edge/provider缺陷或缓存/时序根因。本样本没有观察到false恢复true或两次对象变化。

原始wire10结果1716字节，SHA256 `D515C957C58E7342D46CF00131BCEEC24AC4C61E2DD84D12C843105D0099EFD5`。`FIELD-RESULT.json`、`FIELD-REPORT.md`、`FIELD-SNAPSHOT/`与63项 `FIELD-EVIDENCE-CHECKSUMS.json` 封存原始结果、投影、持有进程/claim/终结及人工观察。证据清单SHA256 `17B2807FF80324957D7FCBA514DDBC742164A569851620B0F2046ED502504AC0`。原5478项中5477项不变，唯一gate变更严格匹配批准，原ZIP不变；许可已消耗，无自动重试。

后续先离线分析UIA焦点暴露与页面输入焦点的区分办法，不重复相同采样、不放宽首次否决。假设边界和D2状态见归档 `ELEMENT-INTERPRETATION.md`、`FOCUS-FAILURE-ANALYSIS.md`。尚未形成可靠IME安全判定，F2partial、首轮4/7、自动回贴关闭。
