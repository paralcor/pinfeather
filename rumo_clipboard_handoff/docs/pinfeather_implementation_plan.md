# 片羽首轮完整实施方案

版本：v1，更新于 2026-10-24。状态：E00–E06已有工程与相应自动证据，E07/T2基础链路已通过本机人工验收；下一包为E08搜索查询。E03b的IME子关卡仍阻塞W3，扩展真机矩阵及性能验收待办。

2026-10-23 用户更新职责：新 Codex 会话全程负责设计、实现、修复、验证与 review，是工作区唯一写者。Mimo 已停止执行，原 Codex 会话在确认交接后也停止写入。此前“Codex 不写代码、Mimo 实现”的限制被本次授权替代；历史报告仍保留原执行身份。

## 1. 阅读入口与方案边界

阅读顺序：本文 → [逐步任务](pinfeather_execution_tasks.md) → [验证与验收](pinfeather_acceptance_plan.md)。执行单个任务时再定位 [MVP 规则](rumo_clipboard_mvp_specification.md)、[原型约定](../../design/prototype/README.md)与[首轮清单](pinfeather_first_iteration_tasks.md)，不一次拼接所有候选 A–G。

权威分工：MVP 拥有产品行为与容量基线；本文拥有首轮模块、接口与实现安排；逐步任务拥有写入范围、依赖和命令；验收文档拥有用例及证据要求。同一规则不在多个源文件另设默认值。负责人先核对并解决工程细节冲突；涉及重要产品变化时停止依赖项交用户决定。

用户已经确认：

- Windows 11 25H2/26H2 Home/Pro x64；先验本机 25H2 家庭中文版 26200.9711，其他组合未测。
- 普通权限运行；管理员或权限不明目标只手动粘贴；不默认提权、修改前台锁或清除 IME 文字。
- Tauri + Rust + React/TypeScript；本地单 SQLite 文件分表；一个可终止的独立抓取进程。
- 文本/HTML 每格式 2 MiB，PNG 样本 8 MiB，单条原始载荷 12 MiB；待存队列 64 条且 32 MiB；历史 10,000 条且载荷 256 MiB；达到任一限制暂停并提示，不自动删历史。
- 读取最多 5 次、总截止 750 ms；唤出键首轮可改的 Ctrl+Alt+V；先键盘选择/Enter/Esc；单击回贴、悬停选中和快选后置。
- 集合、模板、CLI/MCP、同步、完整图片体验、OCR、加密存储、自动清理、开机启动均不在本轮。
- 贡献指南的“大改先 issue”指现有设计方案需要重大调整；按已确认设计建立工程不要求先发 issue。已有两份 issue 草稿保持不发布。

按依赖逐包执行，每包保留实现后 review 和重新运行检查的关卡；用户已授权新 Codex 持续推进首轮，不再等待外部 Mimo 交回。E03a 的 F2/F3 已修复并经本会话复核；E03b 三目标真实 IME 观测暴露当前外部 IMM 方法不满足预检。用户于 2026-10-23 批准独立 W1/W2 先行，W0 的 IME 子关卡保持 needs_plan_revision，W3 自动回贴继续阻塞，首轮不能标完成。除此明确例外，仍按原依赖推进；人工验收与重要决定不以自动结果冒充通过。

## 2. 本轮做成什么

最终流程：用户在真实应用复制 → Rust 监听变更 → 工作进程取得一致快照 → 单库事务保存 → React 列表/正文检索 → 热键打开快捷面板 → 选中纯文本 → 原目标校验 → 复制并尝试发送粘贴指令，失败或状态未知时清楚降级。

交付包括：能复现构建/启动的源码、正式测试和合成夹具、真实运行说明、R1/R2/R6 证据、启动与完整进程树内存基线。交付不包括安装包发布、签名证书、系统运行时安装、提交/推送/PR 或公开消息。

“代码可检查”“本机可启动”“真实应用验收通过”分别记录。人工验收等待时，允许完善独立的自动测试；不把等待写成通过，也不让已失败的接口检查进入下一工作包。

## 3. 总体结构与文件职责

保持一个产品工程，一个 `src-tauri` crate，同一个可执行文件含宿主模式及内部抓取模式；不引入微服务、通用插件系统或多套存储访问层。

```text
pinfeather/
  package.json / package-lock.json / components.json
  index.html / vite.config.ts / tsconfig*.json
  src/
    main.tsx / App.tsx / styles/globals.css
    lib/ipc.ts                   宿主调用唯一入口与 DTO
    stores/app-store.ts          Zustand 摘要、查询代次、选中与状态
    components/ui/               shadcn Nova 按需源码
    features/history/            最小列表、分片纯文本预览
    features/quick-panel/        搜索与键盘选择
    features/capture/            暂停、最小应用排除、错误状态
  src-tauri/
    Cargo.toml / Cargo.lock / build.rs / tauri.conf.json
    capabilities/main.json / capabilities/quick-panel.json
    src/main.rs                  先分派内部 worker，再启动 Tauri
    src/lib.rs                   Tauri 装配，注册命令/窗口/托盘
    src/contracts.rs             Rust DTO、稳定错误与大小常量
    src/config.rs                显式数据目录、设置读写及校验
    src/store.rs                 schema v1、唯一连接所属线程、事务与查询
    src/runtime.rs               服务生命周期、策略 gate、队列/调度
    src/commands.rs              Tauri 入参校验与异步返回
    src/clipboard/mod.rs         捕获状态、来源登记、快照验证
    src/clipboard/worker.rs      工作进程所有权、协议、截止与回收
    src/clipboard/windows.rs     剪贴板 Win32 读取/写入及资源 RAII
    src/platform/windows.rs      消息窗、热键、前台/权限/IME 只读检查
    src/paste.rs                 回贴状态机，不直接处理前端展示
    tests/                       正式集成、协议与性能测试
  tests/frontend/                React 行为测试
  tests/fixtures/                合成文本、格式、故障与预期结果
  tools/dev.ps1                  进程级 PATH、dev/release 启动
  tools/measure-processes.ps1     本应用进程树只读采样
  rumo_clipboard_handoff/docs/   方案及可公开验收说明
```

先建当前任务需要的文件，禁止一次生成上述全部空模块。小文件可以保留在职责明确的模块内，不为凑目录再加转发层。Rust `unsafe` 限于 Windows 适配模块，调用约束与句柄所有权就地说明；纯规则和状态机可用普通 Rust 测试。

### 3.1 进程与线程所有者

| 单元 | 创建与释放者 | 做什么 | 不做什么 |
| --- | --- | --- | --- |
| Tauri 主线程 | 应用宿主 | 窗口、WebView2、托盘、UI 线程调度 | 不持剪贴板锁，不跑 SQL 或阻塞等待 |
| 原生消息线程 | runtime | 隐藏消息窗、剪贴板通知、RegisterHotKey、前台 WinEvent 回调 | 不读延迟渲染载荷、不执行回贴长等待 |
| 抓取 supervisor 线程 | runtime | 串行派发快照请求、监控截止、回收工作进程 | 不写数据库，不当作已落盘证据 |
| 内部抓取进程 | supervisor | 同一次持锁读取标记、格式及载荷，输出快照 | 不启动 Tauri/WebView2、不写库/文件、不写系统剪贴板 |
| 存储线程 | runtime | 独占 SQLite 连接，提交、查询与删除隔离样本 | 不操作系统焦点或输入 |
| 回贴任务 | runtime，每次最多一个 | 用会话/操作 ID 运行一次状态机，取消后拒绝迟到结果 | 不自动重发 Ctrl+V，不抢用户新窗口 |
| React | Tauri 窗口 | 摘要、用户意图、可见状态；通过 ipc.ts 调用 | 不掌握 HWND、系统路径任意读写或原始 SQL |

