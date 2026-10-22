# Rumo 的剪贴板技术设计候选方案 D：参数模板与卡片交互

状态：初步设计，具体方案待定。本文是 Rumo 整理、供片羽项目评估的技术设计候选方案；模块划分、平台范围、接口和性能需要通过原型验证后确定。

下文模块名为暂定设计名称，代码块为示意代码，不代表已有实现。技术风险与待验证问题见[项目交接说明](pinfeather_project_handoff.md#5-原-mvp-技术评审)。

## 1. 候选架构与模块职责

拟评估 Tauri／Rust 与 React／TypeScript 下的参数模板、材料集合、卡片管理和快捷面板。界面表现、系统适配及整体资源占用需要原型验证。

### 1.1 技术栈设想

- 宿主候选：Rust、Tauri 和系统 WebView。
- 前端参考：React 18、TypeScript、TailwindCSS、状态管理与拖拽组件；具体版本和依赖待选型。
- 系统能力：事件监听、多格式读取和写回、前台应用信息、输入合成、本地持久化。
- 平台适配按能力分别验证，不因界面跨平台就认定剪贴板与回贴行为一致。

### 1.2 候选模块与职责

| 暂定模块名 | 拟承担的职责 |
| --- | --- |
| `ClipboardMonitor` | 接收剪贴板事件，捕获上下文，评估重复复制行为 |
| `ClipboardCommands` | 为 WebView 提供格式、历史分页和条目操作 |
| `ClipViewTemplate` | 解析插槽，关联内置值和用户参数 |
| `ClipViewForm` | 显示参数表单，支持填写、预览和取消 |
| `CollectionStore` | 管理材料集合、分类与卡片状态 |
| `QuickPasteMain` | 提供快捷面板入口，与管理界面共用后台数据 |

## 2. 重点机制与设计思路

### 2.1 参数插槽与表单

候选语法为 `{{ slot }}`。用户可保存带变化参数的 SQL、邮件、指令或代码片段，填写参数后生成内容，再决定复制或回贴。

#### 分词与展示示意

```typescript
const templateFieldRegex = /\{\{\s*(.*?)\s*\}\}/g;

const renderWithBadges = (
  value: string,
  templateFields: TemplateField[],
  clipboardValue: string | null,
  globalTemplates: GlobalTemplate[]
) => {
  // 基于双花括号分割文本段落与插槽变量
  const parts = value.split(templateFieldRegex);

  return parts.map((part, index) => {
    // 1. 系统内置宏: {{ clipboard }} 动态拉取实时剪贴板
    if (part.toLowerCase() === 'clipboard') {
      return <Badge variant="clipboard">{clipboardValue}</Badge>;
    }

    // 2. 局部表单变量: 匹配用户自定义的输入占位符
    const matchedField = templateFields.find(f => f.label?.toLowerCase() === part.toLowerCase());

    // 3. 全局共享模板变量
    const matchedGlobal = globalTemplates.find(gt => gt.name?.toLowerCase() === part.toLowerCase());

    return <TemplatePill key={index} field={matchedField || matchedGlobal} />;
  });
};
```

此示例仅表达分词与参数展示思路。普通文本渲染、重复变量、保留字、缺失参数和字面双花括号的处理待完善。

#### 表单流程设想

1. 扫描待填写参数。
2. 显示表单，可评估文本、掩码和数字字段。
3. 合成并预览结果，由用户确认复制或回贴；取消时保留原内容。

字段校验、剪贴板值的取值时机及回贴失败行为待定义，密码掩码不代表内容已获安全存储。

### 2.2 Double Copy 与双窗口

#### 重复复制收藏设想

可将短时间内重复复制相同内容作为收藏手势候选。时间窗、应用重复写入导致的误判和是否默认启用需验证：

```rust
if should_auto_star && is_duplicate_consecutive_copy {
    // 自动为条目打上 is_favorite = true 标记，并推送到置顶看板
    history_service::star_item_by_id(last_item_id);
}
```

#### 窗口分工

- 管理窗口：集合、标签、模板及备份恢复入口。
- 快捷面板：较小边框、无任务栏图标、失焦隐藏；状态与后台协调。
- 低于 50ms 仅为候选设计参考值，冷启动、热唤出与不同机器的指标待原型测量。

## 3. 关键技术机制

### 3.1 大对象传递

可在 Rust 侧保存图片文件或内存块，前端先接收摘要与元数据，再按需读取图片。自定义协议和流式加载可减少 JSON 传输，但路径访问范围、临时资源清理与峰值内存待验证。

### 3.2 复制上下文

拟记录可取得的应用名（如 `chrome.exe`、`code.exe`）、窗口标题和图标。事件时的前台窗口不一定就是剪贴板写入者，信息缺失、归属判定及隐私设置需设计。

## 4. 预期收益与待验证问题

- 验证指令模板与材料集合的组合是否减少重复输入。
- 用真实应用测试重复复制手势，评估误判与用户控制方式。
- 测量快捷面板、管理窗口和 WebView 相关进程；与其他架构比较前不宣称资源优势已成立。
