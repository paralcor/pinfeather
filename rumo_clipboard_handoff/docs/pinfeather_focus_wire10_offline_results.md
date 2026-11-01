# 焦点补读 wire10 离线结果

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

下一步离线补齐独立 focus-only 单次 owned 父流程、预算/取消/退出结算、raw/SHA 绑定、同元素引用与逆序释放对照，再封完整包。焦点诊断中合法但 unattributed 的记录只能保留诊断证据，不能沿用 I00 的接受状态或产生 IME/回贴许可。

当前没有完整新现场执行包、批准或“已准备”。未来仍须先冻结确切包与读取范围，再单独取得新批准和独立新准备确认；不自动重复 I00 或更换目标。目标历史 PID/创建时间不代表当前存活。

## 本地证据

归档：`D:\Develop\Misc Workspace\archive\pinfeather-focus-wire-bound\run-20261101-085955-codex-01a19233`。

- `REPORT.md`、`WIRE10-CONTRACT.md`、`BOUND-PROOF.md`：结论、线格式与大小边界。
- `RED-BOUND.log`、`RED-RID.log`、`RED-SKIP-CONTRADICTIONS.log`、`RED-REASON-CANONICAL.log`：修复前反例。
- `FINAL-BOUND.log`、`GREEN-RID.log`、`WIRE-TESTS.json`、`MUTATIONS.json`：实际离线运行结果。
- `SOURCE-PINS.json`、`CHANGES.diff`、`PROVENANCE-VERIFIED.json`、`CHECKSUMS.json`：来源、差异及冻结摘要。

本轮重新核对原交接 426 项和 v14 冻结 2901 项的字节数/SHA256，全部匹配，原 ACK 保持不变。实验源码与二进制仅在归档，正式文档提交不代表诊断实现已进入产品。
