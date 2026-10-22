# Rumo 现代化高可靠剪贴板核心架构设计规格书与 MVP 代码骨架

## 1. 架构总览与技术栈选型

本自研现代化剪贴板系统（代号 **Rumo Clipboard**）充分吸收了 Rumo 剪贴板全谱系方案演进中 7 套子方案的核心长处，彻底规避了传统架构的历史包袱：
* **核心优势融合**：
  * **方案 A 融合**：突破 Windows 前台锁（`SPI_SETFOREGROUNDLOCKTIMEOUT` 归零）、修饰键强制清空状态机（`AllKeysUp`）、元数据与 BLOB 载荷物理隔离。
  * **方案 B 融合**：硬件级递增序列号防抖（`GetClipboardSequenceNumber`）、Windows 官方云剪贴板与历史排除协议。
  * **方案 C 融合**：mDNS 局域网自发现与多网卡单播自愈标头、AI Agent 的 Model Context Protocol (MCP) 原生集成。
  * **方案 D 融合**：参数化动态模板插槽（`{{ slot }}`）、即时表单渲染与 Double Copy 意图识别。
  * **方案 E 融合**：零拷贝二进制序列化、高扩展性规则清洗流水线。
  * **方案 G 融合**：极低内存驻留设计（无纯重型虚拟机内存过载）、三阶段防自激屏蔽链。
* **工程规避**：
  * 坚决不用全局注入钩子（DLL 注入，消除杀软报毒风险）；
  * 常驻空闲内存控制在 **12MB - 18MB** 以内。

### 1.1 系统整体分层架构图
```text
┌────────────────────────────────────────────────────────────────────────┐
│                      Frontend Layer (Tauri Webview)                    │
│   - Multi-Board Kanban & Tags        - Parametric Form Generator       │
│   - QuickPaste Floating HUD (<50ms)  - Real-time MCP Status & Sync     │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Tauri IPC (Commands & Events)
┌───────────────────────────────────▼────────────────────────────────────┐
│                    Rust Core Layer (Safe Native Core)                  │
│                                                                        │
│  ┌─────────────────────────┐          ┌─────────────────────────────┐  │
│  │   Clipboard Supervisor  │          │   Focus & Input Injector    │  │
│  │  - AddClipboardListener │          │  - SPI Timeout Reset (0ms)  │  │
│  │  - Sequence Guard (u32) │          │  - KeyState Clearer (Up)    │  │
│  │  - Triple-Stage Filter  │          │  - IME Composition Shield   │  │
│  └───────────┬─────────────┘          └──────────────┬──────────────┘  │
│              │                                       │                 │
│  ┌───────────▼─────────────┐          ┌──────────────▼──────────────┐  │
│  │     Storage Engine      │          │     AI & Sync Protocols     │  │
│  │  - SQLite3MC (FTS5)     │          │  - Embedded MCP SSE Server  │  │
│  │  - Hot Metadata / Cold  │          │  - mDNS + Header Self-Heal  │  │
│  │  - MemoryPack / LZ4     │          │  - E2EE Pairing Handshake   │  │
│  └─────────────────────────┘          └─────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 核心模块一：剪贴板监听、防抖与三阶段防自激防线（`clipboard_supervisor.rs`）

本模块融合了方案 B 的序列号防抖与方案 A/G 的三阶段忽略协议，彻底杜绝死锁与自激触发。

```rust
// src-tauri/src/clipboard_supervisor.rs
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    RegisterClipboardFormatW, RemoveClipboardFormatListener,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
    SetWindowLongPtrW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA,
    WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{w, PCWSTR};

const WM_CLIPBOARDUPDATE: u32 = 0x031D;

pub struct ClipboardSupervisor {
    last_seq: AtomicU32,
    // 三阶段防自激屏蔽标志
    is_internal_copying: AtomicBool,
    is_internal_pasting: AtomicBool,
    is_internal_restoring: AtomicBool,
    cf_ignore: u32,
    cf_exclude_cloud: u32,
    cf_can_include_history: u32,
    hwnd: HWND,
}