消息线程发布注册成功后，运行态才可显示“正在收录”。worker、数据库、热键各有独立就绪/失败项，部分成功不合成全部就绪。

## 4. 工程、依赖与启动

### 4.1 依赖选择

下列是 2026-10-22 查询的候选精确版本。已落地的最小依赖以 package/Cargo 清单和锁文件为准，已有编译证据；尚未引入的候选不视为兼容性已验证。有兼容问题先核对并记录，禁止自动追逐 latest 或换库。

| 能力 | 版本与约定 |
| --- | --- |
| 桌面 | Rust tauri 2.12.5、tauri-build 2.7.4；npm @tauri-apps/api 与 cli 均 2.12.4；Rust 1.98.0、x86_64-pc-windows-msvc |
| UI 工程 | React/react-dom 19.3.1、Vite 8.3.5、@vitejs/plugin-react 6.1.2、TypeScript 7.0.2 |
| UI 基础 | shadcn 4.22.1，components.json 使用 base-nova、neutral、Lucide、rsc=false；Tailwind CSS 与 @tailwindcss/vite 均 4.3.3 |
| 图标与状态 | lucide-react 1.61.0；zustand 5.0.16 |
| 原生平台 | windows 0.62.2；只启用实际调用的 Win32 features |
| 存储 | rusqlite 0.40.2，bundled SQLite；不依赖系统 sqlite DLL |
| 序列化 | serde 1.0.229、serde_json 1.0.154、base64 0.23.1；只有 worker 载荷需要 base64 |
| 操作身份 | uuid 1.28.0（v4），宿主实例 UUID + 本地递增操作计数 |
| 单实例 | tauri-plugin-single-instance 2.5.4，仅宿主模式装配 |
| 类型/测试 | @types/react 与 react-dom 19.3.0，@types/node 26.6.5；Vitest 5.0.5、jsdom 30.1.3；@testing-library/react 16.3.3、@testing-library/user-event 14.6.8 |

Motion、react-resizable-panels、dnd-kit 继续作为约定库，首轮无相应功能就不装。shadcn 官方组件生成涉及的 Base UI/cmdk/样式工具是该 UI 组件依赖；只装实际生成组件需要的集合，记录来源并把直接依赖改成精确版本，不自行混入另一套 UI 框架。包的 engine/peerDependencies 由 W0 核查；@types/node 必须与 Node 24 实际 API 范围兼容，不因为类型包版本更高就使用 Node 26 专属 API。

先 button、input、状态提示与原生可访问列表；T3/T4 引入 Command 时关闭其前端过滤（`shouldFilter=false`），Rust 查询结果是权威。不要让组件内置过滤使“正文命中但摘要不含词”的条目消失。

npm lock 和 Cargo.lock 仅由包工具产生。Rust 直接依赖也固定初始版本；Tauri Rust 与构建工具版本号不要求互相相等，npm 的 api/cli 保持同组版本。后续升级按贡献指南处理，不手改 lock，不加 overrides。

### 4.2 数据目录与运行入口

宿主启动必须有 `--data-dir <绝对目录>`，缺少或无效即报错退出；没有生产目录回落。首轮启动脚本固定把新库放在任务 archive 的 `runtime-data/<run-id>`。首次创建及自动化启动传 `--start-paused`，用户在 UI 中主动开始收录；这是隔离原型的启动安排，长期产品默认值另定。

设置保存在该目录的 `settings.json`：`schema_version=1`、capture 开关、热键、排除的规范化完整 exe 路径。缺省热键 Ctrl+Alt+V，排除列表为空；手工更改损坏时停止收录并报告，不静默覆盖。写设置采用同目录临时文件及原子替换；失败不报保存成功。单次 `--start-paused` 覆盖记忆的开启状态，但不改写用户已存设置。

本轮一个 Windows 用户最多运行一个宿主，避免同时测试两个数据目录争用全局剪贴板。相同目录二次启动只唤出已有列表；不同目录二次启动明确拒绝切换或打开另一写者。先初始化单实例插件，再打开 SQLite/注册监听。内部 worker 在解析模式后立即走独立入口，不能被单实例插件错误拦截。

`tools/dev.ps1` 只为其子进程补 `.cargo/bin` PATH；不 `setx`、不改系统环境、不安装组件。Vite 固定 127.0.0.1:1420、strictPort；冲突时报告进程信息，不杀用户进程。release 仅构建 exe（不制作/安装 MSI），资源沿用现有应用图标。

两个 WebView 窗口均由宿主在参数校验后创建，显式使用当前 data-dir 的 webview 子目录，不能在配置默认自动建窗时提前落入其他用户数据目录。两个窗口共享该隔离 WebView2 profile；profile 写入属于运行缓存，不是第二份历史数据库。

## 5. R2：监听、快照和内部写入

### 5.1 不丢掉实现已取得的外部快照

消息窗收到 WM_CLIPBOARDUPDATE，分配 observed ID，记录非零序列值和单调时钟；通知不等于载荷。supervisor 正在抓取时仍接纳变更通知。通知元数据队列上限 256：满时记可见 overrun 并暂停，不静默把中间通知压成一次成功事件。

每个任务读取当前剪贴板，抓取前已覆盖的 observed 记 superseded；同一序列的重复通知关联同一 capture ID。通知队列存元数据，已抓取快照队列另受 64 条/32 MiB 限制；一份最多 12 MiB 的在途快照不计作已排队，但其内存仍计入性能。发布后不复制 Vec/BLOB 到多个监听者。

抓取前/锁内/读取后核对序列一致，允许格式全部从同次锁取得。零序列、锁不可得、版本变化分别处理；序列号只作相等比较，不能用 `>` 假定不回绕。重试不增加独立历史事件；成功快照的 capture ID 不变。

### 5.2 worker 协议与截止

同一个 exe 的内部模式为 `--capture-worker-v1`，不接受任意命令、文件路径或 shell。匿名 stdin/stdout 管道只继承必要端点，CREATE_NO_WINDOW；stderr 只放安全诊断，stdout 只放协议。宿主持有进程/管道/读取线程句柄；使用仅覆盖该 worker 的 Job Object，KILL_ON_JOB_CLOSE，绝不把整个用户进程树放入该 Job。

协议：4 字节 little-endian 无符号长度 + UTF-8 JSON。请求上限 4 KiB，响应上限 18 MiB，读长度先检查再分配；原始 payload 解码后仍检查 12 MiB 合计。每次只有一个请求在途；响应包含 version=1、capture ID、policy epoch、序列前后值、owner HWND/PID、格式及 base64 字节、结果类别。正文/摘要由宿主从原始快照派生，不让 worker 再传一份冗余全文。

