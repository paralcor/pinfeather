// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import ManualPanel from "./ManualPanel";
import type { DesktopBridge, ManualPanelControl } from "./lib/desktop";
import type { ClipSummary, CopyClipTextResult, CopyMergedTextResult, ManualPanelSnapshot, StatusResult } from "./lib/ipc";
afterEach(cleanup);
const a = "a".repeat(32), b = "b".repeat(32);
function fixture() {
  const writes: Array<{ requestId: string; clipId: string; resolve(value: CopyClipTextResult): void }> = [];
  const bridge: DesktopBridge = {
    available: true,
    status: vi.fn(async () => ({ revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" }, capture: { user_enabled: false, admission_open: false, policy_epoch: 1 }, counters: { observed: "2", captured: "2", persisted: "2", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" }, queue: { items: 0, bytes: 0 }, history: { items: 2, payload_bytes: 200000, favorites: 0, revision: "0" }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 2, worker_protocol: 1 }, last_paste: null } as StatusResult)),
    list: vi.fn(async (requestId, _cursor, query = "") => ({ request_id: requestId, normalized_query: query, summaries: ["1", "2"].map(id => ({ id, captured_at_ms: 1, kind: "text" as const, preview: `摘要${id}`, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 100000, source_process: null, source_confidence: "unknown" as const, favorited: false })), next_cursor: null })),
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
const arrowDown = { key: "ArrowDown", code: "ArrowDown" };