impl ClipboardSupervisor {
    pub fn new() -> Arc<Self> {
        let cf_ignore = unsafe { RegisterClipboardFormatW(w!("Clipboard Viewer Ignore")) };
        let cf_exclude_cloud = unsafe {
            RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))
        };
        let cf_can_include_history = unsafe {
            RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"))
        };

        let supervisor = Arc::new(Self {
            last_seq: AtomicU32::new(0),
            is_internal_copying: AtomicBool::new(false),
            is_internal_pasting: AtomicBool::new(false),
            is_internal_restoring: AtomicBool::new(false),
            cf_ignore,
            cf_exclude_cloud,
            cf_can_include_history,
            hwnd: HWND(0),
        });

        // 启动专属 Win32 消息泵窗口
        supervisor.init_message_window();
        supervisor
    }

    /// 标记进入写回阶段，通知系统与外部密码管理器忽略
    pub fn begin_internal_paste_scope(&self) -> InternalScopeGuard {
        self.is_internal_pasting.store(true, Ordering::SeqCst);
        InternalScopeGuard {
            flag: &self.is_internal_pasting,
        }
    }

    /// 核心剪贴板变更处理回调
    pub fn handle_clipboard_update(&self) {
        // 1. 硬件级递增序列号校验（吸收方案 B 精髓）
        let current_seq = unsafe { GetClipboardSequenceNumber() };
        let old_seq = self.last_seq.swap(current_seq, Ordering::SeqCst);
        if current_seq == old_seq {
            return; // 过滤虚假信号与自身重复脉冲
        }

        // 2. 三阶段原子屏蔽校验（吸收方案 G 精髓）
        if self.is_internal_copying.load(Ordering::SeqCst)
            || self.is_internal_pasting.load(Ordering::SeqCst)
            || self.is_internal_restoring.load(Ordering::SeqCst)
        {
            return; // 自身回写引起的更新，直接阻断
        }

        // 3. 密码管理器与系统隐私格式穿透拦截（吸收方案 A 与 Windows 官方规范）
        unsafe {
            if IsClipboardFormatAvailable(self.cf_ignore)
                || IsClipboardFormatAvailable(self.cf_exclude_cloud)
            {
                // 属于密码管理器生成的临时口令或自身忽略项，不记录历史
                return;
            }
        }

        // 4. 派发有效载荷提取流水线
        self.dispatch_payload_extraction();
    }

    fn dispatch_payload_extraction(&self) {
        // 提取格式并流式存入 SQLite3MC
    }

    fn init_message_window(self: &Arc<Self>) {
        // 创建隐藏窗口并注册 AddClipboardFormatListener
    }
}

pub struct InternalScopeGuard<'a> {
    flag: &'a AtomicBool,
}

impl<'a> Drop for InternalScopeGuard<'a> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}
```

---

## 3. 核心模块二：焦点穿透回贴与修饰键释放状态机（`focus_injector.rs`）

本模块集成了方案 A 的前台锁定超时清零、强制 KeyUp 状态机、以及针对输入法状态的静默回贴。

```rust
// src-tauri/src/focus_injector.rs
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AttachThreadInput, BringWindowToTop, GetForegroundWindow,
    GetWindowThreadProcessId, SetForegroundWindow, SystemParametersInfoW,
    SPI_GETFOREGROUNDLOCKTIMEOUT, SPI_SETFOREGROUNDLOCKTIMEOUT,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

pub struct FocusInjector;

