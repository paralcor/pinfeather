# Rumo 的剪贴板技术设计候选方案 G：轻量驻留与历史存储

状态：初步设计，具体方案待定。本文是 Rumo 整理、供片羽项目评估的技术设计候选方案；模块划分、平台范围、接口和性能需要通过原型验证后确定。

下文模块名为暂定设计名称，代码块为示意代码，不代表已有实现。技术风险与待验证问题见[项目交接说明](pinfeather_project_handoff.md#5-原-mvp-技术评审)。

## 1. 候选架构与模块职责

拟评估 C++／Win32、本地数据库与按需加载，以控制常驻开销并支持较长历史。4–8MB 工作集、百万级条目和秒级检索是候选设计参考数值，实际指标待原型验证，不是片羽承诺或实测结果。

### 1.1 技术栈设想

- 语言候选：C++，评估静态构建与部署体积。
- 数据库候选：带 AES-128／AES-256 加密能力的嵌入式引擎；密钥管理与恢复待设计。
- 系统 API 参考：`AddClipboardFormatListener`、`GetClipboardData`、`EnumClipboardFormats`、`GetClipboardOwner`、`SendInput`、`AttachThreadInput`、`GetForegroundWindow`。

### 1.2 候选模块与职责

| 暂定模块名 | 拟承担的职责 |
| --- | --- |
| `ClipboardSupervisor` | 监听变化、协调抓取与事件处理 |
| `FormatFilter` | 按格式及应用策略过滤瞬态或敏感内容 |
| `DatabaseCore` | 保存元数据和载荷，处理事务及加密 |
| `FastIndexService` | 查询历史、收藏和快捷面板所需摘要 |

## 2. 重点机制与设计思路

### 2.1 分页索引与冷热分离

历史增长时，需要验证列表、搜索和载荷加载对内存与延迟的影响。初步设计从分页、摘要与载荷分离入手。

#### 查询接口设想

- `GetCountClipInDatabase()`：历史数量。
- `GetCountStarredClipInDatabase()`：收藏数量。
- `GetClipboardContentDataForLastMinusNClip(int n)`：按相对倒序位置查条目，具体索引和分页实现待定，函数名不证明避开了 OFFSET 扫描。
- `GetClipboardContentDataForQuickPaste()`：按需获取快捷面板数据。

#### 存储结构

摘要、校验值、时间与查询字段可保存于元数据表；图片、富文本及文件列表等多格式载荷独立关联保存。缓存上限、事务一致性、维护清理及集合保留规则待定。

### 2.2 内部复制、粘贴和恢复的过滤

可将内部写入区分为三个阶段，以下为暂定接口名：

1. `AddClipboardViewerIgnoreFormatWhenCopying`
2. `AddClipboardViewerIgnoreFormatWhenPasting`
3. `AddClipboardViewerIgnoreFormatWhenRestoring`

拟组合 `Clipboard Viewer Ignore` 标记和内部状态处理 `WM_CLIPBOARDUPDATE`。阶段标记和原子状态不能单独保证零死锁或不漏记，需要核对异步事件、外部复制与失败重试。

## 3. 关键技术机制

### 3.1 内存测量与按需加载

原生界面是控制运行时开销的候选方向，收益需与具体依赖及功能组合一起测量。

可评估隐藏窗口时的工作集管理。调用 `SetProcessWorkingSetSize` 进行裁剪会影响物理页驻留，不等同于释放应用数据；再次访问还可能增加缺页开销。4–8MB 仅作设计参考，需同时记录提交内存、峰值、相关进程与唤出延迟。

### 3.2 应用上下文

可用 `GetClipboardOwner` 与 `GetWindowThreadProcessId` 辅助判断复制应用，并评估应用排除规则。拥有者缺失、间接写入和密码管理器标记的覆盖范围需验证，不能保证仅凭进程名就排除所有敏感内容。

## 4. 预期收益与待验证问题

- 测量轻量常驻设计在真实机器与混合载荷下的表现。
- 用大历史验证分页与搜索，分别记录冷、热查询与图片加载开销。
- 验证三阶段过滤、清理和重启恢复，不把接口名称或工作集数字当作可靠性证明。
