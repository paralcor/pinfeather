// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import ManualPanel from "./ManualPanel";
import type { DesktopBridge, ManualPanelControl } from "./lib/desktop";
import type { CopyClipTextResult, ManualPanelSnapshot, StatusResult } from "./lib/ipc";
afterEach(cleanup);
const a = "a".repeat(32), b = "b".repeat(32);
function fixture() {
  const writes: Array<{ requestId: string; clipId: string; resolve(value: CopyClipTextResult): void }> = [];
  const bridge: DesktopBridge = {
    available: true,
    status: vi.fn(async () => ({ revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" }, capture: { user_enabled: false, admission_open: false, policy_epoch: 1 }, counters: { observed: "2", captured: "2", persisted: "2", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" }, queue: { items: 0, bytes: 0 }, history: { items: 2, payload_bytes: 200000 }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 1, worker_protocol: 1 }, last_paste: null } as StatusResult)),
    list: vi.fn(async (requestId, _cursor, query = "") => ({ request_id: requestId, normalized_query: query, summaries: ["1", "2"].map(id => ({ id, captured_at_ms: 1, kind: "text" as const, preview: `摘要${id}`, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 100000, source_process: null, source_confidence: "unknown" as const })), next_cursor: null })),
    getText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, text: "长正文仅首段", next_offset: 18 })),
    copyText: vi.fn((requestId, clipId) => new Promise<CopyClipTextResult>(resolve => writes.push({ requestId, clipId, resolve }))),
    setEnabled: vi.fn(), subscribe: vi.fn(async () => () => {}),
  };
  const hide = vi.fn();
  return { bridge, writes, hide };
}
async function panel(f = fixture()) { const mounted = render(<App bridge={f.bridge} panel={{ hide: f.hide }} />); await screen.findByText("摘要1"); return { ...f, ...mounted }; }
function pointerClick(target: HTMLElement) {
  fireEvent.pointerDown(target, { button: 0, pointerId: 1, isPrimary: true });
  fireEvent.pointerUp(target, { button: 0, pointerId: 1, isPrimary: true });
  fireEvent.click(target, { detail: 1 });
}
function browse() { pointerClick(screen.getByRole("button", { name: "浏览结果" })); return screen.getByRole("list", { name: "历史条目" }); }
function selectedRows() { return screen.getAllByRole("button", { name: "查看正文" }).map(button => button.getAttribute("aria-pressed")); }
function keyGesture(target: HTMLElement,key:string){target.focus();fireEvent.keyDown(target,{key,code:key===" "?"Space":key});fireEvent.keyUp(target,{key,code:key===" "?"Space":key});}
describe("C direct browse and full-text explicit copy",()=>{
 it("keeps search editable while the list owns independent navigation and Escape",async()=>{
  const f=await panel(),list=browse(),search=screen.getByLabelText("搜索历史正文") as HTMLInputElement;expect(search.readOnly).toBe(false);search.focus();keyGesture(search,"Escape");expect(f.hide).not.toHaveBeenCalled();
  keyGesture(list,"ArrowDown");expect(selectedRows()).toEqual(["false","true"]);keyGesture(list,"ArrowDown");expect(selectedRows()).toEqual(["false","true"]);keyGesture(list,"ArrowUp");expect(selectedRows()).toEqual(["true","false"]);keyGesture(list,"Escape");expect(f.hide).toHaveBeenCalledTimes(1);expect(f.writes).toHaveLength(0);
 });
 it.each(["copied","not_written","unknown"] as const)("copies stored ID with a partial preview and keeps %s truthful without duplicate",async outcome=>{
  const f=await panel();pointerClick(screen.getAllByRole("button",{name:"查看正文"})[1]);await screen.findByText("长正文仅首段");const list=screen.getByRole("list",{name:"历史条目"});
  keyGesture(list,"Enter");expect(f.writes).toHaveLength(1);expect(f.writes[0].clipId).toBe("2");pointerClick(screen.getByRole("button",{name:"复制中…"}));expect(f.writes).toHaveLength(1);
  await act(async()=>f.writes[0].resolve({request_id:f.writes[0].requestId,clip_id:"2",outcome,reason:outcome==="copied"?null:outcome==="not_written"?"busy":"protocol_error"}));
  await screen.findByText(outcome==="copied"?"已复制，可手动粘贴。":outcome==="not_written"?"未执行复制：当前操作繁忙，请稍后手动重试。":"无法确认复制结果，剪贴板可能已变化。请自行检查；不会自动重试。");expect(f.writes).toHaveLength(1);expect(f.hide).not.toHaveBeenCalled();
 });
 it("return cancels a delayed preview and requires new selection",async()=>{
  const f=fixture();let finish!:(value:Awaited<ReturnType<DesktopBridge["getText"]>>)=>void;f.bridge.getText=vi.fn(()=>new Promise<Awaited<ReturnType<DesktopBridge["getText"]>>>(resolve=>{finish=resolve;}));await panel(f);browse();pointerClick(screen.getByRole("button",{name:"返回搜索"}));
  await act(async()=>finish({request_id:"preview-1",clip_id:"1",text:"迟到正文",next_offset:null}));expect(screen.queryByText("迟到正文")).toBeNull();expect(selectedRows()).toEqual(["false","false"]);expect(f.writes).toHaveLength(0);
 });
 it("close-preview retains list focus and does not allow copy until new selection",async()=>{
  const f=await panel(),list=browse();await screen.findByText("长正文仅首段");pointerClick(screen.getByRole("button",{name:"关闭正文预览"}));expect(document.activeElement).toBe(list);keyGesture(list,"Enter");expect(f.writes).toHaveLength(0);keyGesture(list,"ArrowDown");pointerClick(screen.getByRole("button",{name:"复制纯文本"}));expect(f.writes).toHaveLength(1);
 });
 it("loading, empty and failed results cannot be selected or copied",async()=>{
  const f=await panel();let finish!:(value:Awaited<ReturnType<DesktopBridge["list"]>>)=>void;f.bridge.list=vi.fn(()=>new Promise<Awaited<ReturnType<DesktopBridge["list"]>>>(resolve=>{finish=resolve;}));const list=browse();pointerClick(screen.getByRole("button",{name:"刷新"}));keyGesture(list,"Enter");expect(f.writes).toHaveLength(0);
  await act(async()=>finish({request_id:"page1-2",normalized_query:"",summaries:[],next_cursor:null}));keyGesture(list,"Enter");expect(f.writes).toHaveLength(0);pointerClick(screen.getByRole("button",{name:"返回搜索"}));expect((screen.getByRole("button",{name:"浏览结果"}) as HTMLButtonElement).disabled).toBe(true);
  f.bridge.list=vi.fn(async()=>{throw new Error("synthetic");});pointerClick(screen.getByRole("button",{name:"刷新"}));await screen.findByText("无法完成操作，请重试。");expect((screen.getByRole("button",{name:"浏览结果"}) as HTMLButtonElement).disabled).toBe(true);expect(f.writes).toHaveLength(0);
 });
});
function controller() {
  let current: ManualPanelSnapshot = { revision: "1", session_id: a, phase: "waiting", hotkey: "ready", error: null };
  let listener: ((value: ManualPanelSnapshot) => void) | undefined;
  const histories: ReturnType<typeof fixture>[] = []; const unsubscribe = vi.fn();
  const control: ManualPanelControl = {
    available: true, snapshot: vi.fn(async () => current), subscribe: vi.fn(async callback => { listener = callback; return unsubscribe; }),
    open: vi.fn(async () => current), ready: vi.fn(async () => ({ ...current, phase: "ready" as const })), hide: vi.fn(async () => current),
    history: vi.fn(() => { const f = fixture(); histories.push(f); return f.bridge; }),
  };
  return { control, histories, unsubscribe, emit(next: ManualPanelSnapshot) { current = next; listener?.(next); } };
}
describe("manual panel sessions", () => {
  it.each(["reject", "waiting", "wrong-session"])("does not activate an editor on presentation %s", async outcome => {
    const f = controller();
    f.control.ready = vi.fn(async () => {
      if (outcome === "reject") throw new Error("synthetic focus failure");
      return { revision: "2", session_id: outcome === "wrong-session" ? b : a,
        phase: outcome === "waiting" ? "waiting" : "ready", hotkey: "ready", error: null } as ManualPanelSnapshot;
    });
    render(<ManualPanel control={f.control} />);
    await screen.findByRole("alert");
    expect(screen.queryByLabelText("搜索历史正文")).toBeNull();
    expect(f.histories[0].bridge.list).not.toHaveBeenCalled();
    expect(f.histories[0].writes).toHaveLength(0);
  });
  it.each(["hidden", "replaced", "unmounted"])("discards a late presentation reply after the session is %s", async action => {
    const f = controller();
    const replies = new Map<string, (value: ManualPanelSnapshot) => void>();
    f.control.ready = vi.fn((session: string) => new Promise<ManualPanelSnapshot>(resolve => { replies.set(session, resolve); }));
    const mounted = render(<ManualPanel control={f.control} />);
    await waitFor(() => expect(replies.has(a)).toBe(true));
    if (action === "unmounted") mounted.unmount();
    else act(() => f.emit({ revision: "3", session_id: action === "replaced" ? b : null,
      phase: action === "replaced" ? "waiting" : "hidden", hotkey: "ready", error: null }));
    await act(async () => replies.get(a)!({ revision: "2", session_id: a, phase: "ready", hotkey: "ready", error: null }));
    expect(screen.queryByLabelText("搜索历史正文")).toBeNull();
    expect(f.histories[0].bridge.list).not.toHaveBeenCalled();
    if (action === "replaced") {
      await act(async () => replies.get(b)!({ revision: "4", session_id: b, phase: "ready", hotkey: "ready", error: null }));
      expect(document.activeElement).toBe(await screen.findByLabelText("搜索历史正文"));
    }
  });
  it("does not refocus the editor on duplicate ready notifications", async () => {
    const f = controller(); render(<ManualPanel control={f.control} />); await screen.findByText("摘要1");
    const list = browse(); expect(document.activeElement).toBe(list);
    act(() => f.emit({ revision: "3", session_id: a, phase: "ready", hotkey: "ready", error: null }));
    expect(document.activeElement).toBe(list); expect(f.control.ready).toHaveBeenCalledTimes(1);
  });
  it("waits for native presentation acknowledgement before mounting or focusing the editor", async () => {
    const f = controller();
    let ready!: (value: ManualPanelSnapshot) => void;
    f.control.ready = vi.fn(() => new Promise<ManualPanelSnapshot>(resolve => { ready = resolve; }));
    render(<ManualPanel control={f.control} />);
    await waitFor(() => expect(f.control.ready).toHaveBeenCalledWith(a));
    expect(screen.queryByLabelText("搜索历史正文")).toBeNull();
    await act(async () => ready({ revision: "2", session_id: a, phase: "ready", hotkey: "ready", error: null }));
    const search = await screen.findByLabelText("搜索历史正文");
    expect(document.activeElement).toBe(search);
    expect(f.histories[0].writes).toHaveLength(0);
  });
  it.each([false, true])("replaces IME acknowledgement (end observed: %s), rejects old controls and late query results", async ended => {
    const f = controller(); render(<ManualPanel control={f.control} />); await screen.findByText("摘要1");
    const oldSearch = screen.getByLabelText("搜索历史正文");
    const history = f.histories[0];
    let finish!: (value: Awaited<ReturnType<DesktopBridge["list"]>>) => void;
    const originalList = history.bridge.list;
    history.bridge.list = vi.fn(() => new Promise<Awaited<ReturnType<DesktopBridge["list"]>>>(resolve => { finish = resolve; }));
    fireEvent.compositionStart(oldSearch); fireEvent.change(oldSearch, { target: { value: "旧输入" } });
    await waitFor(() => expect(history.bridge.list).toHaveBeenCalledTimes(1));
    if (ended) fireEvent.compositionEnd(oldSearch);
    const oldCopy = screen.getByRole("button", { name: "复制纯文本" });
    const oldBrowse = screen.getByRole("button", { name: "浏览结果" });
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null })); await screen.findByText("摘要1");
    await act(async () => finish(await originalList("old-query", null, "旧输入")));
    fireEvent.compositionEnd(oldSearch); pointerClick(oldBrowse); fireEvent.keyUp(oldCopy, { key: "Enter", code: "Enter" });
    const search = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
    expect(search.value).toBe(""); expect(search.readOnly).toBe(false); expect(document.activeElement).toBe(search);
    expect(screen.queryByRole("button", { name: "完成输入" })).toBeNull(); expect(selectedRows()).toEqual(["false", "false"]);
    expect(f.histories.every(h => h.writes.length === 0 && vi.mocked(h.bridge.getText).mock.calls.length === 0)).toBe(true);
    expect(f.control.hide).not.toHaveBeenCalled(); browse(); expect(selectedRows()).toEqual(["true", "false"]);
  });
  it("resets an active browse mode and list focus for every new accepted session", async () => {
    const f = controller(); render(<ManualPanel control={f.control} />); await screen.findByText("摘要1");
    const list = browse(); fireEvent.keyDown(list, { key: "ArrowDown" }); expect(selectedRows()).toEqual(["false", "true"]);
    expect((screen.getByLabelText("搜索历史正文") as HTMLInputElement).readOnly).toBe(false);
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null })); await screen.findByText("摘要1");
    const search = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
    expect(search.value).toBe(""); expect(search.readOnly).toBe(false); expect(document.activeElement).toBe(search);
    expect(selectedRows()).toEqual(["false", "false"]); expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
    fireEvent.keyDown(search, { key: "Enter" }); fireEvent.keyDown(search, { key: "Escape" });
    expect(f.histories.every(history => history.writes.length === 0)).toBe(true); expect(f.control.hide).not.toHaveBeenCalled();
  });
  it("shows presentation failures in main even while the hotkey remains registered", async () => { const f = controller(); f.control.snapshot = vi.fn(async () => ({ revision: "3", session_id: null, phase: "hidden", hotkey: "ready", error: "快捷面板聚焦失败。" } as ManualPanelSnapshot)); const h = fixture(); render(<App bridge={h.bridge} panelControl={f.control} />); expect(await screen.findByText("快捷面板聚焦失败。")).toBeTruthy(); });
  it("resets query/selection and discards old replies when the host opens a new session", async () => {
    const f = controller(); render(<ManualPanel control={f.control} />); await screen.findByText("摘要1");
    const search = screen.getByLabelText("搜索历史正文"); const list = browse(); keyGesture(list, "Enter");
    pointerClick(screen.getByRole("button", { name: "返回搜索" }));
    fireEvent.change(search, { target: { value: "旧查询" } });
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null })); await screen.findByText("摘要1");
    expect((screen.getByLabelText("搜索历史正文") as HTMLInputElement).value).toBe(""); expect(screen.getAllByRole("button", { name: "查看正文" }).every(button => button.getAttribute("aria-pressed") === "false")).toBe(true);
    expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull(); expect(document.activeElement).toBe(screen.getByLabelText("搜索历史正文"));
    const old = f.histories[0].writes[0]; await act(async () => old.resolve({ request_id: old.requestId, clip_id: "1", outcome: "copied", reason: null })); expect(screen.queryByText("已复制，可手动粘贴。")).toBeNull();
    expect(f.control.ready).toHaveBeenCalledWith(a); expect(f.control.ready).toHaveBeenCalledWith(b);
    act(() => f.emit({ revision: "1", session_id: a, phase: "ready", hotkey: "ready", error: null }));
    keyGesture(screen.getByLabelText("搜索历史正文"), "Escape"); expect(f.control.hide).not.toHaveBeenCalled();
    keyGesture(browse(), "Escape"); expect(f.control.hide).toHaveBeenCalledWith(b);
  });
  it("unmounts the session on host hide and cleans its event subscription", async () => { const f = controller(); const mounted = render(<ManualPanel control={f.control} />); await screen.findByText("摘要1"); act(() => f.emit({ revision: "2", session_id: null, phase: "hidden", hotkey: "ready", error: null })); expect(screen.queryByLabelText("搜索历史正文")).toBeNull(); mounted.unmount(); expect(f.unsubscribe).toHaveBeenCalledTimes(1); });
  it("keeps the main entry available when hotkey registration fails", async () => { const f = controller(); f.control.snapshot = vi.fn(async () => ({ revision: "1", session_id: null, phase: "hidden", hotkey: "failed", error: "Ctrl+Alt+V 注册失败，可使用主窗口入口。" } as ManualPanelSnapshot)); const h = fixture(); render(<App bridge={h.bridge} panelControl={f.control} />); await screen.findByText("Ctrl+Alt+V 注册失败，可使用主窗口入口。"); const button = screen.getByRole("button", { name: "打开快捷面板" }) as HTMLButtonElement; expect(button.disabled).toBe(false); fireEvent.click(button); await waitFor(() => expect(f.control.open).toHaveBeenCalledTimes(1)); expect(h.writes).toHaveLength(0); });
});