启动时 hello/version 协商限 2 s，未就绪不读剪贴板。每次抓取从父进程发送请求前计时，总 750 ms；worker 的 5 次尝试及 20/40/80/160 ms 间隔服从同一截止。延迟渲染卡住时，由父进程终止自己的 worker、wait/reap 并关闭管道；不能仅把读取线程标为超时继续留在后台。最近一轮可自动重建一次，连续再次超时暂停；一次正常快照后重置连续故障计数。

worker stdin EOF/父端消失即退出；父端 Job 关闭防止遗留进程。回收还要等待管道读取线程确认结束，限 2 s；超限记 cleanup_failed，保持宿主错误/暂停状态，不继续产生新 worker、不做无界 join。协议超长、非法 JSON/base64、错 ID/epoch、迟到响应、部分帧和 worker 崩溃均拒绝且可诊断。

W0 必须用不访问剪贴板的受阻测试子进程证明“超时→终止→句柄/线程回收”。这不能替代真实延迟渲染验收，但能先排除 supervisor 自身泄漏。

### 5.2.1 E03a 的交付边界

本步骤只证明传输和子进程资源所有权，不实现抓取、存储或界面成功状态。生产 supervisor 与测试使用同一有界流读取、截止和清理路径；测试替换子进程行为，不替换 supervisor。

- 握手帧正文固定为 `{"kind":"hello","version":1}`；字段必填，拒绝额外字段、错类型及版本。2 s 截止包含创建、Job 绑定和握手。
- 在途传输探针请求和 fixture ACK 复用现有 `WorkerMessage` 三字段 JSON（version/capture_id/policy_epoch）；ACK 只说明协议往返，不是抓取结果。将来 E05 接入原始快照时须完善响应契约，不得把 ACK 当作历史条目或成功快照。
- E03a 的 hello/ACK 均为控制帧，正文上限 4 KiB，先检查长度再分配/解析；控制帧不需要大块空白。第 5.2 节的 18 MiB 是后续快照响应的总上限，现阶段不能用它允许三字段 ACK 占用 18 MiB。E05 接入载荷时再定义响应类别和各类边界。
- reader 最多缓存 2 个控制帧，即正文合计不超过 8 KiB，另有至多一个 4 KiB 在途读取缓冲。溢出明确失败并回收，不无限缓存或无限排空；停止/失败信号独立于满帧队列，必须可达。有限的已结束 ID 帧可按既有规则丢弃，但数量和处理时间也有界。
- release supervisor 的产品路径固定为当前产品 exe 与 `--capture-worker-v1`。仅 `test-fixtures` 下的测试构造入口可选择 Cargo 定位的 worker_fixture exe，不接受用户命令、shell 或任意产品路径参数。产品 worker 入口在 E05 前继续明确拒绝未实现模式，不伪造 hello/快照。
- supervisor 唯一拥有 Job、子进程、管道、读取线程和截止状态。创建过程限定继承必要管道端点；在开始处理前完成 Job 归属，失败立即清理。Job 仅覆盖自身 worker，不能包含宿主或其他用户进程。
- 状态为 Starting → Ready → InFlight → Ready；失败进入 Reaping，确认进程和读取线程退出后才允许重新创建。一次捕获超时可重建一次，连续第二次超时暂停；正常 ACK 清除连续失败数。清理失败进入 FailedPaused，不再重建；重复关闭幂等。
- reader 的 fatal 状态独立保存，在接纳 hello/ACK 前优先检查；正常 EOF 不覆盖已收到的完整帧。进程、reader、writer 必须在同一 2 s 清理预算内确认退出后才释放管道并允许重建。未确认时保留完整资源束并暂停，close 可重试；最终 Drop 仍失败则保留资源至宿主进程退出并禁止该进程再创建 worker，不能静默 detach。
- 响应截止为发送前至完整读取和关联校验结束的总 750 ms；分段到达不能延长截止。所有失败均回收自有 worker；清理最多 2 s，不能超时后留下无界 join/读取线程。
- fixture 覆盖正常、无 hello、hello 错误、无响应、半长度头、半正文、超长声明、坏 JSON、错 ID/epoch、崩溃及迟到响应。证据包含本轮精确 PID/创建时间、退出完成、读取线程确认和重复关闭结果，不能只凭函数返回 timeout。
- fixture 为 feature-gated 独立 bin；普通 release 不包含它。自动测试不启动 WebView，不访问系统剪贴板、用户库、桌面焦点或输入法。

### 5.2.2 E05 快照协议与监听入口

`CapturePipeline::start_paused` 创建独立消息线程，注册完成后发布监听就绪；默认暂停，不创建抓取请求或读取载荷。后台调用方通过 `poll_capture` 串行抓取；抓取期间释放状态锁，消息线程继续接纳元数据。宿主 UI、存储线程和 IPC 在 E06/E07 装配，不用库模块存在推断 T2 已通过。

请求沿用 version/capture_id/policy_epoch。产品响应严格为 `kind="snapshot"`，包含 result（captured/ignored/failed）、安全 reason 类别、sequence_before/locked/after、owner（hwnd十进制字符串/pid/process可空）、formats（name/data标准base64）、omitted_formats、internal_origin（可空base64）、cloud_marker_anomaly 和 attempts（1–5）。父端拒绝额外字段、错误类型/身份/关联、坏base64及解码后越界；captured/ignored 只接纳同次锁内非零且一致的序列。正文只由原始载荷派生，不传冗余全文。协议仅供同一产品 exe 内的两个进程，原子升级，无已部署旧协议兼容分支。

hello 仍先以4KiB上限读取；此后的产品快照正文上限18MiB，解码后允许载荷合计12MiB。ACK 往返入口只编译于 test-fixtures，继续使用4KiB控制帧上限，产品解析器拒绝ACK。reader最多缓存2帧，另有一份在读帧；产品编码帧缓冲相应上界为2×18MiB加18MiB，不是E03a控制测试的8KiB。解码后的大块wire字符串立即释放，不留第二份全文或向多个监听者复制BLOB。

父端发送前750ms总截止包含读取、解码和关联校验，worker本地5次/20–40–80–160ms仅是从属预算；重建不重放超时的原请求，下一独立通知才有新预算。只有正常captured响应重置连续超时，ignored/failed不重置。CloseClipboard失败丢弃快照并终止、回收自己的worker，保留cleanup_failed禁重建规则。

消息线程仅记录通知元数据（上限256）。抓取前按当前序列相等性淘汰已覆盖观察并计superseded，不用数值大小排序；相邻同序列通知关联同一capture，观察过不同序列后再出现相同数值属于新事件，不把序列当永久ID。通知overrun与64条/32MiB快照队列满均关闭admission并记录原因，不递增epoch、不覆盖已接纳快照；用户显式恢复且快照队列有余量才重开。用户暂停/排除变化递增epoch、取消旧未提交快照。E06提交gate仍必须重核策略与真实历史容量。

零序列不确认内容，并解除先前观察的身份关联；恢复非零后重新建立capture。owner路径先按既有Windows路径契约规范化，再用Windows ordinal Unicode不区分大小写比较排除项；路径无法确认且存在排除规则时禁收，并保留来源未知提示类别。

