# 片羽首轮执行任务卡

版本：v1，更新于2026-10-24。E07/T2基础收录、E08查询契约及E09/T3基础搜索/预览已验收，真实宿主结果见[验收计划6.2节](pinfeather_acceptance_plan.md#62-e09-本机搜索与正文预览)。当前Codex唯一负责协调、独立审查和验收，实现按明确任务交给Mimo。E03b三目标外部IMM方法不可用，IME子关卡needs_plan_revision；用户已批准独立W1/W2先行，W3自动回贴继续阻塞。不把W0、完整R6或首轮标为完成。

总设计：[完整方案](pinfeather_implementation_plan.md)。结果判定：[验证与验收](pinfeather_acceptance_plan.md)。产品契约：[MVP](rumo_clipboard_mvp_specification.md)。E03a历史F2/F3修复记录见archive/runs/20261023-e03a-codex-owner-fixes；当时的npm 28项、Rust lib 34项及worker 37项仅是该阶段计数。当前状态以本页顶部及验收结果为准，后续E07真实复制和E09真实宿主搜索已有独立证据。

## 1. 角色、权限与推进规则

- 当前Codex：唯一负责首轮协调、分工、独立审查、验收与状态维护；按用户明确授权执行提交或推送。
- Mimo：仅按当前Codex明确分配的范围实现，完成后停写等待独立审查；Gemini负责明确分配的合成夹具任务。空闲会话不自行恢复旧任务，同一范围只允许一个写者。
- 此前Codex：正式交接后停止派工和修改；历史报告保留实际执行身份，不改写为当前会话结果。
- 用户：审阅重要方案变化、确认所需项目依赖安装、完成真实桌面验收、决定提交/发布及重要产品变化。常规包由新 Codex 按依赖持续推进。
- 以实际会话权限及用户对具体agent、范围的明确授权为准；不默认提权、不改系统策略、不读取凭据定位模型。可见模型身份不明确时只核实身份，不默默切模型。
- 一次一个工作包；每次只写当前任务涉及文件，依赖失败就停留当前任务。细节遇到不一致先记录，不自行扩大范围。
- 未获用户明确授权时，不暂存、提交、推送、开 PR 或发 issue；不 stash、reset、clean、改证书、装系统组件或删除 copy。2026-10-23 用户已授权提交并推送已完成阶段至 origin/main；两份 copy、归档数据及未完成 E04 依赖快照排除在外。
- 需要项目依赖安装时先列精确清单、用途、安装命令及peer/engine核查结果，按执行会话现有授权/审批处理；审阅本方案不等于执行了一次安装。
- 临时脚本、诊断、截图、原始数据/日志和每包执行报告放首轮archive；正式源码、测试、合成夹具和可公开验收说明放仓库。
- 浏览器仅无头Playwright。系统全局剪贴板和桌面焦点/输入验证由用户按验收步骤操作，不能让子agent自动输入。

本轮无需为了“新增代码多”先发布issue：用户已说明issue前置适用于设计方案重大调整。新 Codex 可修正实现细节；方案重大变化形成具体建议供用户决定，公开发送仍需单独授权。

## 2. 开工基线与文件保护

首轮实现接手基线为 main / `19807067dbf4b80821937514bdf06ba1a2aa1d84`。以下清单记录接手时的文件状态，不代表发布后的 Git 状态；后续执行以实际 HEAD、dirty 清单和文件 hash 为准，不能 checkout 旧基线丢弃方案。

已跟踪、接手前就有修改：

- `rumo_clipboard_handoff/docs/pinfeather_project_handoff.md`
- `rumo_clipboard_handoff/docs/rumo_clipboard_mvp_specification.md`
- `rumo_clipboard_handoff/docs/rumo_plan_queue_injection.md`

接手时未跟踪：设计目标、首轮清单及各自 copy，本文、完整方案和验收计划。正式设计与计划纳入已完成阶段发布；两个 copy 保持未跟踪。后续转交保存完整 dirty 清单/hash，接手者先比对；无法归因的新变动先核对，不覆盖用户文件。

写入白名单：当前包的新工程路径、正式测试/夹具、tools、明确指定的开发/验收文档，以及本轮交接状态/报告。既有MVP/候选F/原型/品牌资产/copy均只读；改变契约或图标设计需交回。`CONTRIBUTING.md` 的构建命令在W0/W1实际验证后可补，不能先写“已通过”。

归档根：`D:\Develop\Misc Workspace\archive\pinfeather-core-prototype\`。每包使用独立 `runs/<timestamp>-wN/`，内容含commands.log、execution.md、必要的非私密机器信息。测试新库在 `runtime-data/<run-id>/`，不复用未知旧库，不自动删旧数据。

## 3. 工作包总览与review关卡

| 包 | 包含任务 | 输出 | 交回时机 |
| --- | --- | --- | --- |
| W0 | E00–E03 | 环境与依赖清单、最小可运行壳、共享契约/失败测试、worker超时及目标/IME只读探针 | 必须先review可行性，再做W1 |
| W1 | E04–E07 | T2：真实复制到SQLite和最小列表，多格式/内部写入规则可测；用户已准许独立先行 | 自动检查和本机复制证据分别交回 |
| W2 | E08–E09 | T3：全文字面搜索、分页、分片预览和前端乱序保护 | 独立review R6 |
| W3 | E10–E11 | T4：快捷面板、热键、保守回贴状态机 | 独立review R1；真实应用由用户验收 |
| W4 | E12–E13 | T5：托盘、设置、排除、退出和恢复 | 失败/重启路径review后进入测量 |
| W5 | E14–E15 | T6：真实应用验收、release性能/内存、最终证据包 | 用户验收+Codex最终review |

W1 没有人工复制结果时标“代码待人工验收”，不能勾选 T2。Codex 可依据已通过的接口/自动证据继续独立开发，但必须保留人工待验状态。2026-10-23 用户明确批准的例外只允许 W1/W2 在工程/传输/回收关卡已通过时先行，未解决的 IME 子关卡继续阻塞 W3；不得将其他失败自动类推为可跳过。

## 4. 每个任务的固定做法

1. 读该任务引用章节与真实文件；列出本任务文件，不读全部研究材料凑上下文。
2. 先写行为断言或失败复现，执行并记录预期失败；新测试因缺模块失败时再用最小stub转到实际断言失败，不把导入错误当充分回归证明。
3. 实现最小行为，不顺手做后续包。禁止生成 `todo!()`、恒定示例返回、catch后返回成功的可调用生产入口。
4. 运行指定检查并看实际测试列表/数量；筛选结果为0不能算通过。对有风险的规则做一次受控错误实现验证测试确实失败，恢复后重跑。
5. 每步记实际改动、命令/退出码、失败原因和未验证层次。达到包终点先 review 真实 diff 并独立复跑；通过才继续，不能自行放宽验收标准。

测试资源约定：常规测试默认只用内存数据/SQLite内存库；需要文件或子进程时，必须由`PINFEATHER_TEST_DATA_ROOT`指向本次archive的独立run目录，测试只创建自身命名的新库，不扫描或清理其他目录。环境未设则该文件型用例明确失败并给出设置方法，不静默skip。

worker故障fixture作为`src-tauri/tests/support/worker_fixture.rs`，在Cargo中声明独立bin，`required-features=["test-fixtures"]`；worker_protocol这个test target也要求同一feature。只有显式测试构建启用，正常release不包含fixture。W0的worker_protocol以该fixture检验同一生产supervisor的回收路径；真正抓取worker仍固定为产品exe内部模式。native集成测试用`CARGO_BIN_EXE_pinfeather`定位Cargo构建的产品exe，不能误把cargo测试harness当成可接受`--capture-worker-v1`的产品。

执行测试前的PowerShell环境准备（由执行者在各包自己的run目录执行，不修改永久环境）：

```powershell
Set-Location -LiteralPath 'D:\Develop\orcamisk\开发\pinfeather'
$pinfeatherRunId = Get-Date -Format 'yyyyMMdd-HHmmss'
$env:PINFEATHER_TEST_DATA_ROOT = Join-Path 'D:\Develop\Misc Workspace\archive\pinfeather-core-prototype\runs' ($pinfeatherRunId + '-tests')
New-Item -ItemType Directory -Path $env:PINFEATHER_TEST_DATA_ROOT -Force | Out-Null
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
```

这段不设置系统剪贴板测试许可，也不安装依赖。`capture_windows`仍需用户单独明确启动，不能从上述目录变量推导已有该授权。

## 5. W0：工程壳与高风险可行性

### E00 · 核对身份、工作区与环境（无源码修改）

- 依赖：用户批准方案并指定W0。
- 读取：本方案入口、用户/仓库指令、CONTRIBUTING、已有preflight-report。
- 动作：记录当前执行者可见客户端/模型及非秘密 provider 证据；历史 E00 的 Claude/Mimo 身份不作为新 Codex 身份。核对 HEAD、dirty 清单/hash；查询 node/npm/rustc/cargo/MSVC/WebView2，检查 1420 端口；列精确依赖与新增文件。
- 检查：`git status --porcelain=v1 -uall`、`git diff --cached --stat`、`node --version`、`npm --version`、绝对路径`rustc --version`/`cargo --version`。Rust缺PATH只对子进程补；不重装。
- 停止：身份与要求冲突、基线无法归因、缺系统组件/权限时记录阻碍；不安装组件解决。

### E01 · 建最小Tauri/React壳与明确启动入口

- 依赖：E00通过，精确项目依赖获执行会话许可。
- 写入：package/lock、Vite/TS配置、index.html、src/main.tsx/App.tsx/样式及首批UI组件；src-tauri的Cargo/lock/build/config/main/lib、最小capabilities；tools/dev.ps1。
- 实现：显式data-dir和start-paused参数；内部worker入口必须在Tauri装配前分派；主窗口先只显示隔离目录/服务状态。使用已有PNG资源，若构建需要ICO仅工具生成衍生文件，不改源资产。先不监听真实剪贴板、不注册热键、不注入按键。
- 脚本契约：`typecheck`=应用/工具TS检查；`test`=vitest run（无测试必须失败）；`build`=TS检查+vite build；`tauri`=本地Tauri CLI。保留npm包管理，不切pnpm/yarn。
- 检查：`npm run typecheck`、`npm run build`、`npm run tauri -- build --no-bundle`；以新隔离目录/start-paused启动壳，核对WebView2实际加载和UI状态。
- 证据：精确依赖/lock、命令、实际创建的宿主及所属WebView2 PID。编译/启动结果与后续系统能力分开。

### E02 · 固定共享契约与高风险失败测试

- 依赖：E01能编译。
- 写入：contracts.rs、lib/ipc.ts、tests/fixtures、所需测试工具；只建本任务需要的类型/入口。
- 实现：完整方案第7节命令/DTO/错误、首轮常量、ID字符串/字符/字节边界；测试同份公开合成JSON的Rust解析和TS形状，不使用真实剪贴板内容。
- 测试：非法ID/limit/枚举、未知字段、超过Unicode字符数、超长frame、错协议版本/ID、正文与字节数不一致、敏感载荷不进入错误信息。
- 检查：`npm run test`；`cargo test --manifest-path src-tauri/Cargo.toml --lib`。指定测试实际存在并执行，不能空target通过。
- 停止：发现契约无法表达前端/宿主某必要行为，由当前 Codex 先整理契约修订与影响；涉及重大设计变化请用户决定，不临时加无界任意命令。

### E03a / E03b 的执行关卡

- 本次 E03a：仅实现完整方案第 5.2/5.2.1 节的传输 supervisor、受控故障 fixture 和 worker_protocol 测试。main 的产品 worker 未实现拒绝行为保留；不做目标/IME、GUI 或系统剪贴板操作。
- E03a 完成即停写交回 review。E03b 另行交付：目标/权限/IME只读探针、用户参与步骤和此前宿主关闭滞留复验。E03b 完成前 W0 不标通过。
- 本次白名单：clipboard/worker.rs、clipboard/protocol.rs、clipboard/mod.rs、Cargo.toml/Cargo.lock、tests/worker_protocol.rs、tests/support/worker_fixture.rs，以及对应正式合成测试；其他源码只读。
- Windows API 可复用已锁定且本机缓存存在的 windows-sys 0.61.2；若增加直接依赖，只能启用实际使用的 Win32 features 并离线更新 lock，不能升级其他版本或联网安装缺失依赖。缺依赖则记录精确清单交回。
- 当前新 Codex 是唯一写者，串行完成修复和验证，不自动恢复 Mimo 或派发子agent。

### E03 · 先证worker可回收与目标/IME可判定边界

- 依赖：E02；不实现完整监听/回贴。
- 写入：clipboard/worker.rs、platform/windows.rs的只读探针、tests/worker_protocol.rs、tests/support/worker_fixture.rs及其Cargo声明；fixture限test-fixtures特性，不能变成产品远程命令。
- 实现：有界frame、hello、一次在途、750ms截止、Job/终止/wait/管道回收。用不读剪贴板的测试子进程故意不回包/半回包/超长回包/崩溃。
- 用户参与：用户自己切换三类应用和输入法未提交状态；只读探针显示身份、权限及IME三态，不读取/记录文本、不自动切前台或发送按键。
- 检查：`cargo test --manifest-path src-tauri/Cargo.toml --features test-fixtures --test worker_protocol`；探针结果填验收文档V00/V01。
- 退出条件：超时后没有属于本测试的遗留worker/读取线程；未知IME明确记录。若普通目标也无法达到MVP的可自动回贴前提，停止W0，标needs_plan_revision交回。

**W0交回：** 源文件/lock及测试diff、实际编译/启动结果、只读探针配置表、worker失败/回收证据、执行身份、未验证项。用户已于 2026-10-23 明确允许独立 W1/W2 先行；IME 仍 needs_plan_revision，不开始 W3。

## 6. W1：T2复制→存储→最小列表

### E04 · 按schema v1完成事务存储

- 依赖：W0 工程/契约/传输/回收证据经复核；用户已批准 IME 阻塞期间独立 W1/W2 先行。数据库目标仍仅新隔离测试库。
- 写入：store.rs、config.rs数据目录部分、tests/store_contract.rs、合成夹具。
- 实现：单连接所属线程、版本/应用ID/非空未知库拒绝、同库多载荷事务、幂等capture_id、独立外部重复事件、条数/字节容量、级联删除。
- 测试：metadata/第二载荷/commit三处故障回滚；同条文本+HTML；重复capture不重复计量；不同capture同文不合并；Unicode摘要；高版本/无关库拒绝且文件不被清空；新测试库close/reopen一致。
- 检查：`cargo test --manifest-path src-tauri/Cargo.toml --test store_contract`。文件库只能在测试自有archive目录，默认其他测试用SQLite内存库。
- 禁止：为通过测试删除坏库、放松外键、用一对一载荷替代schema。

### E05 · 完成真实只读抓取与消息监听

- 依赖：E04通过；E03协议已验证。
- 写入：clipboard/windows.rs、capture.rs、listener.rs、worker_entry.rs、mod.rs、protocol.rs、worker.rs、runtime.rs、main.rs/lib.rs、必要 Cargo 配置及正式合成测试；tests/capture_contract.rs、worker_protocol.rs、support/worker_fixture.rs。
- 实现：隐藏窗/消息泵注册与回收；有界通知队列；worker持锁读取、尺寸/UTF16/标记校验、一致序列、owner来源；观察/抓取/排队/落盘分阶段。暂停下不读载荷。
- 测试：所有策略标记、坏值、超限、序列零/回绕/读取中变更、重复通知、5次重试截止、通知队列和快照队列各自满载；纯规则测试使用内存字节/时钟注入。
- 检查：`cargo test --manifest-path src-tauri/Cargo.toml --test capture_contract`；正常壳仍能关闭，无静默后台进程。
- 注意：不得在普通自动测试里生成系统复制或模拟输入。真实捕获由E07用户操作验证。

### E06 · 实现内部写回规则及最小存储列表IPC

- 依赖：E05。
- 写入：clipboard/mod.rs/windows.rs的内部写入、commands.rs、runtime.rs存储队列、get_status/list_clips空查询、history组件；tests/capture_windows.rs只作为后续用户启动的原生集成入口。
- 实现：来源operation登记、owner/sequence关联、各HGLOBAL所有权、部分失败不报成功；只接纳完整快照，一次事务后才发history_changed。内部写回暂不暴露任意文本命令或回贴按钮。
- 测试：marker未知/错实例/延迟通知、内部与外部同文但不同来源、标记或文本写入失败资源释放、队列条数/字节边界、容量满状态。Win32调用可注入低层故障，但状态/提交链路必须用真实生产逻辑。
- 前端：只显示真实宿主摘要/空/错误/暂停状态；离开Tauri的普通浏览器显示“需要桌面宿主”，不默认塞假记录伪装可用。
- 检查：前端test/typecheck/build；Rust lib + capture_contract/store_contract。没有成功写回证据就不显示“已复制”。

### E07 · T2装配与人工复制验证

- 依赖：E06。
- 写入：实际运行说明（docs/pinfeather_development.md）、必要装配修复；archive记录用户操作结果。
- 操作：用户在浏览器复制带编号的合成文本/链接；主动开启收录后查列表，暂停退出，再以同数据目录重开验证。文本+HTML与PNG样本按验收V02/V05验证。内部写回不需要提前暴露任意文本UI：用户可在隔离测试环境显式启动capture_windows原生测试，让同一生产监听/内部登记/写回链路验证不会自录；不由agent自动启动该系统副作用测试。
- 检查：`npm run build`、Rust相关测试、`npm run tauri -- build --no-bundle`、新exe实际启动。当前包最后一次编辑后重跑受影响检查。
- 退出：T2自动部分/人工部分各自有证据；人工未做就明确等待，不勾选T2。需要修复则留W1，不顺势开始搜索/面板。

## 7. W2：T3真实搜索与预览

### E08 · Rust正文查询、游标与分片

- 依赖：W1接口自动证据review通过，人工状态如实记录。
- 写入：store.rs查询、commands.rs、tests/search_contract.rs。
- 实现：全文字面子串、ASCII大小写、空白、特殊字符、按复制时间稳定排序、upper_id keyset、64KiB安全字符分片；不建FTS。
- 测试：验收固定R6矩阵全覆盖；摘要外命中、同毫秒排序、删除后搜索/预览、游标错query/坏id、分页中新增不混入、UTF8片段连接后等于原文。
- 检查：`cargo test --manifest-path src-tauri/Cargo.toml --test search_contract`。不能用前端筛选或只检索preview代替。

### E09 · React真实搜索与迟到结果防护

- 依赖：E08。
- 写入：ipc.ts、app-store.ts、history/quick-panel共用搜索组件、tests/frontend/search.test.tsx。
- 实现：50ms防抖、输入时立即换代、ID选中、cursor/预览失效；无结果/错误/加载状态；Command若引入则shouldFilter=false。
- 测试：Q1后发先回的Q2不能被Q1覆盖；清空、新增记录、选中项被删、预览乱序、字面%/_、正文命中摘要未命中仍可见；调用真实store/UI逻辑，仅mock宿主边界。
- 检查：`npm run test`、`npm run typecheck`、`npm run build`；无头Playwright验证可见交互。模拟宿主模式只在测试注入，不进入生产演示。
- W2交回：R6矩阵、前端乱序测试和真实宿主查询证据；性能的大规模测量留W5。

## 8. W3：T4热键、面板与回贴

### E10 · 目标会话、热键和面板生命周期

- 依赖：W2 review通过；W0目标/IME可行性结论已定。
- 写入：platform/windows.rs、paste.rs会话部分、quick-panel组件/窗口配置、tests/paste_contract.rs。
- 实现：RegisterHotKey、WinEvent观察、打开前目标身份、每次session ID、隐藏窗口重用、Esc/失焦取消、前台变化永久作废本session；未ready不接Enter。
- 测试：旧session/重复按键、target退出或handle/PID复用、意外前台切换再切回、迟到ready、目标权限未知、热键冲突；IME composition里的Enter只交输入法。
- 检查：Rust paste_contract及前端quick-panel测试；自动测试用可控时钟/目标API适配，不能让自动测试向用户窗口发Ctrl+V。

### E11 · 接入一次纯文本回贴及精确结果

- 依赖：E10。
- 写入：paste.rs、内部写回/commands.rs、结果展示；延伸paste_contract。
- 实现：严格按完整方案8.3八步顺序；300ms激活、500ms修饰键等待、IME三态、权限/焦点/剪贴板复核、一次4事件检查；不自动重发/还原。
- 测试：每个检查点目标变化；写回失败；外部复制覆盖；物理键未释放；unknown IME；SendInput0/1/2/3/4与清理边界；取消后迟到回调不再注入。测试API返回数量即可验证决策，但不外推目标消费。
- 检查：paste_contract、capture_contract及前端结果显示测试；release构建；用户在三类真实控件完成A1–A4。终端不粘贴可执行命令/换行。
- W3交回：R1全部失败状态对应证据、人工目标配置与实际消费结果；未过A1不宣称“链路已跑通”。

## 9. W4：T5驻留、设置和收尾

### E12 · 托盘、暂停/排除/热键设置

- 依赖：W3 review通过。
- 写入：lib.rs托盘、config.rs、runtime.rs策略gate、capture组件；tests/runtime_contract.rs。
- 实现：关闭窗口隐藏；托盘打开/暂停/恢复/退出；设置原子保存、路径/数量限制；新热键先注册成功才替换旧键；暂停ACK与commit gate线性化，旧epoch队列不能事后提交。
- 测试：暂停与commit两种先后顺序；排除变化时在途快照；owner未知；坏settings不覆盖；保存失败不乐观更新；新热键冲突保留旧键。
- 检查：runtime_contract、store/capture/paste相关回归、前端配置状态测试。

### E13 · 重复启动、退出与异常恢复

- 依赖：E12。
- 写入：runtime/lib退出、单实例装配、tests/lifecycle_contract.rs；用户可运行的验收说明。
- 实现：单实例初始化前不写库；重复启动同目录只唤出、不同目录明确拒绝；stopping拒新操作；取消worker并排队落盘；3秒退出未完保持进程报错；重复关闭与部分初始化回收安全。
- 测试：启动各阶段失败、第二实例、pending写入退出、worker受阻退出、DBcommit失败、退出重入、异常终止后仅承诺已提交事务；不杀其他应用或用睡眠猜测关闭成功。
- 检查：`cargo test --manifest-path src-tauri/Cargo.toml --test lifecycle_contract`、相关全套；用户执行A11并记录本应用进程树是否退出。
- W4交回：句柄/线程/进程所有者、失败与回收结果；缺资源回收证据不得进W5正式性能结论。

## 10. W5：T6验收、基线与交付

### E14 · 真实验收与release性能

- 依赖：W4 review通过。
- 写入：tests/search_benchmark.rs、tools/measure-processes.ps1、docs/pinfeather_validation.md；原始数据archive。
- 实现：验收V00–V11与P1–P4，合成固定种子/规模；搜索100k测试使用独立测试库与测试专用容量，不调高正常应用上限。正常UI只测其真实允许数据集，100k的UI路径若没有实际接入则明确未测。
- 检查：验收文档规定命令、release构建、人工流程及原始CSV。所有统计从逐次数据计算，不填估计数。
- 停止：R1/R2/R6阻塞、发现真机错贴/静默丢弃、测量混入别的WebView2或不明PID，回到对应包修复/review，不能靠缩减统计样本掩盖。

### E15 · 最终检查与交回review

- 依赖：E14实际结果已整理，不要求把未覆盖平台假标通过。
- 写入：已存在开发/验证文档的实际命令与结论，首轮清单的证据链接/状态（只在正式task授权后修改既有脏文档，先留精确备份并保留既有内容）。
- 检查：`npm run typecheck`、`npm run test`、`npm run build`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、Rust全部常规测试、显式test-fixtures的worker_protocol、`npm run tauri -- build --no-bundle`、`git diff --check`。单独列出需用户参与的真实结果，不把ignored性能/系统副作用测试算进常规通过数；release构建必须不启用test-fixtures。
- 证据：baseline、dirty文件hash、所有修改路径、实际model/provider标识、每条命令退出码/测试数、功能/构建/真机/性能层次、未测项、偏离方案及需Codex决策项。
- 完成：Codex 按真实 diff 和复跑证据 review，提交首轮完成/未完成报告。无提交/推送/发布；仅用户另行授权后进入这些流程。

## 11. 子agent的实际派工模板

下表保留可选只读分工的上下文要求。当前移交为单 Codex 写者，不自动启用子agent；需要时先有明确的用户或适用指令授权，再确认工具、身份和权限，不能凭 CLI 版本声称支持。

| 子任务 | 给它的上下文 | 要它返回 | 明确禁止 |
| --- | --- | --- | --- |
| API/契约核对 | 当前E任务、完整方案对应章节、指定文件与官方API | 文件/行或官方链接、具体不兼容/遗漏、复现/验证方法 | 不改文件、不运行真实剪贴板/输入、不装依赖、不改方案 |
| 独立review | 用户目标、当前包验收、真实diff/基线、检查命令；不提供主负责人的“已正确”结论 | 按严重度列问题、触发条件、文件/行、需要的证据；没有发现也列未测边界 | 不修复代码、不代主agent改测试、不仅凭报告宣布通过 |

如另获只读分工授权，子agent 不写共享工作树；主负责人完成实现、集成测试和证据核对。不递归派工，不为并行做隐式提交。

## 12. 失败时怎么交回

- 命令/API调用参数、类型错误：在当前任务局部定位并修复，不改变契约；累计两次同类失败后先总结证据再动手，避免整文件重生成。
- 既定设计在真实Windows上不成立：停止相关包，当前 Codex 记录已改文件、最小复现、官方边界与修订建议，标 needs_plan_revision，请用户决定重要产品变化。
- 需要产品范围/权限/系统组件/公开发送决定：停止依赖项，请用户决定；不自动放宽。
- 无法完成实际桌面操作：把具体步骤/预期给用户，标待人工；其他独立只读检查可继续，不能自动切有界面浏览器。
- 报告必须区分失败、未运行、待人工、代码可review、真实通过。未完成包不能靠后续包覆盖掉问题。

## 13. 当前执行证据

- E03a：F2/F3 失败回归及修复完成，本会话自审/复跑见 archive/runs/20261023-e03a-codex-owner-fixes；未启用另一写者或独立模型 review。
- E03b 自动部分：新增[目标/IME只读探针与运行入口](pinfeather_w0_probe_guide.md)，当前 Rust lib 38、worker 37、诊断 CLI 2 项通过，正常 release 构建通过。用户本次授权下已完成工程壳无 UIA/只读 UIA 各 3 次相同单次 WM_CLOSE 对照，均观察到已识别进程树退出。原滞留未复现、原因未知；不能从这 6 次推广为稳定性或人工点击已验收。
- E03b/V01 后续实测：用户完成三目标候选输入并确认，Edge/VS Code/Terminal 合计 529 条稳定、普通权限记录均为 IMM context 不可读、IME unknown。原始数据见 archive/runs/20261023-180555-ime-live。用户已选择独立 W1/W2 先行，W3 保持阻塞，W0 不标通过。管理员/未知权限/关闭目标仍未覆盖。
- E04：`store.rs` 实现 schema v1、同库多载荷事务、capture 幂等、容量与级联删除；`config.rs` 只确定 history 路径，不在工程壳启动时自动开库。文件测试全部使用本次 archive 新建库。覆盖正常重开、提交 WAL 的读取、未知/高版本/模式漂移拒绝和现场字节保留；真实 SQLite 中途/提交失败与三项错误实现反证均已执行。库 42、存储集成 13、worker 37、探针 CLI 2 项通过；证据见 archive/runs/20261023-192601-e04-store-resume。连接线程归属另由编译失败文档测试检查。尚未装配存储线程、监听、列表 IPC，T2 不勾选。
- 用户于 2026-10-23 另行授权 Mimo 并行做 E05 规则/测试矩阵核对；Claude Code 仅提供 Read/Glob/Grep，仓库仍由 Codex 独占写入。新任务及报告见上述 E04 运行目录的 mimo-e05-rules，不恢复历史写者。
- E04 接手收尾：用户改派 Mimo（新交互 CLI，bypass 已获许可）为唯一仓库写者并接手 E04 收尾及剩余实现，Codex 只 review。Mimo 复核 Windows 冻结句柄/只读 WAL 预检（无有效 WAL 才 immutable，有 WAL 走 win32-none 私有索引，已提交 WAL 不被忽略）与全部既有断言，保留通过代码不重写；发现并补齐两处测试缺口：各格式带合法内容的尺寸上界±1（越界 PNG 样例改用合法签名使尺寸上限成为唯一拒绝依据）、capture_id 64 字节与时间戳 JS 安全整数上界。新增 2 项存储集成测试，对 PNG 尺寸上界、capture_id 上界、时间戳上界各做一次错误实现反证均被捕获。复核材料见 archive/runs/20261023-195513-mimo-implementation-transfer/e04-takeover/。
- E04 Codex 复审不通过并定位 F1/P1、F2/P2（源自原交付 store.rs）：缺主库/零字节主库时同名已提交 WAL/journal/shm 会被 SQLite 吞删并新建空库；非空身份未知但 schema 为空的库（如 DROP TABLE 后）被当作新库初始化。Mimo 以真实 rlib 复现（复审探针）后先补失败先行正式回归（12 种侧车现场×全文件集合/字节比较、非空未知空 schema 库现场保持、内存库 Denied 直证），再做最小修复：open 前侧车守卫拒绝非新目标；InitPermit（Memory/FreshFile/Denied）区分初始化资格，写锁下重取新鲜长度再 DDL，不用过期长度覆盖另一创建者。容量/身份契约未改。修复后实跑 lib 43、存储集成 17、worker 37、探针 CLI 2、doc 1 及普通 release 构建全部通过。证据见 archive/runs/20261023-195513-mimo-implementation-transfer/e04-f1-f2-fix/。仍缺 rustfmt/clippy；存储线程/监听/列表 IPC 属 E06，T2 不勾选。
- E04 F1 经复审关闭；F2 遗留 P2（WAL 分支）：未知空 schema 非空库带已提交 WAL 时预检放行 Empty，RW 打开后才在 Denied 拒绝，连接关闭 checkpoint 把 WAL 写进主库并删除（4096+20632 → 8192 现场被改）。Mimo 失败回归先行（写连接打开时复制主+WAL 构造样本，比较全部文件集合/字节）后最小修复：preflight 保留 SchemaState，已有非空主库须只读预检确认 Current 才进 RW，未知/空 schema 目标在任何 RW/create 前拒绝。保留 PNF1 WAL 正例、新建/零字节、F1 全部正反例。实跑 lib 43、存储集成 18、worker 37、探针 CLI 2、doc 1 及普通 release 构建通过。证据见 archive/runs/20261023-195513-mimo-implementation-transfer/e04-f2-wal-fix/。
- rustfmt/clippy 缺组件，未安装，保留检查缺口。复制草稿、现有数据与外部发布边界不变。
- E04 F2 WAL 补修已由当前 Codex 独立复审通过，见 archive/runs/20261023-e04-f2-wal-review。Mimo 的 red-store-contract.txt 实际为筛选1项、0过1败、17 filtered；全量18项通过来自green日志，不能把red描述成17项通过。
- E05 当前由 Codex 实现：只读抓取规则、独立快照协议、同exe产品worker、消息线程及有界通知/快照队列。入口与自动/人工证据边界见方案5.2.2；执行记录在 archive/runs/20261023-e05-codex-owner。存储线程、内部写回登记、commit gate和列表IPC仍属E06，T2不勾选。
- E06：Codex实现内部来源登记与多格式写入所有权、只保留标记/owner的禁止历史响应、独立存储线程、提交gate与历史预留、真实状态/最近列表IPC及React列表。SQL失败保留待提交快照，显式重试后才继续；普通浏览器提示需要桌面宿主。源码、自动检查、自审及错误实现反证见 archive/runs/20261024-e06-codex-owner。原生手动入口使用Cargo定位的产品exe，常规测试仍保持ignored；E06交付时的人工待验已由下述E07补齐。
- E07/T2：用户在本机完成文本/链接、同文外部再次复制、暂停、Edge文本＋HTML、PNG及正常关闭；Codex只读核对真实界面/SQLite并以同目录重启，两轮共5条历史、7份载荷的id/时间/哈希保持一致，PNG解码像素与源样本相同。用户另行手动启动capture_windows，实际1项通过、0 ignored；种子库仍1条，无自录。证据见 archive/runs/20261024-095055-e07-user-acceptance，运行步骤见[指南](pinfeather_development.md)。复用源码/产物校验一致的E06自动与release构建证据，本包仅补文档与人工记录；无实现改动。两次关闭出现Chrome_WidgetWin_0注销错误1412，未观察到数据损坏或自有进程残留，原因未知，留作E13复查。快速复制、占用、隐私标记等扩展真机矩阵未覆盖，R2/R3与首轮仍不整体勾选。
