# W0：目标/IME 与工程壳退出复验

状态：E03a 合成传输与回收已通过；E03b 已在本机三类真实目标观察到用户确认的候选，但外部 IMM 上下文均不可读，IME 子关卡为 needs_plan_revision。用户已批准独立 W1/W2 先行、W3 继续阻塞；不把 W0 整体或首轮标为通过。当前工程仍无剪贴板业务、全局热键、数据库或回贴入口。

## 工具与边界

`src-tauri/src/bin/target_probe.rs` 是仅 `test-fixtures` 构建的诊断入口，不进入普通 release。它调用 `platform/windows.rs` 的只读采样，输出 schema_version=1 的 JSONL；当前消费者只有本次人工验收及其报告，没有部署中的旧消费者。变更诊断格式时递增版本，旧记录保持原义。诊断文件不会自动迁移或覆盖。

记录 HWND/root/owner、PID/线程/进程创建时间、可查焦点、exe 文件名、架构代码、完整性级别/提升/UIAccess、键盘布局及 IMM 返回长度。不读取窗口标题、控件正文或组合字符串，不操作前台、输入或剪贴板。采样结束前复查前台、焦点与进程，变化时状态为未知；一次采样不能排除之后的目标变化或 HWND 复用。

IME 判定是三态。正的合法 UTF-16 字节长度可报告正在组合；null context、负值、奇数字节、释放失败、焦点不稳定均为 unknown。**本次 IMM-only 探针没有能排除 TSF 的 provider，因此即使长度为 0，也保持 unknown，不产生 no_composition。** `ImmIsIME=false` 同样不能证明没有组合。原始观测用于判断该方法的可行性，不构成回贴许可，也没有把产品永久改成只复制。

官方依据：[GetGUIThreadInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo)、[ImmGetContext](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetcontext)、[ImmGetCompositionStringW](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetcompositionstringw)。IMM 返回值描述当前上下文，不能直接外推所有 TSF 输入框。需要人工核对未提交状态和具体配置；三个目标均无法满足自动回贴前提时标 needs_plan_revision，由用户决定方案变化。

复用已缓存的 windows-sys 0.61.2，仅增加 SystemInformation、Input/Ime、Input/KeyboardAndMouse、WindowsAndMessaging features，Cargo.lock 未改变。没有安装依赖或系统组件。

## 构建与独立运行

从仓库根运行，沿用当前权限/执行策略，只在当前 shell 补 cargo PATH：

```powershell
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
cargo build --locked --offline --manifest-path src-tauri/Cargo.toml --features test-fixtures --bin target_probe
```

诊断必须显式设置 `PINFEATHER_TEST_DATA_ROOT` 为已存在的 archive 子目录。参数先校验，再以 create_new 创建新文件；输出名不能包含路径。默认每次生成不同文件名。参数范围为 seconds 1–180、interval-ms 50–1000、delay-seconds 0–10；这是采样循环设置，不保证外部 Windows API 在固定时延返回。

```powershell
$env:PINFEATHER_TEST_DATA_ROOT = '<本次 archive 的新隔离目录>'
.\src-tauri\target\debug\target_probe.exe --label browser-idle
```

label 描述计划场景，不能替代用户实际操作。`process_machine=0` 表示进程使用本机架构；`native_machine=34404`（0x8664）为 AMD64。版本、位数和输入法配置均需连同结果记录。

## 本次用户操作入口

一次性脚本和结果全部在：