describe("search ArrowDown", () => {
  it.each(["Enter", " "])("lets the focused row preview button handle %j", async key => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    const list = screen.getByRole("list", { name: "历史条目" });
    fireEvent.keyDown(search, arrowDown);
    fireEvent.keyUp(list, arrowDown);
    expect(selectedRows()).toEqual(["true", "false"]);

    const second = screen.getAllByRole("button", { name: "查看正文" })[1];
    second.focus();
    const event = { key, code: key === " " ? "Space" : "Enter" };
    const down = fireEvent.keyDown(second, event);
    const up = fireEvent.keyUp(second, event);
    // jsdom leaves native button activation to the test.
    if (down && up) fireEvent.click(second, { detail: 0 });
    expect(f.writes).toHaveLength(0);
    expect(f.hide).not.toHaveBeenCalled();
    expect(selectedRows()).toEqual(["false", "true"]);
    expect(f.bridge.getText).toHaveBeenLastCalledWith(expect.any(String), "2", 0);
    await screen.findByText("长正文仅首段");
  });

  it("recovers list navigation, Enter and Escape after releasing Shift before the letter", async () => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    const list = screen.getByRole("list", { name: "历史条目" });
    fireEvent.keyDown(search, { key: "Shift", code: "ShiftLeft", shiftKey: true });
    fireEvent.keyDown(search, { key: "A", code: "KeyA", shiftKey: true });
    fireEvent.change(search, { target: { value: "A" } });
    fireEvent.keyUp(search, { key: "Shift", code: "ShiftLeft" });
    fireEvent.keyUp(search, { key: "a", code: "KeyA" });
    await screen.findByText(/结果对应搜索词：A。按 ↓/);
    fireEvent.keyDown(search, arrowDown);
    fireEvent.keyUp(list, arrowDown);
    expect(document.activeElement).toBe(list);
    await screen.findByText("长正文仅首段");

    keyGesture(list, "Enter");
    expect(f.writes).toHaveLength(1);
    const { requestId, clipId, resolve } = f.writes[0];
    await act(async () => resolve({ request_id: requestId, clip_id: clipId, outcome: "not_written", reason: "storage_unavailable" }));
    expect(f.hide).not.toHaveBeenCalled();
    keyGesture(list, "Escape");
    expect(f.hide).toHaveBeenCalledTimes(1);
  });

  it("lets a fresh ArrowDown browse after a late IME Enter is released", async () => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    const list = screen.getByRole("list", { name: "历史条目" });
    fireEvent.compositionStart(search);
    fireEvent.compositionEnd(search);
    fireEvent.keyDown(search, { key: "Process", code: "Enter", keyCode: 229, isComposing: true });
    fireEvent.keyUp(search, { key: "Enter", code: "Enter" });
    expect(selectedRows()).toEqual(["false", "false"]);
    expect(f.writes).toHaveLength(0);
    expect(f.hide).not.toHaveBeenCalled();

    fireEvent.keyDown(search, arrowDown);
    fireEvent.keyUp(list, arrowDown);
    expect(document.activeElement).toBe(list);
    expect(selectedRows()).toEqual(["true", "false"]);
    await screen.findByText("长正文仅首段");
  });

  it("selects the first ID, moves focus, and waits for a fresh ArrowDown before advancing", async () => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    const list = screen.getByRole("list", { name: "历史条目" });
    expect(document.activeElement).toBe(search);
    expect(fireEvent.keyDown(search, arrowDown)).toBe(false);
    expect(document.activeElement).toBe(list);
    expect(selectedRows()).toEqual(["true", "false"]);
    expect(f.bridge.getText).toHaveBeenLastCalledWith(expect.any(String), "1", 0);
    fireEvent.keyDown(list, { ...arrowDown, repeat: true });
    fireEvent.keyUp(list, arrowDown);
    expect(selectedRows()).toEqual(["true", "false"]);

    keyGesture(list, "ArrowDown");
    expect(selectedRows()).toEqual(["false", "true"]);
    expect(f.bridge.getText).toHaveBeenLastCalledWith(expect.any(String), "2", 0);
    await screen.findByText("长正文仅首段");
    search.focus();
    expect(fireEvent.keyDown(search, arrowDown)).toBe(false);
    fireEvent.keyUp(list, arrowDown);
    expect(document.activeElement).toBe(list);
    expect(selectedRows()).toEqual(["true", "false"]);
    expect(f.bridge.getText).toHaveBeenLastCalledWith(expect.any(String), "1", 0);
    await screen.findByText("长正文仅首段");
    expect(f.writes).toHaveLength(0);
    expect(f.hide).not.toHaveBeenCalled();
    pointerClick(screen.getByRole("button", { name: "返回搜索" }));
    expect(document.activeElement).toBe(search);
    expect(screen.getByRole("button", { name: "浏览结果" })).toBeTruthy();
    expect(screen.getByText(/按 ↓ 浏览结果/)).toBeTruthy();
  });

  it.each([false, true])("rejects Enter started while ArrowDown is held (release ArrowDown first: %s)", async releaseArrowFirst => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    const list = screen.getByRole("list", { name: "历史条目" });
    fireEvent.keyDown(search, arrowDown);
    expect(document.activeElement).toBe(list);
    await screen.findByText("长正文仅首段");
    const enter = { key: "Enter", code: "Enter" };
    fireEvent.keyDown(list, enter);
    if (releaseArrowFirst) fireEvent.keyUp(list, arrowDown);
    fireEvent.keyUp(list, enter);
    if (!releaseArrowFirst) fireEvent.keyUp(list, arrowDown);
    expect(f.writes).toHaveLength(0);
    expect(f.hide).not.toHaveBeenCalled();

    keyGesture(list, "Enter");
    expect(f.writes).toHaveLength(1);
    expect(f.writes[0].clipId).toBe("1");
    expect(f.hide).not.toHaveBeenCalled();
  });

  it.each([{ shiftKey: true }, { ctrlKey: true }, { altKey: true }, { metaKey: true },
    { repeat: true }, { isComposing: true }, { key: "Process" }, { keyCode: 229 }])(
    "leaves search ArrowDown with %j to the browser or IME", async flags => {
      const f = await panel();
      const search = screen.getByLabelText("搜索历史正文");
      expect(fireEvent.keyDown(search, { ...arrowDown, ...flags })).toBe(true);
      fireEvent.keyUp(search, arrowDown);
      expect(document.activeElement).toBe(search);
      expect(selectedRows()).toEqual(["false", "false"]);
      expect(f.bridge.getText).not.toHaveBeenCalled();
      expect(f.writes).toHaveLength(0);
      expect(f.hide).not.toHaveBeenCalled();
    },
  );

  it("waits for composition to end and the candidate's ArrowDown to be released", async () => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    fireEvent.compositionStart(search);
    expect(fireEvent.keyDown(search, arrowDown)).toBe(true);
    fireEvent.compositionEnd(search);
    expect(fireEvent.keyDown(search, { ...arrowDown, repeat: true })).toBe(true);
    fireEvent.keyUp(search, arrowDown);
    expect(document.activeElement).toBe(search);
    expect(f.bridge.getText).not.toHaveBeenCalled();

    expect(fireEvent.keyDown(search, arrowDown)).toBe(false);
    const list = screen.getByRole("list", { name: "历史条目" });
    fireEvent.keyUp(list, arrowDown);
    expect(document.activeElement).toBe(list);
    expect(selectedRows()).toEqual(["true", "false"]);
    await screen.findByText("长正文仅首段");
    expect(f.writes).toHaveLength(0);
    expect(f.hide).not.toHaveBeenCalled();
  });

  it("keeps search focus during debounce and loading without replaying ArrowDown", async () => {
    const f = await panel();
    const search = screen.getByLabelText("搜索历史正文");
    const list = screen.getByRole("list", { name: "历史条目" });
    const originalList = f.bridge.list;
    type Page = Awaited<ReturnType<DesktopBridge["list"]>>;
    let finish: (() => void) | undefined;
    f.bridge.list = vi.fn(async (requestId, cursor, query) => {
      const page = await originalList(requestId, cursor, query);
      return new Promise<Page>(resolve => {
        finish = () => resolve({ ...page, summaries: [{ ...page.summaries[0], id: "3", preview: "新查询摘要" }] });
      });
    });
    fireEvent.change(search, { target: { value: "新查询" } });
    expect(screen.getByText("正在搜索，结果更新后可查看。")).toBeTruthy();
    expect(fireEvent.keyDown(search, arrowDown)).toBe(true);
    fireEvent.keyUp(search, arrowDown);
    await waitFor(() => expect(finish).toBeDefined());
    expect(fireEvent.keyDown(search, arrowDown)).toBe(true);
    fireEvent.keyUp(search, arrowDown);
    expect(document.activeElement).toBe(search);
    expect(f.bridge.getText).not.toHaveBeenCalled();
    await act(async () => finish!());
    await screen.findByText("新查询摘要");
    expect(document.activeElement).toBe(search);
    expect(selectedRows()).toEqual(["false"]);
    expect(f.bridge.getText).not.toHaveBeenCalled();

    expect(fireEvent.keyDown(search, arrowDown)).toBe(false);
    fireEvent.keyUp(list, arrowDown);
    expect(document.activeElement).toBe(list);
    expect(selectedRows()).toEqual(["true"]);
    expect(f.bridge.getText).toHaveBeenLastCalledWith(expect.any(String), "3", 0);
    await screen.findByText("长正文仅首段");
    expect(f.writes).toHaveLength(0);
    expect(f.hide).not.toHaveBeenCalled();
  });
});

