# P2 可交互就绪测量契约（schema 1）

本入口只准备冷启动 10 次、热键 50 次现场测量。源码自动测试不代表真实性能或新 Release 候选已验收。普通运行完全关闭测量：不创建 recorder、不写证据文件、不发测量 IPC；旧 PanelSnapshot、ListClips、ready_manual_panel、复制和数据库契约不变。

## 显式启动

宿主必须同时收到 `--measure-run-id`、`--measure-output`、`--measure-cold-start-qpc`、`--measure-qpc-frequency`，并提供原有 `--data-dir` 和显式 `--start-paused`。四个测量参数全无即关闭；缺项、重复、非法值、混入 worker 模式均拒绝。run-id 为 1–64 个 ASCII 字母、数字、下划线或短横线；QPC 与频率为规范十进制正整数，频率须等于本机 QueryPerformanceFrequency，冷起点不得在宿主当前 QPC 之后。

输出为规范绝对文件路径；父目录必须已存在且沿途无 reparse point，输出不得位于 data-dir 内。使用 create_new 拒绝覆盖，不创建父目录；初始化失败拒绝测量启动。证据不写数据库。宿主只在启用时向 main / quick-panel 注入不可写、不可配置且冻结的 `__PINFEATHER_MEASUREMENT__ = {schema:1, enabled:true}`，不含路径或权限秘密。每条命令仍独立验证 Tauri 注入的真实窗口标签及本地页面来源。

`tools/measure_launch.py` 使用 Python 标准库，在 Popen 前读取 Windows QPC 并传入上述参数。必须指定 EXE、隔离数据目录、全新 output、全新 launch-receipt 和 run-id。launch-receipt 保存候选 SHA256、Popen 来源、PID 与时钟口径；不重试，不发送热键，不操作剪贴板。失败留下部分证据，不能把空的启动回执当作启动成功。本轮只使用 mock Popen 测试。

## 独立 IPC

各请求使用单个 `request` 对象，严格拒绝未知字段、缺字段、错误类型、非法枚举与超界整数。跨端标识、代次和序号均为规范十进制字符串，最大 u64；schema 是数字 1。session 必须显式为 main 的 null 或 panel 的 32 位小写十六进制当前会话。

| 命令 | request / response |
| --- | --- |
| measure_start_model | `{schema:1}` → Identity |
| measure_invalidate | Identity + generation + sequence → null |
| measure_stop_model | Identity → null |
| measure_list_clips | Identity + generation + sequence + query_request（原 ListClipsRequest） → `{result: 原 ListClipsResult, receipt: Receipt或null}` |
| measure_ready | Receipt + input_focused（boolean） → null |

Identity 精确字段为 schema、run_id、attempt、window、session、model。Receipt 另加 generation（可为 0）、sequence、receipt（宿主发行的不透明结果 ID）、content（empty / nonempty）。除 generation 外的十进制身份均大于 0。首页每次失效必须有严格递增 sequence，generation 不可回退；同 generation 的 refresh/status 补偿也会更换首页版本。分页沿用首页版本，不发行新的首次就绪凭据。

main 由宿主建立独立 cold attempt 和窗口 lifetime；panel 由真实热键 attempt 绑定原有 session。每次 model start 都发行新 model 身份。注册、失效、首页开始和实际查询成功完成由同一宿主状态机验证；列表命令复用 HostRuntime.list、原 parser 与原存储，不复制 SQL。新查询、refresh、history/status 补偿、stop、隐藏、销毁、退出、查询失败均使旧凭据失效。旧响应和重复首页不能发行当前凭据。

首页或分页查询失败仅能终结仍由该请求的 attempt/window/session/model 及 generation/sequence 共同拥有的当前样本；身份和首页版本检查与终结在同一宿主锁内完成。旧请求失败不得清除后继 receipt，也不得给后继样本追加 query_failed。

前端只有成功首页的 receipt 可进入 model 提交状态。React effect 在对应结果已提交且 loading=false/error=null 后检查输入、文档和结果条件，并在同一 JS turn 重新核对当前 model/代次/版本后立即 invoke ACK。默认关闭或测量终止后的普通列表没有 receipt，不能产生样本。测量截止、写出失败或终止不会阻断普通历史查询或热键窗口调度。

## 终点与生命周期