注册格式先通过 RegisterClipboardFormatW 取得运行期ID，再按ID识别HTML、PNG与隐私标记；名称比较不区分大小写，不能依赖生产者返回的拼写，见[微软注册格式说明](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerclipboardformatw)。未知格式只枚举名称，最多256项、每名256个Unicode字符，不反序列化其数据。未知/损坏内部标记继续按外部候选处理，E06登记表尚未装配，不能据此抑制自录。

监听器关闭通过自身原生线程句柄关联的消息线程退出，2s内确认后才join/释放；失败保留线程并阻止再创建，避免向已回收的HWND发送关闭。普通测试的消息泵检查不注册真实剪贴板监听；产品worker的hello/EOF检查不发送抓取请求。真实通知、延迟渲染和窗口关闭验收另行由用户操作。

### 5.2.3 E06 宿主装配与自动检查边界

`HostRuntime`在开库和注册监听前持有本次Windows会话的命名互斥对象，第二宿主明确拒绝启动；E13的同目录唤起与完整恢复流程尚未实现。抓取线程唯一持有listener/supervisor；存储线程内创建并使用SQLite连接，读请求队列限16，每批最多4个读请求后检查FIFO写队列。窗口调用的状态、列表和暂停等待通过Tauri后台任务执行，不在GUI消息线程等待提交gate。

历史禁止的快照继续不读取文本载荷，但允许返回有界内部标记/owner/sequence元数据，父端用28字节标记和128项操作登记确认内部来源；不存在登记、实例不符、owner不符、未确认sequence均不能按内部成功抑制。写回标记与两项隐私DWORD先于文本，SetClipboardData成功才移交HGLOBAL；失败只释放尚未移交的内存，部分写入不报成功、不恢复旧剪贴板。写回仅有Rust的已存条目入口，不注册任意文本IPC；排队超时的写入在副作用前拒绝，已开始但结果未知时不得自动重试。

队列预留包含正在SQLite事务中的一份快照，接纳时同时核查历史条数/字节与队列额度；事务内Store仍核查真实容量。最终策略复核、事务提交与暂停/排除确认共用gate，SQL期间释放普通状态锁，监听元数据仍可入队。失败保留原FIFO载荷并关闭admission，只有显式重试才重新提交；history_changed只在成功事务ACK后发出。

最近列表只读摘要元数据与格式名，不向WebView传BLOB；当前仅空查询，upper_id keyset分页及1MiB输出预算照既有契约检查。原get_shell_status已替换为get_status/list_clips/set_capture_enabled，校验注入的真实窗口及本地origin。React先订阅再读取，不乐观确认暂停，用revision防旧状态覆盖，合并事件保证单个列表请求在途；状态中的历史计量变化也触发列表刷新，补偿遗漏的history事件。普通浏览器无宿主时不调用IPC或显示假记录。

关闭请求先停止观察，已接纳且策略仍有效的队列继续提交；3s内未确认全部线程回收则保留窗口和未保存内容。SQLite失败时仍可显式重试保存后再次关闭。本包提供必要的有界退出保护，不替代E13的完整生命周期/异常恢复验收。

普通自动检查仅用合成Win32接口、真实受控worker、SQLite内存或本次新建隔离库，以及不注册剪贴板的消息泵；宿主UI、生产监听、真实owner/内存句柄和内部写回仍待E07人工。`capture_windows`只在test-fixtures编译，始终ignored，并使用Cargo的产品exe路径而非测试harness；未运行不能计入通过数。执行证据见 archive/runs/20261024-e06-codex-owner。

### 5.3 Win32 内存与格式

`GetClipboardData` 的句柄属于系统，GlobalLock 后复制到自有字节，GlobalUnlock 并 CloseClipboard；不跨锁持有指针、不 GlobalFree 系统给的句柄。GlobalSize 和加法都检查溢出/上限，不先按不可信尺寸分配。

格式规则和隐私标记按 MVP 3.4：整体排除存在即禁收，历史标记读取 DWORD 值，云禁止不当本地禁收。未知格式只存名称用于解释，不尝试反序列化。

文本：UTF-16LE 至首个 NUL 严格解码；后续分配填充不视为正文；无终止符、奇数字节或坏代理对拒绝。HTML 原始字节只存储/转义展示，不解析执行。PNG 只检查固定签名和尺寸上限，不解码；检查头不等于完整图像有效性。没有 Unicode 文本而只有 PNG 时允许存入不可文本检索的样本条目。

### 5.4 写回与抑制自录

内部写入先登记 operation ID，再使用宿主隐藏窗 HWND 调用 OpenClipboard/EmptyClipboard，写 `Pinfeather.InternalOrigin.v1`、两种禁止历史/云上传的 DWORD 0 标记及 CF_UNICODETEXT。必须使用真实 owner HWND，不能 OpenClipboard(NULL) 后 EmptyClipboard 导致 owner 为空。为每个 HGLOBAL 使用 GMEM_MOVEABLE；成功 SetClipboardData 的对象转交系统，失败未转交的对象由自身释放。CloseClipboard 和失败路径均有资源测试。

标记使用固定28字节布局：u32 little-endian版本1 + UUID原始16字节 + u64 little-endian操作计数。分配用GMEM_MOVEABLE与GMEM_ZEROINIT；解析允许GlobalSize中的分配填充，但先要求至少28且不超过256字节，只解析前28字节。只有匹配登记、owner 属于宿主消息窗且序列与完成写入相符时识别内部来源。登记保留最近 128 个操作；当前剪贴板操作和未完成关联的条目不提前淘汰。超出固定资源边界则拒绝新写回并报告，不扩大无界表。

先写内部标记/隐私格式，再写文本；任一步失败即停止后续自动回贴，不把部分写入描述为全部成功。Win32 写入不能回滚，禁止自动恢复旧内容。通知分类同时覆盖写入中/完成后/延迟通知；没有全局 busy 或固定毫秒屏蔽窗。

### 5.5 策略变更与入队

runtime 的 policy epoch 在暂停、排除变更时递增。快照携带取得时的 epoch。入队前及 SQLite commit gate 内复核当前策略，旧 epoch 的待提交快照被取消并计数；不能在 UI 已确认暂停后继续把旧队列写入。

commit gate 是短的 Mutex 保护区：只覆盖最终策略检查与事务提交，设置变更也经同一 gate 线性化。若提交已先取得 gate，则暂停确认等待该提交结束；暂停操作返回后，不再有旧策略提交。等待发生在后台任务，不能阻塞 Tauri/Win32 消息线程。排除 owner 无法确认时按 MVP 已确认策略处理，不拿前台进程冒充 owner。

用户主动暂停/改排除规则与系统过载必须分开：前者更新 policy epoch 并按已确认策略取消旧快照；队列满只关闭 admission（接纳新快照），不递增 epoch，保证此前已接纳且策略仍有效的快照继续落盘。UI分别显示“用户暂停”“队列过载”“存储失败”，不能只用一个paused布尔值控制全部路径。

runtime维护已提交条数/载荷字节和待存预留量，首次从store读取。在快照接纳时同时预留历史条数/字节和队列额度；达到容量则拒绝当前快照、停止新增、显示原因，不能先接纳一批已知无法存入的记录。提交ACK把预留转成已提交，取消/失败释放对应预留，重复capture不重复预留；SQLite事务内仍复核真实容量。统计不一致就暂停并从store重查，不能继续猜计数。