describe("C direct browse and full-text explicit copy",()=>{
 it("keeps search editable while the list owns independent navigation and Escape",async()=>{
  const f=await panel(),list=browse(),search=screen.getByLabelText("搜索历史正文") as HTMLInputElement;expect(search.readOnly).toBe(false);search.focus();keyGesture(search,"Escape");expect(f.hide).not.toHaveBeenCalled();
  keyGesture(list,"ArrowDown");expect(selectedRows()).toEqual(["false","true"]);keyGesture(list,"ArrowDown");expect(selectedRows()).toEqual(["false","true"]);keyGesture(list,"ArrowUp");expect(selectedRows()).toEqual(["true","false"]);keyGesture(list,"Escape");expect(f.hide).toHaveBeenCalledTimes(1);expect(f.writes).toHaveLength(0);
 });
 it.each(["copied","not_written","unknown"] as const)("copies stored ID with a partial preview and keeps %s truthful without duplicate",async outcome=>{
  const f=await panel();pointerClick(screen.getAllByRole("button",{name:"查看正文"})[1]);await screen.findByText("长正文仅首段");const list=screen.getByRole("list",{name:"历史条目"});
  keyGesture(list,"Enter");expect(f.writes).toHaveLength(1);expect(f.writes[0].clipId).toBe("2");pointerClick(screen.getByRole("button",{name:"复制中…"}));expect(f.writes).toHaveLength(1);
  await act(async()=>f.writes[0].resolve({request_id:f.writes[0].requestId,clip_id:"2",outcome,reason:outcome==="copied"?null:outcome==="not_written"?"busy":"protocol_error"}));
  await screen.findByText(outcome==="copied"?"已复制，可手动粘贴。":outcome==="not_written"?"未执行复制：当前操作繁忙，请稍后手动重试。":"无法确认复制结果，剪贴板可能已变化。请自行检查；不会自动重试。");
  expect(f.writes).toHaveLength(1);
  if (outcome === "copied") {
    expect(f.hide).toHaveBeenCalledExactlyOnceWith(true);
  } else {
    expect(f.hide).not.toHaveBeenCalled();
  }
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
  const search = screen.getByLabelText("搜索历史正文");
  expect(fireEvent.keyDown(search, arrowDown)).toBe(true);
  fireEvent.keyUp(search, arrowDown);
  f.bridge.list=vi.fn(async()=>{throw new Error("synthetic");});pointerClick(screen.getByRole("button",{name:"刷新"}));await screen.findByText("无法完成操作，请重试。");expect((screen.getByRole("button",{name:"浏览结果"}) as HTMLButtonElement).disabled).toBe(true);expect(f.writes).toHaveLength(0);
  expect(fireEvent.keyDown(search, arrowDown)).toBe(true);
  fireEvent.keyUp(search, arrowDown);
  expect(document.activeElement).toBe(search);
  expect(f.hide).not.toHaveBeenCalled();
 });
});

