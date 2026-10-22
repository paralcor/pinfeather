# Rumo 的剪贴板技术设计候选方案 A：原生 Win32

状态：初步设计，具体方案待定。本文是 Rumo 整理、供片羽项目评估的技术设计候选方案；模块划分、平台范围、接口和性能需要通过原型验证后确定。

下文模块名为暂定设计名称，代码块为示意代码，不代表已有实现。技术风险与待验证问题见[项目交接说明](pinfeather_project_handoff.md#5-原-mvp-技术评审)。

## 1. 候选架构与模块职责

拟以 C++ 和 Win32／OLE 组织 Windows 原生剪贴板能力，评估原生界面、事件监听、格式存储和回贴行为。低开销和快速响应是设计方向，实际表现待测。

### 1.1 技术栈与平台设想

- 语言候选：C++17。
- 平台参考范围：Windows 7–11、x86／x64；片羽首轮支持范围仍需单独确定。
- 持久化候选：SQLite Multiple Ciphers，评估整库加密及 BLOB 存储；密钥管理待设计。
- 压缩候选：`zlib`、`lz4`，用于大对象存储或传输。
- 界面候选：Windows RichEdit 与自绘控件。

### 1.2 候选模块与职责

| 暂定模块名 | 拟承担的职责 |
| --- | --- |
| `ClipboardViewer` | 监听剪贴板变化，优先评估 `AddClipboardFormatListener`，旧式监听链仅作为兼容候选 |
| `ExternalWindowTracker` | 记录前台窗口及焦点子控件，为回贴提供目标上下文 |
| `SendKeys` | 检测修饰键状态并合成输入；等待释放与合成 KeyUp 的使用边界待验证 |
| `ProcessPaste` | 封装 OLE 数据、拖放与内部写入标记 |
| `ClipboardSaveRestore` | 抓取、保存和恢复剪贴板格式快照 |
| `UAC_Helper` | 检查自身和目标进程的权限差异 |
| `UAC_Thread` | 评估经用户提权的辅助进程及其回贴通信 |
| `DatabaseUtilities` | 初始化表、索引及必要的关联维护 |

### 1.3 事件循环与生命周期设想

拟由消息循环接收 `WM_CLIPBOARDUPDATE`，必要时评估 `WM_DRAWCLIPBOARD` 兼容路径。磁盘 I/O 放入工作线程，避免阻塞界面；监听注销、线程停止、队列排空及窗口销毁顺序待定义。

## 2. 重点机制与设计思路

### 2.1 目标窗口恢复与修饰键处理

目标是在用户选择条目后，将内容准确回贴到此前的输入位置。下面保留前台锁设置、线程输入附加、KeyUp 和提权代理的流程设想；这些机制的可靠性与副作用待验证，不作为默认回贴路径。

```text
[用户双击条目]
      │
      ▼
1. 抓取预存的前台窗口: m_activeWnd, m_focusWnd (来自 ExternalWindowTracker::TrackActiveWnd)
      │
      ▼
2. 状态机清空物理修饰键: SendKeys::AllKeysUp() (强制发送 L/R Shift, Ctrl, Alt KeyUp)
      │
      ▼
3. 绕过 Windows 前台锁定: SPI_SETFOREGROUNDLOCKTIMEOUT -> 0
      │
      ▼
4. 线程输入附加 (AttachThreadInput) 并激活窗口: BringWindowToTop -> SetForegroundWindow
      │
      ▼
5. 还原系统前台超时设置
      │
      ▼
6. 检查目标是否提权 (UAC_Helper::PasteAsAdmin):
      ├─ 是 ──> 唤醒 runas 提权代理进程触发 SendInput
      └─ 否 ──> 本地 SendInput(Ctrl + V)
```

#### 前台窗口恢复示意

```cpp
bool ExternalWindowTracker::ActivateTarget()
{
    if (IsIconic(m_activeWnd)) {
        ShowWindow(m_activeWnd, SW_RESTORE);
    }

    // 突破 Windows 前台锁定超时: 记录原始超时并将当前设为 0
    PVOID timeoutMS = 0;
    SystemParametersInfo(SPI_GETFOREGROUNDLOCKTIMEOUT, 0, &timeoutMS, 0);
    SystemParametersInfo(SPI_SETFOREGROUNDLOCKTIMEOUT, 0, (PVOID)0, 0);

    // 若当前非前台窗口，通过 AttachThreadInput 强行绑定输入队列
    bool detach = false;
    DWORD foreGroundProcessId = GetWindowThreadProcessId(::GetForegroundWindow(), NULL);
    if(foreGroundProcessId != GetCurrentThreadId()) {
        if(AttachThreadInput(foreGroundProcessId, GetCurrentThreadId(), TRUE)) {
            detach = true;
        }
    }

    BringWindowToTop(m_activeWnd);
    SetForegroundWindow(m_activeWnd);

    if(detach) {
        AttachThreadInput(foreGroundProcessId, GetCurrentThreadId(), FALSE);
    }

    // 恢复系统原前台超时设置
    SystemParametersInfo(SPI_SETFOREGROUNDLOCKTIMEOUT, 0, (PVOID)timeoutMS, 0);
    return true;
}
```

#### 修饰键处理示意

拟比较等待用户松键与合成 KeyUp 的行为。下面示例会主动改变按键状态，不能据此认定物理按键已释放或不会串键。

```cpp
void SendKeys::AllKeysUp()
{
    // 检查所有常见控制键的物理状态
    for (int key : {VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN}) {
        short val = GetKeyState(key);
        if (val & 0x8000) {
            SendKeyUp(key);
        }
    }

    // 强行向系统队列灌入独立左右修饰键的 KeyUp 事件，防止驱动层按键挂起
    SendKeyUp(VK_LSHIFT);
    SendKeyUp(VK_RSHIFT);
    SendKeyUp(VK_LCONTROL);
    SendKeyUp(VK_RCONTROL);
    SendKeyUp(VK_LMENU);
    SendKeyUp(VK_RMENU);
}
```

### 2.2 权限差异与辅助进程设想

普通进程向较高完整性级别窗口注入输入可能受 UIPI 限制。拟通过 `OpenProcessToken`、`GetTokenInformation(TokenElevation)` 检查差异；以下只是判断逻辑示意，实际还需明确目标身份与完整性级别。

```cpp
bool UAC_Helper::PasteAsAdmin(HWND hWnd)
{
    bool runningElevated = AmIRunningElevated();
    bool theirRunningElevated = AreTheyRunningElevated(hWnd);

    // 自身非提权，但目标是提权程序时，触发提权代理通道
    return (!runningElevated && theirRunningElevated);
}
```

可评估通过 `ShellExecuteEx` 的 `runas` 启动辅助实例，使用暂定参数 `/uacpaste:<PID>` 协调输入。辅助进程身份校验、命名对象访问权限、退出及失败恢复待设计。

```cpp
// 初始化命名全局同步事件
AddEvent(UAC_PASTE, StrF(_T("Global\\Rumo_UAC_PASTE_%d"), m_processId));
AddEvent(UAC_COPY,  StrF(_T("Global\\Rumo_UAC_COPY_%d"),  m_processId));
AddEvent(UAC_EXIT,  StrF(_T("Global\\Rumo_UAC_EXIT_%d"),  m_processId));
```

`Global\\Rumo_UAC_*` 是暂定事件命名，不是片羽已发布的 IPC 契约。辅助进程仍需检查目标状态和输入注入结果，不能仅凭事件触发认定粘贴成功。

## 3. 关键技术机制

### 3.1 监听与内部写入过滤

优先评估 `AddClipboardFormatListener`；若确定需要旧系统兼容，再评估动态加载及 `SetClipboardViewer` 回退。监听竞争、重试和消息顺序需要验证。

可用注册格式标记内部写入：

  ```cpp
  m_cfIgnoreClipboard = ::RegisterClipboardFormat(_T("Clipboard Viewer Ignore"));
  ```

`Clipboard Viewer Ignore` 可作为过滤信号之一；其他应用是否设置或遵守该标记需要验证，不能用它保证所有敏感内容都会被排除。

### 3.2 多格式存储与冷热分离

候选模型将摘要与载荷分开，并建立一对多关系：

- `Main`：条目 ID、时间、校验值、搜索文本、热键及排序。
- `Data`：通过 `lParentID` 关联条目，每种格式保存一份 `strClipBoardFormat` 与 `ooData`。

列表拟只读取摘要，回贴时再加载需要的 BLOB 并恢复合适的数据句柄。大对象峰值内存、格式差异和事务一致性需要实测。

### 3.3 焦点子控件上下文

可评估 `GetGUIThreadInfo` 与 `hwndFocus` 补充 `GetForegroundWindow`，记录焦点子窗口。应用内部控件和浏览器输入位置是否可可靠恢复，需要分应用测试。

## 4. 预期收益与待验证问题

- 比较普通窗口激活与修改前台锁设置的行为，检查失败返回值和恢复路径。
- 验证松键等待、合成 KeyUp、输入法组合输入及用户切换窗口时的行为。
- 验证过滤标记、多格式快照、剪贴板占用和辅助进程退出，不以“无死锁”描述未测试的结果。