宿主在接收 ACK 时先取 QPC，再做记录 I/O。原生状态直接查询窗口存在、可见、非最小化、当前前台原生焦点，查询失败拒绝成功；不持测量锁等待主线程派发。panel ACK 还经原 PanelSessions 的当前 Ready 会话锁接纳；隐藏/退出先使测量失效再执行原生效果，保持相同串行顺序。

宿主先核对完整当前 receipt；伪造、过时、错窗口或错会话提交不能触发当前样本的原生探测或终结。有效 receipt 在原生检查时不可交互，立即记录唯一 terminal invalid/window_not_interactive；原生探测失败记录 invalid/native_probe_failed；panel 输入未聚焦记录 invalid/input_not_focused。早 ACK 候选在 native_entry_ready 时遇到同类问题也终结。后续恢复焦点不能将该 attempt 变成成功，不自动重试、重建样本、show/focus 或增加定时器。输出/flush/时钟错误仍粘性失败；不能把未成功写出的 invalid 当成已记录。

前端只记录 receipt 已提交，不把 invoke 发出视为宿主已接受 ACK。每个 receipt 最多一次在途提交；拒绝后仅清除仍属该次 model lifetime 和当前版本的本地 receipt，不清除后继结果，也不重试。未知传输失败无法证明宿主已记录 invalid，结果保持未确认，不合成为成功或已落盘终结。

文档必须有焦点；搜索框必须已连接、可见、启用、非只读。main 不要求 activeElement 是搜索框，也不新增自动 focus，记录 input_focused 实况。panel 必须保持原有搜索框实际焦点和输入可操作状态，测量本身不 show/focus。nonempty 首批结果必须已有可用选择按钮；真 empty 可以记录，但不能计作正式非空负载样本。初始化空数组或错误空状态不能就绪。

native_entry_ready 仅在热键初始化和 residency/tray 安装处理完成后、两者均成功时记录；失败记录 native_entry_degraded 并否决 cold。main React ACK 与 native_entry_ready 两种先后顺序均支持；早 ACK 只保存候选，待入口完成时重新核对窗口状态，以较晚的宿主 QPC 完成，期间失效、30 秒截止或失败均否决。完成最多一次，不被后续事件复活。

热起点名称为 native_hotkey_dequeued：匹配 WM_HOTKEY 从原生消息队列取出后立即读 QPC。它不覆盖物理按键至消息出队前的时间。原 32 条轮询限制保留，每条匹配消息都有独立 attempt；同批额外消息记录 coalesced，dispatch_pending 抑制的消息记录 dispatch_busy。真正接纳的首条身份带到主线程并绑定新会话；派发/打开失败明确 invalid。普通模式仍使用原 bool 合并调度。

## NDJSON 与失败

UTF-8 NDJSON 首条 run_start 包含 schema=1、run_id、sequence、event、qpc、frequency、cold_start_qpc。随后记录 model_start/model_stop、native_hotkey_dequeued、result_receipt、react_commit_ack、main_interactive_ready/panel_interactive_ready、native_entry_ready/native_entry_degraded、invalid 和 run_end。事件按宿主接纳 sequence 排列；热键起点的 qpc 保留消息出队值，因此跨线程接纳顺序不表示 tick 排序。关联字段只含不透明 attempt/window/model/result、empty/nonempty、input_focused 和固定原因；不含正文、查询原文、clip ID、标题、命令行或数据路径。

总运行最多 1800 秒，每 attempt 最多 30 秒，最多 4096 事件（提前预留终结空间）。在事件和收尾处检查，不增加安静窗口轮询。超时或上限写 invalid 与 complete=false 的 run_end，停止测量接纳。正常收尾写唯一 run_end；未完成 attempt 明确 invalid/run_closed。complete 只表示事件流完整，不能替代成功非空样本数。

每条记录写入并 flush 后才认可证据；写入/flush 或时钟失败为粘性错误，不能继续产生成功证据。错误使用不带个人内容的类型值：disabled、invalid_contract、wrong_window、stale、not_interactive、expired、clock、output、limit、stopped。文件截断、缺完整 run_end、complete=false、时钟错误、丢事件、写出失败均使整次 run 不完整，部分文件保留、不覆盖、不重试。统计必须列出全部尝试及原因，只用有效 nonempty 终点，不能隐藏合并/忙碌样本。

新前后端须随同一个产品包原子交付。旧前端忽略启用标志，只留下无终点的不完整测量；旧宿主拒绝新 CLI 参数。混搭版本不用于认定测量成功。共享正反 fixture 位于 tests/fixtures/contracts/measurement.json，旧 DTO 的严格解析器不放宽。