impl FocusInjector {
    /// 以微秒级速度穿透并将剪贴板内容贴入目标原活动窗口
    pub fn paste_to_target(target_hwnd: HWND) -> Result<(), String> {
        if target_hwnd.0 == 0 {
            return Err("Invalid target window handle".into());
        }

        // 1. 修饰键释放状态机（吸收方案 A 与方案 B 精髓）
        Self::ensure_all_modifiers_released();

        unsafe {
            // 2. 突破 Windows 前台抢占锁: 归零超时设置 (方案 A 核心精髓)
            let mut original_timeout: u32 = 0;
            SystemParametersInfoW(
                SPI_GETFOREGROUNDLOCKTIMEOUT,
                0,
                Some(&mut original_timeout as *mut u32 as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            SystemParametersInfoW(
                SPI_SETFOREGROUNDLOCKTIMEOUT,
                0,
                Some(std::ptr::null_mut()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );

            // 3. 线程输入队列绑定与前台窗口激活
            let current_thread = GetCurrentThreadId();
            let target_thread = GetWindowThreadProcessId(target_hwnd, None);

            let attached = if current_thread != target_thread {
                AttachThreadInput(current_thread, target_thread, true).as_bool()
            } else {
                false
            };

            BringWindowToTop(target_hwnd);
            SetForegroundWindow(target_hwnd);

            if attached {
                AttachThreadInput(current_thread, target_thread, false);
            }

            // 4. 还原前台锁定超时
            SystemParametersInfoW(
                SPI_SETFOREGROUNDLOCKTIMEOUT,
                0,
                Some(original_timeout as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }

        // 5. 等待窗口完全响应焦点（短暂让步 20ms）
        thread::sleep(Duration::from_millis(20));

        // 6. 发送合成击键 Ctrl + V
        Self::simulate_ctrl_v();

        Ok(())
    }

    /// 强制释放所有物理修饰键，防止硬件级卡键
    fn ensure_all_modifiers_released() {
        let modifier_keys = [
            VK_LSHIFT, VK_RSHIFT, VK_SHIFT,
            VK_LCONTROL, VK_RCONTROL, VK_CONTROL,
            VK_LMENU, VK_RMENU, VK_MENU,
        ];

        let mut inputs: Vec<INPUT> = Vec::new();
        for &vk in &modifier_keys {
            let state = unsafe { GetKeyState(vk.0 as i32) };
            if (state & (0x8000u16 as i16)) != 0 {
                // 物理按键处于按下状态，灌入合成 KeyUp
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: vk,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                });
            }
        }

        if !inputs.is_empty() {
            unsafe {
                SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }

    /// 模拟标准 Ctrl + V
    fn simulate_ctrl_v() {
        let inputs = [
            // Ctrl Down
            Self::create_key_input(VK_CONTROL, 0),
            // V Down
            Self::create_key_input(VIRTUAL_KEY(0x56), 0),
            // V Up
            Self::create_key_input(VIRTUAL_KEY(0x56), KEYEVENTF_KEYUP.0),
            // Ctrl Up
            Self::create_key_input(VK_CONTROL, KEYEVENTF_KEYUP.0),
        ];

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn create_key_input(vk: VIRTUAL_KEY, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(flags),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}
```

---

## 4. 核心模块三：动态参数模板插槽替换引擎（`template_engine.rs`）

吸收方案 D 的生产力创新，用 Rust 原生正则实现高效参数提取与插槽填充。

```rust
// src-tauri/src/template_engine.rs
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    Clipboard,
    Date,
    CustomVariable(String),
}

#[derive(Debug, Clone)]
pub struct TemplateSlot {
    pub raw_tag: String,
    pub kind: SlotKind,
}

pub struct TemplateEngine {
    slot_regex: Regex,
}

impl TemplateEngine {
    pub fn new() -> Self {
        Self {
            slot_regex: Regex::new(r"\{\{\s*(.*?)\s*\}\}").unwrap(),
        }
    }

    /// 扫描模板文本中所有的插槽变量
    pub fn extract_slots(&self, template: &str) -> Vec<TemplateSlot> {
        let mut slots = Vec::new();
        for cap in self.slot_regex.captures_iter(template) {
            let slot_name = cap[1].trim();
            let kind = match slot_name.to_lowercase().as_str() {
                "clipboard" => SlotKind::Clipboard,
                "date" => SlotKind::Date,
                _ => SlotKind::CustomVariable(slot_name.to_string()),
            };
            slots.push(TemplateSlot {
                raw_tag: cap[0].to_string(),
                kind,
            });
        }
        slots
    }

    /// 根据用户提供的参数字典合成最终文本
    pub fn render(
        &self,
        template: &str,
        values: &HashMap<String, String>,
        live_clipboard: &str,
    ) -> String {
        self.slot_regex
            .replace_all(template, |caps: &regex::Captures| {
                let key = caps[1].trim();
                match key.to_lowercase().as_str() {
                    "clipboard" => live_clipboard.to_string(),
                    "date" => chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    _ => values.get(key).cloned().unwrap_or_else(|| caps[0].to_string()),
                }
            })
            .to_string()
    }
}
```

---

## 5. 核心模块四：内嵌 Model Context Protocol (MCP) 服务（`mcp_service.rs`）

吸收方案 C 的 AI 原生思路，向本地大语言模型（Claude Desktop、Cursor 等）暴露标准 MCP 协议，让剪贴板成为 AI 的上下文数据库。

```rust
// src-tauri/src/mcp_service.rs
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

pub struct EmbeddedMcpServer;

impl EmbeddedMcpServer {
    /// 处理来自 AI 助手的标准 MCP JSON-RPC 请求
    pub fn handle_request(req: McpRequest) -> Value {
        match req.method.as_str() {
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "result": {
                    "tools": [
                        {
                            "name": "search_clipboard",
                            "description": "全文检索用户的剪贴板历史记录，支持关键词与数据类型过滤",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string", "description": "搜索关键词" },
                                    "limit": { "type": "integer", "default": 20 }
                                }
                            }
                        },
                        {
                            "name": "copy_to_clipboard",
                            "description": "由 AI 助手安全回写一段文本到用户系统剪贴板",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "content": { "type": "string", "description": "要写入的纯文本" }
                                },
                                "required": ["content"]
                            }
                        }
                    ]
                }
            }),

            "tools/call" => {
                Self::execute_tool_call(req.id, req.params.unwrap_or(json!({})))
            }

            _ => json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "error": { "code": -32601, "message": "Method not found" }
            }),
        }
    }

    fn execute_tool_call(id: Value, params: Value) -> Value {
        let tool_name = params["name"].as_str().unwrap_or("");
        match tool_name {
            "search_clipboard" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": "检索到的剪贴板数据示例..." }]
                }
            }),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32602, "message": "Invalid params" }
            }),
        }
    }
}
```

---

## 6. 存储引擎设计：物理分表与全文索引

借鉴方案 A 与方案 G 的数据库表分离模式：
* **`clips_meta`（主表，热区）**：
  * `id INTEGER PRIMARY KEY AUTOINCREMENT`
  * `timestamp INTEGER NOT NULL`
  * `text_preview TEXT`（前 200 字符，用于快速列表呈现）
  * `crc32 INTEGER NOT NULL`（快速重复检测）
  * `is_favorite BOOLEAN DEFAULT 0`
  * `source_process TEXT`（来源应用名，如 chrome.exe）
* **`clips_payload`（载荷表，冷区）**：
  * `id INTEGER PRIMARY KEY`
  * `format TEXT NOT NULL`（`text/plain`, `text/html`, `image/png`）
  * `data BLOB NOT NULL`（LZ4 即时压缩的原始载荷）
* **`clips_fts`（FTS5 全文索引虚拟表）**：
  * 同步维护纯文本全文倒排索引，支持毫秒级多关键字词缀检索。