describe("list Enter dismissal", () => {
  it("writes once on release and blocks further commands while dismissal is pending", async () => {
    const f = fixture();
    let finishHide!: () => void;
    f.hide.mockImplementation(() => new Promise<void>(resolve => { finishHide = resolve; }));
    await panel(f);
    const list = browse();
    const enter = { key: "Enter", code: "Enter" };
    fireEvent.keyDown(list, enter);
    expect(f.writes).toHaveLength(0);
    fireEvent.keyUp(list, enter);
    fireEvent.keyUp(list, enter);
    expect(f.writes).toHaveLength(1);
    expect(f.hide).not.toHaveBeenCalled();
    const write = f.writes[0];
    await act(async () => write.resolve({ request_id: write.requestId, clip_id: write.clipId, outcome: "copied", reason: null }));
    expect(f.hide).toHaveBeenCalledExactlyOnceWith(true);
    keyGesture(list, "Enter");
    keyGesture(list, "Escape");
    expect(f.writes).toHaveLength(1);
    expect(f.hide).toHaveBeenCalledTimes(1);
    await act(async () => finishHide());
  });

  it.each(["mismatched receipt", "transport failure"])("leaves the preview open on %s", async failure => {
    const f = fixture();
    if (failure === "transport failure") vi.mocked(f.bridge.copyText!).mockRejectedValueOnce(new Error("transport closed"));
    await panel(f);
    const list = browse();
    keyGesture(list, "Enter");
    if (failure === "mismatched receipt") {
      const write = f.writes[0];
      await act(async () => write.resolve({ request_id: write.requestId, clip_id: "2", outcome: "copied", reason: null }));
    }
    expect(await screen.findByText(/无法确认复制结果/)).toBeTruthy();
    expect(f.bridge.copyText).toHaveBeenCalledTimes(1);
    expect(f.hide).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(list);
  });

  it.each(["pointer", "keyboard"])("keeps the copy button open after a successful %s copy", async method => {
    const f = await panel();
    browse();
    const button = screen.getByRole("button", { name: "复制纯文本" });
    if (method === "pointer") pointerClick(button);
    else keyGesture(button, "Enter");
    expect(f.writes).toHaveLength(1);
    const write = f.writes[0];
    await act(async () => write.resolve({ request_id: write.requestId, clip_id: write.clipId, outcome: "copied", reason: null }));
    expect(screen.getByText("已复制，可手动粘贴。")).toBeTruthy();
    expect(f.hide).not.toHaveBeenCalled();
  });

  it.each(["search", "selection", "focus-round-trip", "window-blur", "Escape"])(
    "never auto-dismisses after %s interrupts the copy",
    async interruption => {
      const f = await panel();
      const list = browse();
      keyGesture(list, "Enter");
      expect(f.writes).toHaveLength(1);
      if (interruption === "search") pointerClick(screen.getByRole("button", { name: "返回搜索" }));
      if (interruption === "selection") keyGesture(list, "ArrowDown");
      if (interruption === "focus-round-trip") {
        screen.getByRole("button", { name: "返回搜索" }).focus();
        list.focus();
      }
      if (interruption === "window-blur") fireEvent.blur(window);
      if (interruption === "Escape") keyGesture(list, "Escape");
      const write = f.writes[0];
      await act(async () => write.resolve({ request_id: write.requestId, clip_id: write.clipId, outcome: "copied", reason: null }));
      expect(f.writes).toHaveLength(1);
      expect(f.hide.mock.calls).toEqual(interruption === "Escape" ? [[]] : []);
    },
  );

  it("does not dismiss after the host starts stopping, even before the panel session is removed", async () => {
    const f = fixture();
    const status = await f.bridge.status();
    let statusChanged!: () => void;
    f.bridge.subscribe = vi.fn(async (name, receive) => {
      if (name === "status_changed") statusChanged = receive;
      return () => {};
    });
    await panel(f);
    keyGesture(browse(), "Enter");
    expect(f.writes).toHaveLength(1);
    f.bridge.status = vi.fn(async (): Promise<StatusResult> => ({ ...status, revision: "2", state: "stopping" }));
    act(() => statusChanged());
    await screen.findByText("正在退出");
    const write = f.writes[0];
    await act(async () => write.resolve({ request_id: write.requestId, clip_id: write.clipId, outcome: "copied", reason: null }));
    expect(f.hide).not.toHaveBeenCalled();
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
  it("recovers from an initial snapshot failure when a new session arrives", async () => {
    const f = controller(); vi.mocked(f.control.snapshot).mockRejectedValueOnce(new Error("read failed"));
    render(<ManualPanel control={f.control} />); await screen.findByText("无法读取快捷面板状态，请重试打开。");
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null }));
    await screen.findByText("摘要1"); expect(screen.queryByText("无法读取快捷面板状态，请重试打开。")).toBeNull(); expect(f.control.ready).toHaveBeenCalledWith(b);
  });
  it("keeps an open failure visible in main when a snapshot arrives", async () => {
    const f = controller(); vi.mocked(f.control.open).mockRejectedValueOnce(new Error("open failed"));
    const h = fixture(); render(<App bridge={h.bridge} panelControl={f.control} />); await screen.findByText("摘要1");
    fireEvent.click(screen.getByRole("button", { name: "打开快捷面板" })); await screen.findByText("快捷面板打开失败，主窗口仍可使用。");
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null }));
    expect(screen.queryByText("快捷面板打开失败，主窗口仍可使用。")).toBeTruthy();
  });
  it.each(["notification before rejection", "rejection before notification"])("shows failed dismissal after session removal (%s)", async order => {
    const f = controller();
    let rejectHide!: (error: Error) => void;
    vi.mocked(f.control.hide).mockImplementation(() => new Promise((_resolve, reject) => { rejectHide = reject; }));
    render(<ManualPanel control={f.control} />);
    await screen.findByText("摘要1");
    keyGesture(browse(), "Enter");
    const history = f.histories[0];
    expect(history.writes).toHaveLength(1);
    const write = history.writes[0];
    await act(async () => write.resolve({ request_id: write.requestId, clip_id: write.clipId, outcome: "copied", reason: null }));
    expect(f.control.hide).toHaveBeenCalledExactlyOnceWith(a, true);
    const error = "已复制，面板隐藏失败。可手动切回粘贴，或重新打开快捷面板。";
    const failed = { revision: "2", session_id: null, phase: "hidden", hotkey: "ready", error } as const;
    if (order === "rejection before notification") await act(async () => rejectHide(new Error("hide failed")));
    act(() => f.emit(failed));
    expect(screen.getByRole("alert").textContent).toBe(error);
    expect(screen.queryByText("快捷面板已隐藏。")).toBeNull();
    expect(screen.queryByLabelText("搜索历史正文")).toBeNull();
    act(() => f.emit({ ...failed, revision: "3", session_id: b, phase: "waiting", error: null }));
    await screen.findByText("摘要1");
    if (order === "notification before rejection") await act(async () => rejectHide(new Error("hide failed")));
    act(() => f.emit(failed));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(history.writes).toHaveLength(1);
    expect(f.histories[1].writes).toHaveLength(0);
  });

  it("shows the ordinary native Esc hide error after its session is removed", async () => {
    const f = controller();
    render(<ManualPanel control={f.control} />);
    await screen.findByText("摘要1");
    keyGesture(browse(), "Escape");
    expect(f.control.hide).toHaveBeenCalledExactlyOnceWith(a);
    act(() => f.emit({ revision: "2", session_id: null, phase: "hidden", hotkey: "ready", error: "快捷面板隐藏失败。" }));
    expect(screen.getByRole("alert").textContent).toBe("快捷面板隐藏失败。");
    expect(screen.queryByText("快捷面板已隐藏。")).toBeNull();
    expect(f.histories[0].writes).toHaveLength(0);
  });

  it.each(["waiting", "stopping"] as const)("ignores a copy that resolves after the session changes to %s", async phase => {
    const f = controller();
    render(<ManualPanel control={f.control} />);
    await screen.findByText("摘要1");
    keyGesture(browse(), "Enter");
    const history = f.histories[0];
    expect(history.writes).toHaveLength(1);
    act(() => f.emit({ revision: "2", session_id: phase === "waiting" ? b : null, phase, hotkey: "ready", error: null }));
    const write = history.writes[0];
    await act(async () => write.resolve({ request_id: write.requestId, clip_id: write.clipId, outcome: "copied", reason: null }));
    expect(f.control.hide).not.toHaveBeenCalled();
  });
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

