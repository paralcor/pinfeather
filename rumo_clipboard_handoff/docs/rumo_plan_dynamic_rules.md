# Rumo 的剪贴板技术设计候选方案 E：动态宏与规则流水线

状态：初步设计，具体方案待定。本文是 Rumo 整理、供片羽项目评估的技术设计候选方案；模块划分、平台范围、接口和性能需要通过原型验证后确定。

下文模块名为暂定设计名称，代码块为示意代码，不代表已有实现。技术风险与待验证问题见[项目交接说明](pinfeather_project_handoff.md#5-原-mvp-技术评审)。

## 1. 候选架构与模块职责

拟评估 .NET、Roslyn 与触发器规则，表达剪贴板文本清洗、自定义宏和应用条件匹配。是否引入完整脚本运行环境及其成本待验证。

### 1.1 技术栈设想

- 运行时参考：.NET 8.0 x64；部署、编译和后续版本选择待验证。
- 脚本候选：Microsoft.CodeAnalysis（Roslyn），评估 C#／VB 编译及编辑器提示能力。
- 图形候选：DirectX 互操作与 GPU 字形渲染。
- 同步候选：SignalR／WebSocket 的服务端协调模式；部署及账号鉴权方案待定，不表示片羽已有云服务。
- 序列化候选：二进制格式，用于快照和大对象存储；是否采用零拷贝方式需实测。

### 1.2 候选模块与职责

| 暂定模块名 | 拟承担的职责 |
| --- | --- |
| `CoreSupervisor` | 监听变化并调度文本清洗、规则和宏任务 |
| `ScriptingSDK` | 向脚本提供经过授权的剪贴板、条目及窗口操作 |
| `RulesEngine` | 按应用、文本表达式和格式匹配动作 |

## 2. 重点机制与设计思路

### 2.1 Roslyn 动态宏与执行边界

拟允许用户用 C# 表达清洗和提取逻辑。网络、文件与系统交互若需开放，应另行设计权限、隔离、超时及终止机制；动态编译不等于安全沙箱。

#### API 门面示意

`Rumo` 和后文 `RumoScripting` 是暂定示例名称，不表示已有 SDK 或片羽正式接口。

```csharp
// 脚本默认继承的基础门面
public static class Rumo
{
    public static class Clipboard
    {
        // 剪贴板核心操作
        public static string GetText();
        public static void SetText(string text);
        public static bool HasText();
        public static void CopyText(string text);
        
        // 历史记录与收藏项穿透
        public static string GetHistoryText(int index);
        public static string[] GetAllHistoryText();
        public static string GetLastHistoryText();
        public static void AddHistoryText(string text);
        
        // 本地钉选与标签操作
        public static string GetLocalPinnedText(int index);
        public static void AddLocalPinnedText(string text);
        public static string[] GetAllLocalPinnedText();
        
        // 窗口与交互自动化
        public static IntPtr GetWindowByText(string windowTitle);
        public static IntPtr[] GetWindowsByText(string windowTitle);
    }
}
```

#### 编译与执行流程设想

以下流程保留“编译、加载、执行、变换或丢弃”的初步设计：

```text
[剪贴板变更或快捷键触发]
      │
      ▼
1. 提取剪贴板当前原始数据 (CF_UNICODETEXT / HTML / RTF)
      │
      ▼
2. 调度 Roslyn 编译器:
   CSharpCompilation.Create(assemblyName)
   .AddReferences(mscorlib, System, RumoScripting)
   .AddSyntaxTrees(CSharpSyntaxTree.ParseText(userScriptCode))
      │
      ▼
3. 内存加载 Emit 到 Byte[] 并在内存中反射实例化
      │
      ▼
4. 执行入口方法: public static string ProcessText(string text, string parameter)
      │
      ▼
5. 拦截或清洗结果:
   ├─ 返回 null ──> 阻断本次复制 (Drop Clipboard)
   └─ 返回 newText ──> 写回系统剪贴板 (SetClipboardData)
```

示例中的返回 null 可表达“不保存或不继续处理”，不能据此认定已经阻止其他应用写入系统剪贴板。重复编译、程序集卸载、异常和取消行为待定义。

### 2.2 条件规则与文本清洗

候选操作包括：

- 去除 HTML 标签、生成纯文本。
- 修剪空白、合并连续换行、转换制表符。
- 选择性移除 URL 中的 `utm_source`、`spm`、`from` 等参数。

清洗是否自动执行、原文是否保留、链接参数是否影响实际功能，需要按操作分别确定。

候选 Trigger 结构：

| 部分 | 初步设计 |
| --- | --- |
| Match | 窗口标题、进程名、文本正则及格式条件 |
| Action | 清洗、执行宏、提示音、发送到设备组或保存 |
| Stop Processing | 控制是否继续后续规则 |

## 3. 关键技术机制

### 3.1 监听与防重复处理

可评估 `AddClipboardFormatListener`、内部操作状态和 `Clipboard Viewer Ignore` 标记。时间窗布尔状态不能单独保证防自激，需验证异步通知、外部同时复制和重试行为。

### 3.2 序列化与同步

可比较二进制与 JSON／XML 在实际载荷下的开销。格式版本、兼容读取和失败恢复需要设计，不能仅凭格式名称认定零拷贝或性能提升。

## 4. 预期收益与待验证问题

- 评估动态宏对重复处理的帮助，以及完整运行时和编辑器的维护成本。
- 验证脚本权限、规则顺序、短路、失败隔离和用户取消。
- 比较真实数据下的编译、加载、序列化吞吐及内存，不把候选设计描述成已测性能。