存储队列满：保留已接纳快照，当前快照记 overloaded，停止新增并通知。存储错误时在原有有界队列中保留未提交快照，不报已保存；可重试/退出失败由用户决定。容量满不触发自动清理。只允许用户显式恢复，不用自动反复重试造成重复条目。

## 6. R3/R6：数据库、查询与预览

### 6.1 schema v1

一个 `history.sqlite3`，单连接归存储线程，WAL + foreign_keys=ON + synchronous=FULL；busy_timeout 250 ms。仅全新空库初始化，schema 和 user_version 在同一事务设置。建议 application_id 使用固定十六进制 `0x504E4631`（PNF1），识别无关数据库；不符合、模式更高或非空未知库拒绝写入，不重建。启动只读 quick_check 失败即停止收录。

E04 的文件预检先用 Windows 只读文件句柄阻止主库及已有 WAL 被写入或替换。无有效 WAL 时在冻结文件上只读检查；存在 WAL 时使用 SQLite 内置 `win32-none` 与私有 WAL 索引读取，避免普通只读连接创建共享内存文件。该 VFS 仅用于预检，必须持有冻结句柄，绝不用于写连接。存在非空 rollback journal 时保留现场并拒绝打开。预检后普通 SQLite 写事务仍重新校验身份、版本和完整模式；DDL、application_id、user_version 一起提交。机制依据见 [SQLite VFS](https://www.sqlite.org/vfs.html)、[只读 WAL](https://www.sqlite.org/wal.html#read_only_databases)和 [Windows 文件共享模式](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html#tymethod.share_mode)。

| 表 | 字段与约束 |
| --- | --- |
| clips | id INTEGER PRIMARY KEY AUTOINCREMENT；capture_id TEXT NOT NULL UNIQUE；captured_at_ms INTEGER NOT NULL；text_full TEXT NULL；preview TEXT NOT NULL；source_process TEXT NULL；source_confidence 枚举 owner/unknown；omitted_formats_json TEXT NOT NULL；payload_bytes INTEGER NOT NULL CHECK>=0 |
| payloads | clip_id INTEGER NOT NULL REFERENCES clips(id) ON DELETE CASCADE；format_name TEXT NOT NULL；encoding_version INTEGER NOT NULL（首轮1）；byte_size INTEGER NOT NULL CHECK>=0且等于length(data)；data BLOB NOT NULL；PRIMARY KEY(clip_id,format_name) |
| 索引 | clips(captured_at_ms DESC,id DESC)；capture_id 唯一索引；不建 FTS |

一条快照的元数据、全部允许载荷、完整正文一次事务提交。容量计数由该连接在事务内读取已存总条数/字节并加本次增量判断；重复 capture_id 是同事件幂等，不再次计量或写半条。不以正文或 CRC 判重。故障注入在“元数据已写、第二个载荷前、提交前”逐点验证整体回滚。

正文不裁到摘要。UTF-16 原始大小与 UTF-8 正文大小分别算，不假定转换后等长；原始 2 MiB 的合法文本转换后可能超过 2 MiB，正文仍完整存储并受已定义原始载荷上限约束。预览最多前 200 个 Unicode scalar values，不能按任意 byte slice 截断。

删除只用于隔离条目验收：在本地只读查询出准确 clip_id 后，由 Rust 测试直接调用存储删除路径，验证级联与查询一致；不为本轮额外做完整历史批量删除 UI。数据库打开/提交失败绝不 DROP 或清空补救。已有个人数据库不纳入此执行包。

### 6.2 检索与分页

Rust 唯一负责 trim Unicode 空白、256 字符限制和 limit 1–100（缺省50）；中间空白不拆词。使用绑定参数 `instr(lower(text_full),lower(?))>0`；空查询走最近列表。SQLite lower 仅承诺 ASCII 大小写，中文/其他 Unicode 不做额外归一化。百分号、下划线、引号、反斜杠均不参与 SQL/FTS 语法。

Keyset 分页：第1页固定查询开始时的最大 id 作为 upper_id，后页只取 id<=upper_id，按 captured_at_ms DESC,id DESC 及最后排序键续取。cursor 是结构化对象，携带 normalized_query、upper_id、last_captured_at_ms、last_id；逐字段验证，cursor 与当前查询不一致报 invalid_input。cursor 不写日志。id 用十进制字符串过 IPC，时间戳限制在 JS 安全整数范围。

存储线程读/写队列均有界：最多 16 个待处理查询；前端只有一个当前搜索和一个当前预览。查询不能饿死已接纳写入；采用每批最多4个读请求后检查写队列的调度规则。UI 50 ms 防抖，输入变化立即递增代次并使旧 cursor/预览失效；返回 request_id 和 normalized_query，只有当前代次可更新列表。

预览按 id 和 offset 取纯文本，offset明确为UTF-8字节偏移，必须处于字符边界且不超过正文长度；每帧 UTF-8 不超过 64 KiB，结束也落在完整字符，返回 next_offset或null。按需“继续加载”，不在 React 首次列表加载所有正文。每次 IPC 仍低于 MVP 的 2 MiB 文本上限。HTML 不用 dangerouslySetInnerHTML；PNG 本轮只显示“已保存 PNG 样本/字节数”，不把其伪装成完整图片体验。

## 7. IPC 契约与安全边界

以下是字段设计，不是已存在的 API。Rust contracts.rs 为宿主权威，TS lib/ipc.ts 逐项镜像；增加/改字段必须同时改契约测试。wire字段统一snake_case；有入参的命令仅接收一个名为request的DTO，前端invoke传{request:...}，避免Tauri顶层参数命名映射各写一套。DTO拒绝未知字段。request_id是前端生成的非空ASCII标识，最长64字节；id均为规范正整数十进制字符串，session/operation身份由宿主生成且前端不可伪造目标句柄。所有数值范围、枚举、路径和 id 在 Rust 边界重校验；命令只允许本地 main/quick-panel 窗口，不开放给远程 origin。

| 命令 | 入参 | 返回与限制 |
| --- | --- | --- |
| get_status | 无 | startup/listening/paused/overloaded/failed/stopping；各服务 readiness、计数、队列条数/字节、当前数据目录与版本、最近paste结果或null；无正文 |
| list_clips | request_id、query、limit、cursor 可空 | request_id、normalized_query、summaries、next_cursor；摘要含 id、时间、类型/格式名、preview、source 可信度、字节数 |
| get_clip_text | request_id、clip_id、offset | request_id、clip_id、text、next_offset；64 KiB，找不到/已删除报 not_found |
| set_capture_enabled | enabled | 更新完成后的 status；暂停确认服从 commit gate，不先发乐观成功 |
| set_excluded_apps | 规范化 exe 完整路径数组，最多32个 | 更新后 status；每路径最多1024字符，不允许目录/通配符；来源未知规则不变 |
| set_hotkey | ctrl/alt/shift/win 布尔及单个受支持键 | 注册新键成功后才保存并释放旧键；冲突保留旧键，返回 shortcut_unavailable；拒绝 Win+V |
| panel_ready | panel_session_id | 校验当前会话；标记渲染内容可交互，只作测量及 UI 就绪 |
| paste_clip | panel_session_id、clip_id | operation_id、clipboard_state、input_state、reason；不接受前端提供的 HWND、任意文本或注入数组 |
| dismiss_panel | panel_session_id | 仅当前会话生效，取消回贴任务；需要时安全恢复原焦点 |
| request_exit | 无 | 开始有界退出；若失败保持进程并给出待保存数量 |

summary精确字段：id、captured_at_ms、kind(text/link/png)、preview、format_names（允许格式最多3个）、omitted_format_count、payload_bytes、source_process（可空）、source_confidence(owner/unknown)。链接只在整段合法http/https URL时标link，不引入代码/日志自动识别。完整遗漏格式名仅在数据库元数据中保留，不把全部未知格式名称塞进每页摘要。单页JSON最多1MiB，达到预算可少于limit，cursor指向最后实际返回条目。

paste返回：clipboard_state为not_written/written/failed/replaced，input_state为not_sent/sent/cancelled/failed，reason为稳定错误code或null。sent只指4事件已插入输入流，绝不表明目标消费。状态/结果字段必须在Rust与前端测试里一一对齐。

调用者窗口来自Tauri注入的WebviewWindow对象，不能信任前端自报label。两个窗口都可status/list/preview和暂停恢复；settings热键/排除仅main；panel_ready/paste/dismiss仅quick-panel；request_exit仅main，原生托盘直接调用宿主服务。未知窗口或非本地origin拒绝。权限与capabilities共同约束，不能只靠按钮隐藏。

统一错误：`code`（invalid_input/not_found/busy/capacity_exceeded/storage_unavailable/shortcut_unavailable/target_changed/permission_unknown/unsupported_input_state/clipboard_changed/injection_failed/timeout/protocol_error/cleanup_failed）、安全 `message`、`retryable`。不要把文件原文、SQL 或外部复制内容拼入错误。前端不得对 paste 自动重试；只有明确可重试的只读查询可由用户重试。

事件只发 `history_changed{revision}`、`status_changed{revision}`、`panel_opened{session_id}`、`paste_finished{operation_id,result}`。revision/ID 用字符串避免精度问题；先订阅再取初始状态，事件丢失/重连后重新查询状态，不能只依赖增量事件维持永久真相。隐藏窗口仍可保存最近结果，用户再打开时能看见失败原因。

Tauri CSP 生产仅本地打包资源和所需 IPC，禁止远程导航/弹窗；不启用 shell/fs/http/opener 插件。capabilities 仅给 main/quick-panel 必需的事件/窗口能力，内部 worker 没有 WebView。UUID/来源标记不是调用者授权；不要因此向 UI 提供任意系统写入接口。

### 7.1 边界验证与规范形式

本节补齐共享契约的字段语义，状态为设计基线，不表示相关命令已经实现或验收。Rust 与前端原子升级，没有已发布的旧消费者；不为尚未部署的解析器提供兼容分支。

- 查询以 Rust `str::trim` 的 Unicode White_Space 集合为准：U+0009–000D、0020、0085、00A0、1680、2000–200A、2028、2029、202F、205F、3000。U+FEFF 不属于该集合。TS 显式镜像，不使用语义不同的 JS `trim`。内部空白不变；query/preview/text 的字符数采用 Unicode scalar values，拒绝未配对代理项。
- `clip_id`、summary.id、cursor.upper_id/last_id 的 wire 类型始终为字符串，规范范围为1..=9223372036854775807。不得先转 JS Number 再验证范围。revision 与累计计数为规范非负 u64 十进制字符串（允许单个 `0`，上限18446744073709551615，无前导零/符号/空白）。
- capture/session/operation 身份采用非空 ASCII graphic 标识，最多64字节；宿主生成并登记，形状合法不代表有效身份或权限。后续生成方式使用宿主实例UUID与单调操作计数，面板会话可用UUID；不能用截断正文或外部句柄充当身份。
- `list_clips`：缺失 limit 时为50，显式 null、非整数或越界均拒绝；缺失或显式 null 的 cursor 均表示首页。query/request_id 仍为必填，未知字段拒绝。两端返回相同的规范值；非空 cursor 的 last_id 不得大于 upper_id。
- worker `policy_epoch` 为0..=9007199254740991的 JSON 整数，由 Rust 校验原始 JSON；内部 worker 帧不经过 WebView，TS 不维护解析副本。超限、浮点或指数表示拒绝。递增达到上限时停止相关操作并报告 capacity_exceeded，不回绕或复用。其余worker协议仍由第5.2节规定。
- 原型热键的键名使用 A–Z、0–9、F1–F12、Space、Insert、Delete、Home、End、PageUp、PageDown，大小写按这些拼写；四个修饰键为必填布尔，至少一个为 true。仅Win+V仍拒绝。实际注册、冲突和物理按键处理留给相应工作包。

排除路径仅在本包做 Windows 词法校验/规范化，不访问文件系统或执行目标：支持 `C:\dir\app.exe` 与 `\\server\share\dir\app.exe`，拒绝普通/驱动/根相对路径、设备命名空间（`\\?\`、`\\.\`）、缺少server/share的UNC、控制字符及段内`<>:"|?*`、尾点/尾空格、末尾分隔符和非`.exe`末段（扩展名ASCII不区分大小写）。统一分隔符为反斜杠，盘符大写，折叠重复分隔符及`.`，解析`..`但不能越过盘根或UNC share根；保留其余路径字符和大小写。长度上限1024标量在输入和规范结果都检查。数据目录可复用完整路径规则，但不要求exe末段，允许目录根。这里只证明路径形状；真实规则保存/owner匹配时仍须核对规范进程路径，按Windows不区分大小写比较，不把词法通过称为文件存在或可执行证明。

### 7.2 完整响应 DTO

所有对象逐层拒绝未知字段；响应与事件的所有字段均必填，标为可空者允许显式null，不能省略。请求的缺省字段仅按第7.1节处理。以下为 wire 形状，Rust 可以保留内部整数类型，但用 serde 的专用字段转换/类型保证序列化后ID和计数仍是字符串。响应既要有真实类型/解析，也要有实际 serde 序列化回归，不能只定义返回unit的校验器。

`StatusResult`：

| 字段 | wire 类型与边界 |
| --- | --- |
| revision | 规范非负u64十进制字符串 |
| state | startup/listening/paused/overloaded/failed/stopping |
| services | 固定键listener、worker、storage、hotkey、target_observer；每项为not_started/starting/ready/failed/stopping/stopped |
| capture | user_enabled、admission_open均为布尔；policy_epoch按7.1。用户意图与接纳开关分开，不合成一个paused值 |
| counters | 固定键observed、captured、persisted、ignored、superseded、failed、overloaded、cancelled；每项为规范非负u64字符串，表示本宿主实例的累计数 |
| queue | items为0..64整数、bytes为0..33554432整数；仅已取得、待存的快照，不包括通知元数据或一份在途快照 |
| history | items为0..10000整数、payload_bytes为0..268435456整数；表示当前已提交占用 |
| data_dir | 非空规范Windows完整目录路径；不查询文件系统 |
| versions | app为非空ASCII graphic字符串、最多32字节；schema=1；worker_protocol=1。后两项是当前支持的版本，不冒充已成功打开数据库/握手 |
| last_paste | PasteClipResult或null；不包含正文、标题或任意注入参数 |

服务状态彼此独立；解析不得把storage.ready推导成所有服务就绪。E02仅定义合成夹具和校验；E06由真实服务构造get_status，并替换原get_shell_status壳命令，不用固定返回冒充运行状态。

`ListClipsResult`固定字段为request_id、normalized_query、summaries、next_cursor，Rust返回明确的结果结构。normalized_query必须已经符合7.1且最多256标量；summaries最多100项；整个结果JSON的UTF-8编码最多1MiB；cursor非空时绑定同一查询。与实际请求的limit/结果代次对应由调用方使用请求上下文复核，不能仅因形状通过就更新UI。

`GetClipTextResult`固定字段为request_id、clip_id（规范i64字符串）、text、next_offset。text为合法Unicode纯文本，UTF-8最多65536字节；next_offset为null或JS安全范围内非负整数。结合对应GetClipTextRequest验证request_id/clip_id回声；next_offset非空时，text必须非空，且next_offset严格等于请求offset加text的UTF-8字节数，计算须检查溢出。null表示正文结束，可返回空文本；正文真实长度和offset字符边界仍由存储读取路径验证，E02合成校验不冒充真实正文读取。

set_capture_enabled、set_excluded_apps、set_hotkey成功返回StatusResult；panel_ready/dismiss_panel成功返回JSON null，只代表已处理有效当前会话的请求。request_exit返回StatusResult（stopping或退出失败状态），实际已退出不能由响应形状证明。统一失败仍为code/message/retryable，不新增“永远成功”的入口。

事件沿用第7节，history/status revision使用上述u64字符串，panel session/operation使用短标识。PasteClipResult的双状态和reason语义不变。错误也使用真实serde/JSON序列化，转义由序列化器负责；夹具同时验证值、wire类型、未知字段拒绝与安全错误，不只统计accept/reject个数。

## 8. R1：目标、面板与回贴

### 8.1 两个窗口与基础交互

main：最小历史列表/预览、收录状态、热键和排除配置；不做完整管理页。quick-panel：一个可复用的预创建隐藏窗口，搜索输入+结果+状态区，约420×540逻辑像素，适应当前显示器工作区，不复制原型的假桌面和黄色控制条。页面沿用中性灰、晨曦粉、系统字体和减少动态偏好；首轮没有动画库依赖。

打开面板建立新的 session ID，清空旧搜索/选择/回贴；UI ready ACK 后搜索框聚焦。箭头选择，Enter 只处理已提交输入，Esc 关闭；compositionstart/end 控制中文输入，组合中的 Enter 不发 paste。鼠标单击只选中，不隐式回贴；可有明确“复制并回贴”按钮，仍走同一 Rust 检查路径。

### 8.2 原目标与变化观察

WM_HOTKEY 处理时，在显示面板前取得目标 HWND、root/owner、PID、线程、进程创建时间与可查的子控件焦点。窗口无效/自身窗口/管理员/权限未知时记录 manual_only 原因，不构造看似正常的 target。

原生线程用限定 EVENT_SYSTEM_FOREGROUND 的 SetWinEventHook、WINEVENT_OUTOFCONTEXT 观察前台变化；这是进程外 WinEvent 通知，不是 R8 的低级键盘钩子或 DLL 注入。回调只把安全的事件信息入队，持有句柄并在退出 UnhookWinEvent。观察失败则不自动回贴。

面板获得自身焦点属于预期；其他外部前台切换永久取消本次目标，即使随后又切回也不重启旧会话。回调延迟、事件时间早于本次唤出、时间回绕、同 HWND/PID 复用均须测试。每个外部边界前再读取真实前台与身份，不能仅相信 hook 事件已送达。

### 8.3 一次回贴操作的确定顺序

1. Rust 校验 session 当前、条目存在且有文本、目标有效及无外部切换；在状态机中占有唯一操作，重复 Enter 不产生第二次任务。
2. 从存储线程按 id 取完整文本；期间目标改变则取消。写回前再检查目标/会话，然后按5.4写所选纯文本。
3. 写入全部成功则记录 clipboard_written 和序列/operation；即使后续不能回贴，仍可根据当前剪贴板是否未被替换提示手动路径。
4. 隐藏面板，最多调用一次 SetForegroundWindow，实际前台在300 ms内变为原目标才继续；失败不 AttachThreadInput、不调前台锁。
5. 等 Ctrl/Alt/Shift/Win 物理修饰键释放，最多500 ms；等待中每次复核目标与取消。包括首轮热键里的 Alt，不合成释放用户仍按住的键。
6. 校验目标完整性级别与自身普通权限、子控件焦点、IME 状态及当前剪贴板 operation/序列。任何 unknown 都是降级原因；不得把 API 失败当成没有风险。
7. 最终紧邻 SendInput 再复核 session/窗口身份/前台/剪贴板；一次发送4个事件，检查数量。4只表示 instruction_sent；0/部分表示 injection_failed，永不自动重发整组。
8. 部分注入的清理只针对可证明由本操作产生且未混入用户按键的合成状态；不能仅用返回数量推断具体哪几个Down/Up已插入。不能证明时停止自动输入并告知用户检查修饰键，不再合成清理事件。UI结果显示复制状态和指令状态，不使用“已粘贴成功”。

预检到系统分发仍有竞态，没有绝对错贴零风险保证。按钮、排序变化和返回 Ok 都不证明目标消费；A1 人工逐个控件核对。

### 8.4 IME 与可行性前置关卡

GetGUIThreadInfo/ImmGetContext/ImmGetCompositionStringW 仅作为有明确适用范围的探测，不能用 null 上下文、负返回值或 ImmIsIME=false 证明没有 TSF 组合输入。只记录能否读取、是否有未提交内容及字节长度，不记录组合字符串。

W0 必须先在用户实际配置上做只读探针：浏览器、编辑器、终端各查目标/焦点/权限/IME，观察未提交中文时是否能可靠区分。返回明确的“可判定未组合/正在组合/未知”三态。未知则 manual_only，不能为打通演示伪造“安全”。如果三个目标无法得到符合 MVP 的自动回贴前提，当前 Codex 先提出修订建议供用户决定；不得悄悄把整个产品永久改成只复制，也不得安装注入钩子或清空输入法。

本机三目标观测已执行，结果均 unknown；证据见[探针说明](pinfeather_w0_probe_guide.md)。用户已批准先推进独立 W1/W2，W3 的本项前置条件仍未满足。没有批准放宽 unknown、引入目标插件/注入或永久只复制；首轮最终完成条件不变。

## 9. 故障、关闭与恢复

启动顺序：解析模式/参数 → 单实例 → 数据/设置检查 → 存储就绪 → worker握手 → 原生窗口/监听 → 热键/WinEvent → 发布就绪。每步都有反向回收，部分失败时已建资源也必须关闭。UI可在可诊断状态显示启动错误，但不能显示监听已就绪。

正常关闭：不接新操作 → stopping → 注销热键、监听和WinEvent → 取消/回收在途 worker → 保留并排空已接纳快照 → 提交/关闭 SQLite → 销毁消息窗/线程 → 退出Tauri。3 s 内未排空则保持进程在错误/暂停状态，显示未保存数量；用户可重试或明确放弃，不能后台静默退出。

用户点窗口关闭按钮隐藏到托盘；托盘退出调用同一 request_exit。第二次退出或启动中退出幂等；不存在每个窗口各清一遍全局资源。异常进程终止后 SQLite 已提交数据保留，内存队列不保证恢复；下次打开先检查，不自动重建损坏库。

存储设置与数据文件错误保留现场。回滚首轮实现只停止本应用并保留隔离数据，不使用 git reset --hard、git clean 或自动删目录；源码修复与复核由当前 Codex 负责人完成。无法确定副作用完成时先查当前状态，不盲目重试。

## 10. 分工与人工关卡

工作包 W0–W5 和精确任务见[任务文档](pinfeather_execution_tasks.md)。Codex 按包实现，随后检查真实 diff、独立复跑受影响检查并记录实际命令、未完成项；检查失败留在当前包，不靠后续功能掩盖问题。

2026-10-23 已确认的依赖例外：W0 的工程、契约、传输及回收证据允许 W1/W2 独立开发；IME provider 的 needs_plan_revision 单独阻塞 W3。W4/W5 的原依赖、真实剪贴板测试授权及最终验收要求不变，不把执行顺序调整记成 IME 已通过。

本次移交采用一个 Codex 会话串行写入，不再自动向 Mimo 派工。此前对 Mimo 子agent 的授权属于旧协作安排，不要求新会话启动子agent；不得为并行放宽审批、安装扩展或改变模型。

真实剪贴板、焦点/输入测试仍按验收计划由用户参与。需要多个写者或独立 worktree 时须另行明确安排隔离及未提交基线传递；当前不存在这样的并行写入授权。

## 11. 当前事实与审阅要点

首轮实现接手基线为 main / `19807067dbf4b80821937514bdf06ba1a2aa1d84`。用户已授权将完成的正式设计、工程和验证内容提交并推送至 origin/main；两个 copy 不改动、不删除、不提交。后续接手须核对实际 HEAD、dirty 清单与 hash，不能 checkout 旧基线丢掉方案和实现。E04 尚无存储源码或数据库操作，其未完成依赖快照仅存本地归档，不纳入发布。

已有最小 Tauri/Rust/React 工程、锁文件、契约与 fixture supervisor。E03a 的 F2/F3 已按失败回归修复；本会话自审及新进程重跑为 Rust lib 34 项、worker 集成 37 项、前端 28 项、typecheck 和正常 release 构建通过，未发现 fixture 残留。证据在 archive 的 runs/20261023-e03a-codex-owner-fixes/{execution,review}.md；这不是另一模型的独立 review。rustfmt/clippy 缺组件，未安装；保留检查缺口。此前复审报告保留原失败事实。

首轮仍要求启动默认暂停、一个用户一个宿主、W0 先验 IME 可行性、按包 review 以及真实应用人工验收。监听/SQLite/搜索/热键/回贴业务、启动和完整进程树内存均未验收；其余 Windows 组合不得宣称已支持。

E03b 自动部分已实现，见[只读探针说明](pinfeather_w0_probe_guide.md)。最新检查为 Rust lib 38、worker 集成 37、诊断 CLI 2 项；普通 release 构建通过。用户本次授权代运行工程壳复验后，无 UIA/只读 UIA 各 3 次相同单次 WM_CLOSE 均观察到进程树退出，UIA 核对真实目录和暂停状态；不等同人工点击或历史滞留根因已关闭。三目标真实候选已由用户确认，外部 IMM 方法无法判定；用户已批准独立 W1/W2 先行、W3 仍阻塞，W0 不标整体通过。

## 12. 官方依据与限制

- [GetClipboardData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboarddata)：系统句柄与持锁复制边界。
- [SetClipboardData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata)：内存所有权转移、真实owner HWND及延迟渲染边界。
- [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)：进程外事件交付与消息循环要求。
- [ImmGetCompositionStringW](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetcompositionstringw)：返回字节数与错误；不能外推到所有输入框/TSF。
- [Tauri单实例](https://v2.tauri.app/plugin/single-instance/)、[托盘](https://v2.tauri.app/learn/system-tray/)、[调用Rust](https://v2.tauri.app/develop/calling-rust/)：宿主装配与IPC参考。
- [shadcn手工安装](https://ui.shadcn.com/docs/installation/manual)：base-nova、主题及组件源码约定。

本文件的线程组织、队列/协议上限、分页与工作包安排是本项目设计；已有证据只覆盖上述实际检查，不外推为真实产品链路或 Windows 全矩阵已通过。

## 13. 决定与已排除路径

- 2026-10-23：E03a 控制 hello/ACK 限为 4 KiB、帧队列 2 项，避免用未来快照的 18 MiB 上限承载无界控制数据；原 18 MiB 空白 ACK 和 50 MiB 未请求帧反例见 archive/runs/20261023-e03a-codex-review。该轮拒绝旧反例，随后另以失败回归修复 fatal 状态优先级和取消确认，见下方 F2/F3 记录。
- 2026-10-23：用户改为新 Codex 全程负责；旧 Mimo 与原 Codex 停写，沿用当前 dirty 基线和证据，不再使用旧的职责拆分限制。完整移交使用 rumo-agent-collaboration；不恢复 agent-handoff 状态机。
- 2026-10-23：按真实失败回归关闭 E03a F2/F3；reader fatal 优先于合法 ACK，writer 纳入有界回收，未确认线程及句柄保留并禁重建。同步管道以 CancelSynchronousIo 请求取消，以原生线程句柄 signaled 确认结束；正常 EOF 与半帧分开。证据见 archive/runs/20261023-e03a-codex-owner-fixes。E03b/W0 真机前提仍待验。
- 2026-10-23：E03b 新增 feature-gated 只读探针；不把 IMM 零长度当作 TSF 未组合。经用户本次授权完成工程壳 6 次单次 WM_CLOSE 对照（无 UIA/只读 UIA 各 3 次），未复现旧滞留，保留原因未知。证据见 archive/runs/20261023-e03b-target-ime；不以自动关闭冒充人工目标/IME 验收，W0 未过。
- 2026-10-23：用户完成 Edge、VS Code、Windows Terminal 的真实候选输入并确认，529 条目标记录均无法读取外部 IMM context，IME 为 unknown。原方法不可作为通用安全预检；用户选择“先做 W1/W2，W3 仍阻塞”，只修改独立开发的依赖门槛，不放宽自动回贴规则或首轮最终标准。原始数据、具体提案及用户决定见 archive/runs/20261023-180555-ime-live。
- 2026-10-23：E04 拒绝库测试发现普通只读 WAL 连接仍会创建 sidecar；只读 exclusive 在本机返回 SQLITE_IOERR_LOCK。最终使用冻结文件与只读私有 WAL 索引预检，正常重开及 WAL 高版本无改动拒绝测试通过。未采用会忽略已提交 WAL 的无条件 immutable 打开；证据见 archive/runs/20261023-192601-e04-store-resume。
- 2026-10-23：用户再次改派当前Codex全程负责，Mimo停止E05；接手保留已批准base64依赖与Memory feature。E05协议、只读worker和监听/队列按5.2.2实现；自动检查不访问系统剪贴板，真实捕获仍待E07。淘汰积压通知时只比较序列相等，观察过不同序列后不复用先前数值的永久身份；反例与证据见 archive/runs/20261023-e05-codex-owner。
