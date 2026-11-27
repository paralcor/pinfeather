# 桌面运行与复制、搜索验收

功能、开发启动入口和已知限制见 [README](../../README.md#当前状态)。本页的 E07 步骤验证真实复制、保存、列表和重启。E09 的搜索、预览与重启记录见[验收计划第 6.2 节](pinfeather_acceptance_plan.md#62-e09-本机搜索与正文预览)。

## 启动与退出

在仓库根目录的 PowerShell 中执行。沿用当前执行策略，不更改系统配置。

```powershell
$pinfeatherTestRoot = '<隔离测试根的绝对路径>'
$pinfeatherRunId = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N')
$pinfeatherE07Run = Join-Path $pinfeatherTestRoot "runs\$pinfeatherRunId"
New-Item -ItemType Directory -Path $pinfeatherE07Run -Force | Out-Null
$pinfeatherE07Data = Join-Path $pinfeatherE07Run 'data'
$pinfeatherE07Exe = (Resolve-Path -LiteralPath '.\src-tauri\target\release\pinfeather.exe').Path
& $pinfeatherE07Exe --data-dir $pinfeatherE07Data --start-paused
```

先按 [CONTRIBUTING.md](../../CONTRIBUTING.md#构建与检查) 构建 Release；已有构建仅在来源、源码和校验值可核对时复用。不要同时运行开发宿主或原生剪贴板测试。应用拒绝第二实例，不自动关闭现有实例。

程序默认暂停。展开「收录诊断」，核对数据目录与本次 `data` 路径一致。初次启动应显示零条历史。点击「开始收录」，复制合成内容，观察列表与计数。结束时点击「暂停收录」，待界面确认且未保存队列归零后，点「退出片羽」。

重启时复用同一个 `$pinfeatherE07Data`，不要重新执行创建 run 目录的语句。程序仍应暂停，已提交记录应保留。保留整个 run 目录，不删除或重置失败现场。

开发启动可执行 `tools/dev.ps1 -DataDir $pinfeatherE07Data`。同一时间运行一个宿主，记录所用构建类型。

## 分步验收

每一步完成后记录操作时间、预期及实际条目、格式和队列计数。由助手只读核对本次测试库；不直接写 SQL 补数据或修改结果。

| 步骤 | 用户操作 | 通过条件 |
| --- | --- | --- |
| 初始暂停 | 保持暂停，复制 `E07-PAUSED-01 不应入库` | 历史仍为零；开始收录后不补收暂停期间的事件 |
| 文本 | 开始收录，复制 `E07-CN01 今天讨论剪贴板项目定位` | 出现一个对应条目，未保存队列最终为零 |
| 链接 | 复制 `https://example.invalid/e07/a_b?q=100%25` | 链接按文本保存，内容及格式可核对 |
| 外部同文 | 再次复制完全相同的 CN01 文本 | 新增独立条目，不因文字相同被当成内部写入 |
| 暂停 | 暂停确认后复制 `E07-PAUSED-02 不应入库` | 不新增条目；恢复后复制新的编号才入库 |
| 重启 | 暂停并等待队列归零，点「退出片羽」，用同目录重开 | 原条目的 id、时间、载荷一致。启动仍暂停，历史条数保持一致 |
| 多格式 | 开始收录，在浏览器复制带格式的合成段落 | 同一条目同时保存 CF_UNICODETEXT 和 HTML Format，不拆成两条；只读核对载荷关系 |
| PNG | 在支持复制 PNG 格式的应用中复制合成 PNG | 若提供 PNG 注册格式则关联保存；仅提供 DIB/DIBV5 的来源按不支持格式记录，不能据此宣称 PNG 链路通过 |

先完成文本和重启，再逐项进行多格式及 PNG。测试夹具属于本次 run；浏览器中的选择、复制和图片操作由用户完成。只保存必要的合成结果，不记录私人剪贴板内容、窗口标题或凭据。

## 快捷面板现场验收

1. 收录一段超过 64 KiB 的合成长文，保留首尾标记。暂停收录后，复制另一段短样本作为剪贴板初值。
2. 在记事本或常用编辑器放好插入点，按 `Ctrl+Alt+V` 打开面板。用微软拼音输入搜索词，确认候选期间 ↓ 用于候选选择。确认候选并释放按键后，待结果就绪，再按 ↓ 预览首条。
3. 在首段预览中按 Enter，等待面板收起。直接按 `Ctrl+V`，核对目标窗口、插入点和长文内容。记录主窗口可见、最小化或在托盘时的结果，以及通过主窗口按钮唤出的焦点落点。
4. 改用「复制纯文本」按钮，验证复制后保留预览。如果能在复制等待期间切换应用，记录焦点最终落点。

## 内部写回原生测试

此测试会替换真实系统剪贴板为 `Pinfeather isolated internal write`，不恢复旧内容。先关闭片羽宿主，再由用户手动执行以下命令。普通测试不启用此许可，也不执行 ignored 用例。

```powershell
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
$env:PINFEATHER_TEST_DATA_ROOT = Join-Path $pinfeatherE07Run ('internal-write-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $env:PINFEATHER_TEST_DATA_ROOT | Out-Null
$env:PINFEATHER_ALLOW_SYSTEM_CLIPBOARD_TESTS = '1'
try {
    cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --features test-fixtures --test capture_windows -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw '内部写回验收失败；保留测试目录与输出。' }
} finally {
    Remove-Item Env:PINFEATHER_ALLOW_SYSTEM_CLIPBOARD_TESTS -ErrorAction SilentlyContinue
}
```

通过条件为该项实际执行且通过，真实写入通知被判为内部来源，种子历史仍为一条，队列为空。它不覆盖外部复制交错、全部隐私标记、剪贴板占用或快速复制的真机验收；未测部分继续保持待办。

## 记录与判定

原始日志和结果放本次 run；记录 HEAD、工作区差异、exe SHA256、系统和实际来源应用版本、启动/退出 PID、步骤结果和失败原因。观察到进程存在只证明启动，用户看见并操作真实界面才构成相应 UI 证据。

完整矩阵见[验收计划](pinfeather_acceptance_plan.md#6-真实应用用例映射)。T2 仅在 E07 所需自动与人工证据齐全后勾选；人工未完成、出现漏记/自录、错误成功提示或重启丢数据时，保留现场并继续 E07 修复。