`D:\Develop\Misc Workspace\archive\pinfeather-core-prototype\runs\20261023-e03b-target-ime\`

脚本已准备好，助手没有运行其中的桌面操作。请在普通 PowerShell 中运行，不加 Bypass、不提权；执行策略阻止时保留报错。

先做三类应用的空闲/组合矩阵：

```powershell
& 'D:\Develop\Misc Workspace\archive\pinfeather-core-prototype\runs\20261023-e03b-target-ime\manual-target-matrix.ps1' -RepoRoot 'D:\Develop\orcamisk\开发\pinfeather'
```

按脚本依次准备 Edge/Chrome 测试输入框、VS Code 新未保存文件、Windows Terminal 的安全接收区。每个目标先空闲，再输入 `nihao` 保持中文候选未提交。启动每组采样后有 5 秒自己切回目标，保持约 12 秒。终端不执行样本文字或换行，组合结束后由用户自己取消。记录应用/IME 版本及每组实际状态；脚本不会帮忙切焦点或输入。结果进入新建的 `manual-targets-*` 目录。

再做工程壳退出复验：

```powershell
& 'D:\Develop\Misc Workspace\archive\pinfeather-core-prototype\runs\20261023-e03b-target-ime\manual-shell-exit.ps1' -RepoRoot 'D:\Develop\orcamisk\开发\pinfeather'
```

该脚本由用户主动运行后，在新隔离目录以 `--start-paused` 打开正常 release。先采集自身进程树，再提示用户核对真实目录/暂停状态并亲自点击 X 关闭；回控制台按 Enter 后只读观察 30 秒，不使用 UIA、WM_CLOSE 或强杀。按 PID/创建时间追踪已观察的后代，不能把未观察到的短生存进程或其他应用混作退出证据。`observed_tree_exited=true`、`observed_webview=true`、用户目视正确须分别检查，脚本不自动宣布 W0 通过。结果进入 `manual-exit-*`；建议相同方式做 3 次。若残留，先保留现场，不把原因写成 UIA，也不把强杀记作正常退出。

管理员目标及权限查询失败、目标关闭后的身份复核仍需另记录；用户自行准备管理员窗口，不由助手提权。核心矩阵未做之前，不把普通前台 smoke 算作三目标验收。

## 已验证与待办

自动部分已有 Rust lib 38 项、诊断 CLI 2 项通过；实际 smoke 采到 4 条 Orca.exe 普通权限观测，IME 为 unknown。拒绝覆盖已有 JSONL（原 hash 不变）和非法 duration 零文件副作用已实测。正常 release 构建通过。这些不证明 IME 未组合、真实目标可自动回贴、工程壳人工关闭或 W0 通过。

用户随后给予本次代运行权限，当前 release 已完成相同单次 WM_CLOSE 的两组退出复验：无 UIA 3 次、只读 UIA 3 次，全部观察到已识别宿主与 WebView2 后代退出；UIA 组均读到正确隔离目录和“已暂停”，实际 WebView2 为 155.0.4283.62。日志在 archive 的 `auto-close-20261023-165425-61741614` 和 `auto-close-uia-20261023-165745-9afe5aee`。这是受控自动关闭，不是人工点击 X、启动速度或内存验收；历史 75 s 滞留未复现，不能据此归因 UIA或宣称根因已解决。

2026-10-23 真实 IME 结果：用户在 Edge 155.0.4283.62、VS Code 1.143.1、Windows Terminal 1.24.12741.0 使用 Windows 自带中文输入法，并逐一确认候选出现。三目标分别有 142、151、236 条身份稳定/普通权限记录，全部 context_present=false、ime=unknown。证据为 archive/runs/20261023-180555-ime-live/{result.json,execution.md}；没有逐行组合阶段标签，不报告状态分类准确率，兼容模式未确认。

当前外部 IMM 链路不能满足自动回贴预检，不把 unknown 当作未组合。微软明确[输入上下文的创建线程限制](https://learn.microsoft.com/en-us/windows/win32/intl/developing-ime-aware-multiple-thread-applications)；本次没有证据证明所有替代 provider 都不成立。用户已选择先做 W1/W2、W3 仍阻塞，只调整依赖顺序，未知 IME 禁自动回贴与首轮最终验收标准不变。

管理员/权限未知/关闭目标及另列的人工目视/点击仍未覆盖。所有真实候选输入由用户完成；助手没有输入注入、强切前台或读写剪贴板。rustfmt/clippy 缺组件，仍为检查缺口，未安装。
