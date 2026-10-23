# Rumo 的剪贴板技术设计候选方案 F：输入感知与队列粘贴

状态：初步设计，具体方案待定。本文是 Rumo 整理、供片羽项目评估的技术设计候选方案；模块划分、平台范围、接口和性能需要通过原型验证后确定。

下文模块名为暂定设计名称，代码块为示意代码，不代表已有实现。技术风险与待验证问题见[项目交接说明](pinfeather_project_handoff.md#5-原-mvp-技术评审)。

## 1. 候选架构与模块职责

拟评估 Win32 输入事件、输入法状态与 FIFO／LIFO 队列粘贴。全局钩子是否必要、目标权限和系统架构适配均待验证。

### 1.1 技术栈设想

- Windows 原生实现；评估 32／64 位辅助进程和跨架构通信。
- 可选音频提示，用于热键及队列反馈。
- 可选任务栏入口，具体交互与系统兼容性待定。

### 1.2 候选模块与职责

| 暂定模块名 | 拟承担的职责 |
| --- | --- |
| `MainSupervisor` | 协调消息循环、历史窗口、模板与托盘 |
| `InputHookModule` | 评估键盘和消息事件监听；定义回调与释放 |
| `ArchBridge64` | 协调 64 位目标的辅助处理；位数适配不等同于权限提升 |
| `QuickToggle` | 提供轻量热键唤出入口，是否独立进程待定 |

## 2. 重点机制与设计思路

### 2.1 键盘与消息钩子状态感知

候选流程包含低级键盘钩子与 `WH_GETMESSAGE`。R8 的初次及重试分支均使用 Windows SDK 具名常量：`WH_KEYBOARD_LL` 为 13，10 是 `WH_SHELL`。此处只修正示例常量，回调签名、安装失败、超时、卸载及跨进程适用性仍未验证，不能直接运行或据此认定输入状态可可靠判断。片羽首轮使用正常全局快捷键注册，不引入本方案钩子。

```c
void HookInputPipeline(void)
{
    // 1. 设置共享数据区中的主窗口通知句柄
    if (in_stack_target != 0) {
        g_TargetHwnd = in_stack_target;
    }
    
    // 2. 挂载 WH_KEYBOARD_LL 底层按键钩子
    if (g_hKeyboardHook == (HHOOK)0) {
        g_hKeyboardHook = SetWindowsHookExW(WH_KEYBOARD_LL, (HOOKPROC)&KeyboardProc, g_hInst, 0);
    }
    if (g_hKeyboardHook == (HHOOK)0) {
        Sleep(5); // 仅保留旧骨架的一次重试，不代表已设计退避策略
        g_hKeyboardHook = SetWindowsHookExW(WH_KEYBOARD_LL, (HOOKPROC)&KeyboardProc, g_hInst, 0);
    }

    // 3. 挂载 WH_GETMESSAGE 消息流钩子
    if (g_hMessageHook == (HHOOK)0) {
        g_hMessageHook = SetWindowsHookExW(WH_GETMESSAGE, MessageProc, g_hInst, 0);
    }
    if (g_hMessageHook == (HHOOK)0) {
        Sleep(5);
        g_hMessageHook = SetWindowsHookExW(WH_GETMESSAGE, MessageProc, g_hInst, 0);
    }
}
```

拟通过键盘事件触发队列动作，并评估消息流能否补充 IME Composition 上下文。目标进程适配、跨位数限制、事件延迟及输入法差异需实测。

### 2.2 队列粘贴

1. 队列构建：用户多选历史条目后，按确定的顺序加入 FIFO 或 LIFO 队列。
2. 逐项触发：通过暂定快捷键（如 Ctrl＋Shift＋V）选择下一条，向当前输入目标回贴，方便配合 Tab 在字段间移动。

光标保留、出队时机、注入失败是否重试以及目标改变时是否取消，均需定义；不能把发送按键直接当作粘贴成功。

## 3. 关键技术机制

### 3.1 跨架构与权限

部分进程内钩子需要与目标位数匹配，可评估 64 位辅助进程及共享内存／命名管道通信。跨架构适配不解除 UIPI 限制，身份校验、资源所有者和退出顺序待设计。

### 3.2 输入法上下文

可研究 `ImmGetContext`、`ImmGetCompositionString` 对目标输入法状态的适用范围。发送 Escape 或清空 Composition 只是待评估的干预方式，可能丢失用户未提交的文字，不作为默认保护行为。

## 4. 预期收益与待验证问题

- 在表单填报等场景验证逐项粘贴是否减少操作。
- 验证键盘、输入法与目标窗口切换，比较有钩子和无注入监听的适用范围。
- 全局钩子可能增加兼容性与安全软件拦截风险；注册、超时、卸载和应用退出路径需要实测。