it("guards favorite gestures against IME and cross-window intent changes while retaining the preview", async () => {
  const f = fixture();
  let onFavorite!: (change: import("./lib/ipc").FavoriteChange) => void;
  f.bridge.subscribeFavorite = async callback => { onFavorite = callback; return () => {}; };
  const write = vi.fn(async (_id: string, clip_id: string, favorited: boolean) => ({
    clip_id, favorited, changed: true, revision: "2",
  }));
  f.bridge.setFavorite = write;
  await panel(f);
  const search = screen.getByLabelText("搜索历史正文");
  const list = screen.getByRole("list", { name: "历史条目" });
  fireEvent.compositionStart(search);
  fireEvent.compositionEnd(search);
  fireEvent.keyDown(search, { key: "Process", code: "Enter", keyCode: 229 });
  fireEvent.keyUp(search, { key: "Enter", code: "Enter" });
  expect(write).not.toHaveBeenCalled();

  fireEvent.keyDown(search, arrowDown);
  fireEvent.keyUp(list, arrowDown);
  await screen.findByText("长正文仅首段");
  const favorite = screen.getByRole("button", { name: "收藏" });
  fireEvent.click(favorite, { detail: 0 });
  expect(write).not.toHaveBeenCalled();
  favorite.focus();
  fireEvent.keyDown(favorite, { key: "Enter", code: "Enter" });
  await act(async () => onFavorite({ clip_id: "1", favorited: true, revision: "1" }));
  const unfavorite = screen.getByRole("button", { name: "取消收藏" });
  fireEvent.keyUp(unfavorite, { key: "Enter", code: "Enter" });
  expect(write).not.toHaveBeenCalled();
  expect(screen.getByText("★ 已收藏")).toBeTruthy();
  expect(screen.getByText("长正文仅首段")).toBeTruthy();

  pointerClick(unfavorite);
  expect(write).toHaveBeenCalledWith(expect.any(String), "1", false);
  await screen.findByRole("button", { name: "收藏" });
  expect(screen.getByText("长正文仅首段")).toBeTruthy();
  expect(f.hide).not.toHaveBeenCalled();

  keyGesture(list, "Enter");
  expect(f.writes).toHaveLength(1);
  const { requestId, clipId, resolve } = f.writes[0];
  await act(async () => resolve({ request_id: requestId, clip_id: clipId, outcome: "copied", reason: null }));
  expect(f.hide).toHaveBeenCalledExactlyOnceWith(true);
});

