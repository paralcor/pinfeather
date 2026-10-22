# Rumo 的剪贴板技术设计候选方案 B：脚本引擎与 CLI

状态：初步设计，具体方案待定。本文是 Rumo 整理、供片羽项目评估的技术设计候选方案；模块划分、平台范围、接口和性能需要通过原型验证后确定。

下文模块名为暂定设计名称，代码块为示意代码，不代表已有实现。技术风险与待验证问题见[项目交接说明](pinfeather_project_handoff.md#5-原-mvp-技术评审)。

## 1. 候选架构与模块职责

拟评估 C++／Qt、ECMAScript 和本地 IPC，为剪贴板提供命令行、脚本动作、标签页和格式访问能力。Windows、macOS、Linux 是跨平台参考范围，具体平台适配与片羽首版范围待定。

### 1.1 技术栈设想

- 语言与界面候选：C++17、Qt 5.15+ 或 Qt 6，版本与兼容范围待验证。
- 脚本候选：Qt 可集成的 ECMAScript 引擎，绑定 C++ 对象与槽函数；具体引擎及权限边界待选型。
- IPC 候选：Local Socket，Windows 命名管道 `\\\\.\\pipe\\...` 或 Unix domain socket。
- 扩展设想：音频提示、本地存储加密；具体实现待定。

### 1.2 候选模块与职责

| 暂定模块名 | 拟承担的职责 |
| --- | --- |
| `Scriptable` | 暴露剪贴板和条目操作；文件、网络能力是否开放需单独设计 |
| `ScriptableProxy` | 隔离脚本与 GUI 线程，协调跨线程调用和有界执行 |
| `Server` | 接收本地 CLI 请求，协调单实例与命令调度 |
| `ClientSocket` | 处理长度前缀、帧边界和流式消息 |
| `WinPlatformClipboard` | 监听 Windows 剪贴板并读取排除标记与序列号 |
| `WinPlatformWindow` | 恢复目标窗口，等待修饰键释放，选择回贴按键 |
| `Serialize` | 保存标签页与多格式字典，评估加密及版本兼容 |
| `TabWidget` | 组织按工作流区分的条目标签页 |

## 2. 重点机制与设计思路

### 2.1 ECMAScript 脚本绑定与执行边界

拟允许用脚本表达条目读取、变换和自定义动作。脚本与 CLI 可以共享能力，但是否允许任意脚本执行仍待设计，不能把语言引擎本身称为已实现的安全沙箱。

#### 接口设想

候选对象包括：

- 数据：`ByteArray()`、`File()`、`TemporaryFile()`、`Dir()`。
- 工作流：`ItemSelection()`、`Settings()`。
- 网络：`NetworkRequest()`、`NetworkReply()`。
- 剪贴板：`clipboard()`、`setClipboard()`、`read()`、`write()`、`tab()`、`select()`。

上述文件、网络与写入操作的授权、隔离、超时和终止机制待定，不能默认全部向用户脚本开放。

#### 调用与清洗示意

```cpp
// 暴露统一的剪贴板读取接口，支持任意 MIME 格式
QByteArray Scriptable::read(const QString &mime, int itemIndex)
{
    if (itemIndex < 0) {
        // 读取当前系统实时剪贴板
        return m_proxy->clipboardData(mime);
    }
    // 读取指定历史索引项的特定 MIME 格式
    return m_proxy->itemData(mime, itemIndex);
}

// 脚本过滤钩子：用户脚本可在条目入库前动态清洗
QJSValue Scriptable::evalAndCatch(const QString &script, const QString &label)
{
    clearExceptions();
    const QJSValue result = m_engine->evaluate(script, label);
    if (result.isError()) {
        log(QString("Script error [%1]: %2").arg(label, result.toString()), LogError);
    }
    return result;
}
```

### 2.2 CLI 与本地 IPC 分帧

候选命令示例为 `rumo-clip read text/plain`、`rumo-clip eval "tab('&Work'); add('Todo')"`。`rumo-clip` 是暂定示例名称，尚无可运行命令，也不是片羽已发布的接口。

#### 帧格式设想

拟采用长度前缀、消息码和载荷。以下四字节大端长度头是初步设计；帧大小上限、非法长度、版本、访问权限和截断连接的处理待定。

```text
┌─────────────────────────────────┬────────────────┬──────────────────────────┐
│  Message Length (quint32, 4B)   │  Message Code  │  Payload (QByteArray)    │
│  [大端字节序，表示后续包体长度] │  (qint32, 4B)  │  [由数据流序列化]        │
└─────────────────────────────────┴────────────────┴──────────────────────────┘
```

#### 帧解析示意

```cpp
void ClientSocket::onReadyRead()
{
    while (m_socket->bytesAvailable() > 0) {
        // 1. 读取 4 字节消息长度包头
        if (!m_hasMessageLength) {
            if (m_socket->bytesAvailable() < static_cast<qint64>(sizeof(m_messageLength)))
                return;

            QDataStream in(m_socket.get());
            in >> m_messageLength;
            m_hasMessageLength = true;
        }

        // 2. 校验包体是否完整接收
        if (m_socket->bytesAvailable() < m_messageLength)
            return;

        // 3. 读取完整载荷并派发给 Scriptable 引擎执行
        QDataStream in(m_socket.get());
        int messageCode = 0;
        in >> messageCode;
        in >> m_message;

        m_hasMessageLength = false;
        emit messageReceived(m_message, messageCode, m_socketId);
    }
}
```

## 3. 关键技术机制

### 3.1 Windows 排除标记与变化检测

可评估以下过滤示意。实际策略需要区分不进入历史、不上传云端及内部写入的不同含义：

```cpp
bool WinPlatformClipboard::isHidden(const QMimeData &data) const
{
    static const QByteArray zeroDWord(4, '\0');
    const QStringList formats = data.formats();

    return formats.contains(
            QLatin1String("application/x-qt-windows-mime;value=\"Clipboard Viewer Ignore\""))
        // 微软官方云剪贴板排除标准
        || formats.contains(
            QLatin1String("application/x-qt-windows-mime;value=\"ExcludeClipboardContentFromMonitorProcessing\""))
        // 限制进入 Windows + V 本地历史面板
        || contains(formats, data,
            QStringLiteral("application/x-qt-windows-mime;value=\"CanIncludeInClipboardHistory\""), zeroDWord)
        // 限制上传到 Microsoft 微软云端同步
        || contains(formats, data,
            QStringLiteral("application/x-qt-windows-mime;value=\"CanUploadToCloudClipboard\""), zeroDWord)
        // 内部格式
        || contains(formats, data,
            QStringLiteral("ExcludeClipboardContentFromMonitorProcessing"), QByteArrayLiteral("1"));
}
```

序列号可用于减少同一次变化的重复处理，但不能直接等同于内容去重、来源识别或持久化成功：

```cpp
void WinPlatformClipboard::onChanged(int)
{
    const DWORD newClipboardSequenceNumber = GetClipboardSequenceNumber();
    // 若系统底层序列号未递增，坚决拦截虚假更新
    if (newClipboardSequenceNumber == m_lastClipboardSequenceNumber)
        return;

    m_lastClipboardSequenceNumber = newClipboardSequenceNumber;
    emitConnectionChanged(ClipboardMode::Clipboard);
}
```

### 3.2 回贴与主动复制时序

- `waitForModifiersReleased`：拟有界等待用户松开修饰键，超时后的取消或降级行为待定。
- `Shift + Insert`／`Ctrl + V`：可针对终端和 GUI 应用配置按键，实际适配需分应用验证。
- 主动复制：发送 Ctrl＋C 后，可等待序列号变化并设置参考超时 2000ms；该数字待验证。序列号变化不证明是指定目标写入，也不证明保存成功，需结合操作上下文检查。

## 4. 预期收益与待验证问题

- 验证 Windows 历史和云端排除格式、敏感应用策略及内部写入过滤。
- 评估 CLI 与脚本共用能力的收益，定义权限和有界执行，避免无头模式绕过访问限制。
- 验证 IPC 分帧、重复命令、连接中断、跨线程调度及应用退出，不以固定等待替代完成条件。