describe("ordered merge in the quick panel", () => {
  function mergeFixture() {
    const f = fixture();
    const rows = ["1", "2", "3", "4"].map((id): ClipSummary => ({
      id, captured_at_ms: 1, kind: id === "3" ? "png" : "text", preview: id === "2" ? "" : "摘要" + id,
      format_names: id === "3" ? ["PNG"] : id === "4" ? ["HTML Format"] : ["CF_UNICODETEXT"],
      omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown", favorited: id === "2",
    }));
    vi.mocked(f.bridge.list).mockImplementation(async (request_id, _, query = "", favorites = false) => ({
      request_id, normalized_query: query, next_cursor: null,
      summaries: rows.filter(row => (!favorites || row.favorited) && (!query || row.id === query)),
    }));
    const copy = vi.fn(async (request_id: string, clip_ids: string[]): Promise<CopyMergedTextResult> => ({
      request_id, clip_ids, outcome: "copied", reason: null, missing_clip_id: null,
    }));
    f.bridge.copyMergedText = copy;
    return { ...f, copy };
  }

  const button = (name: string) => screen.getByRole<HTMLButtonElement>("button", { name });
  const mergeRegion = () => within(screen.getByRole("region", { name: "已选合并" }));
  const search = () => screen.getByLabelText("搜索历史正文");
  const row = (id: string) => within(document.querySelector<HTMLElement>('[data-history-id="' + id + '"]')!);
  const query = (value: string) => fireEvent.change(search(), { target: { value } });

  it("toggles merge with Enter and Space and hides on Escape from the focused row button", async () => {
    const f = mergeFixture();
    await panel(f);
    browse();
    const toggle = row("1").getByRole("button", { name: "加入合并" });
    keyGesture(toggle, "Enter");
    expect(row("1").getByRole("button", { name: /已选 #1/ })).toBe(toggle);
    keyGesture(toggle, " ");
    expect(row("1").getByRole("button", { name: "加入合并" })).toBe(toggle);
    expect(f.hide).not.toHaveBeenCalled();
    keyGesture(toggle, "Escape");
    expect(f.hide.mock.calls).toEqual([[]]);
    expect(f.writes).toHaveLength(0);
  });

  it("identifies the deleted middle entry when all selected previews match", async () => {
    const f = mergeFixture();
    const preview = "相".repeat(200);
    const summaries: ClipSummary[] = ["41", "73", "92"].map(id => ({
      id, captured_at_ms: 1, kind: "text", preview,
      format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 400,
      source_process: null, source_confidence: "unknown", favorited: false,
    }));
    vi.mocked(f.bridge.list).mockImplementation(async request_id => ({
      request_id, normalized_query: "", summaries: [...summaries], next_cursor: null,
    }));
    f.copy.mockImplementationOnce(async (request_id, clip_ids) => ({
      request_id, clip_ids, outcome: "not_written", reason: "not_found",
      missing_clip_id: clip_ids.find(id => !summaries.some(item => item.id === id)) ?? null,
    }));
    render(<App bridge={f.bridge} panel={{ hide: f.hide }} />);
    await screen.findAllByText(preview);
    for (const id of ["41", "73", "92"]) pointerClick(row(id).getByRole("button", { name: "加入合并" }));
    summaries.splice(1, 1);
    pointerClick(button("合并复制（3 条）"));
    await screen.findByText("未执行合并复制：条目 #73 已不存在，请从已选列表中移出后重试。");

    const items = mergeRegion().getAllByRole("listitem");
    expect(within(items[0]).getByText("#41")).toBeTruthy();
    expect(within(items[1]).getByText("#73")).toBeTruthy();
    expect(within(items[2]).getByText("#92")).toBeTruthy();
    expect(mergeRegion().getAllByText("已不存在")).toEqual([within(items[1]).getByText("已不存在")]);
    pointerClick(within(items[1]).getByRole("button", { name: "移出第 2 条" }));
    expect(mergeRegion().queryByText("已不存在")).toBeNull();
    expect(mergeRegion().getAllByRole("listitem")).toHaveLength(2);
  });

  it("keeps list Space separate from preview and copies across search and favorites with zero results", async () => {
    const f = mergeFixture();
    await panel(f);
    expect(row("3").getByRole<HTMLButtonElement>("button", { name: "仅支持文本" }).disabled).toBe(true);
    expect(row("4").getByRole<HTMLButtonElement>("button", { name: "仅支持文本" }).disabled).toBe(true);
    const preview = row("1").getByRole("button", { name: "查看正文" });
    pointerClick(preview);
    const list = screen.getByRole("list", { name: "历史条目" });
    keyGesture(list, " ");
    expect(row("1").getByRole("button", { name: /已选 #1/ })).toBeTruthy();
    expect(f.writes).toHaveLength(0);
    expect(button("合并复制（1 条）").disabled).toBe(true);
    query("2");
    fireEvent.click(button("只看收藏"));
    await waitFor(() => expect(row("2").getByRole("button", { name: "加入合并" })).toBeTruthy());
    pointerClick(row("2").getByRole("button", { name: "加入合并" }));
    expect(mergeRegion().getAllByRole("listitem").map(item => item.textContent)).toEqual([
      expect.stringContaining("摘要1"), expect.stringContaining("（空文本）"),
    ]);
    pointerClick(mergeRegion().getByRole("button", { name: "上移第 2 条" }));
    expect(mergeRegion().getAllByRole("listitem")[0].textContent).toContain("（空文本）");
    pointerClick(mergeRegion().getByRole("button", { name: "下移第 1 条" }));
    expect(mergeRegion().getAllByRole("listitem")[0].textContent).toContain("摘要1");
    pointerClick(mergeRegion().getByRole("button", { name: "下移第 1 条" }));
    query("missing");
    await screen.findByText("没有匹配的收藏。");
    expect(button("合并复制（2 条）").disabled).toBe(false);
    pointerClick(button("合并复制（2 条）"));
    await screen.findByText("已合并复制 2 条正文，可手动粘贴。");
    expect(f.copy).toHaveBeenCalledExactlyOnceWith(expect.any(String), ["2", "1"]);
    expect(f.hide).not.toHaveBeenCalled();
    pointerClick(mergeRegion().getByRole("button", { name: "移出第 1 条" }));
    expect(mergeRegion().getAllByRole("listitem")).toHaveLength(1);
    pointerClick(button("清空已选"));
    expect(screen.queryByRole("region", { name: "已选合并" })).toBeNull();
  });

  it("blocks IME confirmation during composition and reports a deleted entry for explicit removal", async () => {
    const f = mergeFixture();
    f.copy.mockImplementationOnce(async (request_id, clip_ids) => ({
      request_id, clip_ids, outcome: "not_written", reason: "not_found", missing_clip_id: "2",
    }));
    await panel(f);
    for (const id of ["1", "2"]) pointerClick(row(id).getByRole("button", { name: "加入合并" }));
    const copy = button("合并复制（2 条）");
    fireEvent.compositionStart(search());
    keyGesture(copy, "Enter");
    keyGesture(copy, " ");
    expect(f.copy).not.toHaveBeenCalled();
    fireEvent.compositionEnd(search(), { data: "" });
    keyGesture(copy, "Enter");
    await screen.findByText("未执行合并复制：条目 #2 已不存在，请从已选列表中移出后重试。");
    expect(f.copy).toHaveBeenCalledTimes(1);
    expect(f.hide).not.toHaveBeenCalled();
  });
});
