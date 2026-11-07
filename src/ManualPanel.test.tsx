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
  fireEvent.pointerDown(target, { button: 0, pointerId: 1 });
  fireEvent.pointerUp(target, { button: 0, pointerId: 1 });
  fireEvent.click(target, { detail: 1 });
}
function browse() { pointerClick(screen.getByRole("button", { name: "浏览结果" })); return screen.getByRole("list", { name: "历史条目" }); }
function completeInput() {
  pointerClick(screen.getByRole("button", { name: "完成输入" }));
  expect(selectedRows()).toEqual(["false", "false"]);
  expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
}
function completeAndBrowse() { completeInput(); return browse(); }
function selectedRows() { return screen.getAllByRole("button", { name: "查看正文" }).map(button => button.getAttribute("aria-pressed")); }
describe("explicit browse", () => {
  it("hides on Escape in browse mode even when Tab reaches the read-only query", async () => {
    const f = await panel(); browse(); const search = screen.getByLabelText("搜索历史正文"); search.focus();
    fireEvent.keyDown(search, { key: "Escape" }); expect(f.hide).toHaveBeenCalledTimes(1);
  });
  it("does not recycle transition buttons for a still-held Space default click", async () => {
    const f = await panel(); const entry = screen.getByRole("button", { name: "浏览结果" });
    entry.focus(); fireEvent.keyDown(entry, { key: " ", code: "Space" }); pointerClick(entry);
    expect(entry.isConnected).toBe(false);
    fireEvent.keyUp(entry, { key: " ", code: "Space" }); fireEvent.click(entry, { detail: 0 });
    const back = screen.getByRole("button", { name: "返回搜索" });
    back.focus(); fireEvent.keyDown(back, { key: " ", code: "Space" }); pointerClick(back);
    expect(back.isConnected).toBe(false);
    fireEvent.keyUp(back, { key: " ", code: "Space" }); fireEvent.click(back, { detail: 0 });
    expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull(); expect(selectedRows()).toEqual(["false", "false"]);
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
  });
  it.each([false, true])("completes then enters with a distinct click after Space (release before end: %s), then owns list commands", async releaseFirst => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: "Process", code: "Space", isComposing: true });
    if (!releaseFirst) fireEvent.compositionEnd(search);
    fireEvent.keyUp(search, { key: "Process", code: "Space", isComposing: releaseFirst });
    if (releaseFirst) fireEvent.compositionEnd(search);
    fireEvent.keyDown(search, { key: "ArrowDown", code: "ArrowDown" }); fireEvent.keyUp(search, { key: "ArrowDown", code: "ArrowDown" });
    fireEvent.keyDown(search, { key: "Escape", code: "Escape" }); fireEvent.keyUp(search, { key: "Escape", code: "Escape" });
    expect(selectedRows()).toEqual(["false", "false"]); expect(f.hide).not.toHaveBeenCalled();
    const list = completeAndBrowse(); expect(document.activeElement).toBe(list); expect(selectedRows()).toEqual(["true", "false"]);
    fireEvent.keyDown(list, { key: "ArrowDown" }); expect(selectedRows()).toEqual(["false", "true"]);
    fireEvent.keyDown(list, { key: "ArrowDown" }); expect(selectedRows()).toEqual(["false", "true"]);
    fireEvent.keyDown(list, { key: "ArrowUp" }); expect(selectedRows()).toEqual(["true", "false"]);
    fireEvent.keyDown(list, { key: "Enter" }); fireEvent.keyDown(list, { key: "Enter", repeat: true });
    expect(f.writes).toHaveLength(1); expect(f.writes[0].clipId).toBe("1"); expect(f.hide).not.toHaveBeenCalled();
    await act(async () => f.writes[0].resolve({ request_id: f.writes[0].requestId, clip_id: "1", outcome: "copied", reason: null }));
    await screen.findByText("已复制，可手动粘贴。");
    fireEvent.keyDown(list, { key: "Escape" }); fireEvent.keyDown(list, { key: "Escape", repeat: true }); expect(f.hide).toHaveBeenCalledTimes(1);
  });
  it.each(["浏览结果", "查看正文", "隐藏快捷面板"])("blocks candidate-active and composition-ending %s clicks until completion and a distinct gesture", async name => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文"); const button = screen.getAllByRole("button", { name })[0];
    fireEvent.compositionStart(search); pointerClick(button);
    expect(selectedRows()).toEqual(["false", "false"]); expect(f.hide).not.toHaveBeenCalled();
    fireEvent.pointerDown(button); fireEvent.compositionEnd(search); fireEvent.pointerUp(button); fireEvent.click(button, { detail: 1 });
    expect(selectedRows()).toEqual(["false", "false"]); expect(f.hide).not.toHaveBeenCalled(); expect(f.writes).toHaveLength(0);
    expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
    completeInput();
    pointerClick(button);
    if (name === "隐藏快捷面板") expect(f.hide).toHaveBeenCalledTimes(1);
    else { expect(document.activeElement).toBe(screen.getByRole("list", { name: "历史条目" })); expect(selectedRows()).toEqual(["true", "false"]); }
  });
  it("keeps search keys and default candidate clicks inert, including after an observed terminal release", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    for (const key of ["ArrowDown", "ArrowUp", "Enter", "Escape", "Tab"]) { const nativeDefault = fireEvent.keyDown(search, { key }); if (key === "Enter") expect(nativeDefault).toBe(false); fireEvent.keyUp(search, { key }); }
    expect(selectedRows()).toEqual(["false", "false"]); expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    const button = screen.getByRole("button", { name: "浏览结果" }); fireEvent.compositionStart(search);
    fireEvent.keyDown(button, { key: "Enter" }); fireEvent.keyUp(button, { key: "Enter" }); fireEvent.compositionEnd(search); fireEvent.click(button, { detail: 0 });
    expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
    fireEvent.keyDown(search, { key: "Tab" }); fireEvent.keyUp(search, { key: "Tab" }); fireEvent.click(button, { detail: 0 });
    expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
    completeInput();
    button.focus(); const down = fireEvent.keyDown(button, { key: " ", code: "Space" }); const up = fireEvent.keyUp(button, { key: " ", code: "Space" });
    expect(down && up).toBe(true); if (down && up) fireEvent.click(button, { detail: 0 });
    expect(document.activeElement).toBe(screen.getByRole("list", { name: "历史条目" })); expect(f.writes).toHaveLength(0);
  });
  it("preserves a held candidate key across pointer entry and return-to-search", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: "Process", code: "Enter", isComposing: true }); fireEvent.compositionEnd(search);
    const list = completeAndBrowse(); fireEvent.keyDown(list, { key: "Enter", code: "Enter", repeat: true }); fireEvent.click(screen.getByRole("button", { name: "复制纯文本" }), { detail: 0 });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    pointerClick(screen.getByRole("button", { name: "返回搜索" })); expect(document.activeElement).toBe(search); expect(selectedRows()).toEqual(["false", "false"]);
    fireEvent.keyUp(search, { key: "Process", code: "Enter", isComposing: false }); fireEvent.keyDown(search, { key: "Enter", code: "Enter" });
    expect(f.writes).toHaveLength(0); expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
  });
  it("returns to editable search, drops pending preview and never copies on the transition", async () => {
    const f = fixture(); let finish!: (value: Awaited<ReturnType<DesktopBridge["getText"]>>) => void;
    f.bridge.getText = vi.fn(() => new Promise<Awaited<ReturnType<DesktopBridge["getText"]>>>(resolve => { finish = resolve; })); await panel(f);
    const list = browse(); const search = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
    expect(search.readOnly).toBe(true); expect(screen.getByText("正在读取正文…")).toBeTruthy();
    pointerClick(screen.getByRole("button", { name: "返回搜索" })); expect(document.activeElement).toBe(search); expect(search.readOnly).toBe(false);
    await act(async () => finish({ request_id: "preview-1", clip_id: "1", text: "迟到正文", next_offset: null }));
    expect(screen.queryByText("迟到正文")).toBeNull(); expect(selectedRows()).toEqual(["false", "false"]);
    fireEvent.keyDown(search, { key: "Enter" }); expect(f.writes).toHaveLength(0); expect(list).toBeTruthy();
  });
  it("cannot enter while loading or empty and keeps browse selection clear across refresh", async () => {
    const f = await panel(); const list = browse(); const originalList = f.bridge.list;
    let finish!: (value: Awaited<ReturnType<DesktopBridge["list"]>>) => void;
    f.bridge.list = vi.fn(() => new Promise<Awaited<ReturnType<DesktopBridge["list"]>>>(resolve => { finish = resolve; }));
    pointerClick(screen.getByRole("button", { name: "刷新" }));
    fireEvent.keyDown(list, { key: "Enter" }); expect(f.writes).toHaveLength(0);
    await act(async () => finish({ request_id: "page1-2", normalized_query: "", summaries: [], next_cursor: null }));
    fireEvent.keyDown(list, { key: "ArrowDown" }); fireEvent.keyDown(list, { key: "Enter" }); expect(f.writes).toHaveLength(0);
    pointerClick(screen.getByRole("button", { name: "返回搜索" })); expect((screen.getByRole("button", { name: "浏览结果" }) as HTMLButtonElement).disabled).toBe(true);
    f.bridge.list = originalList; pointerClick(screen.getByRole("button", { name: "刷新" }));
    expect((screen.getByRole("button", { name: "浏览结果" }) as HTMLButtonElement).disabled).toBe(true);
    await screen.findByText("摘要1"); const nextList = browse();
    pointerClick(screen.getByRole("button", { name: "刷新" })); await waitFor(() => expect(screen.queryByText("正在读取历史…")).toBeNull());
    expect(selectedRows()).toEqual(["false", "false"]); fireEvent.keyDown(nextList, { key: "Enter" }); expect(f.writes).toHaveLength(0);
    fireEvent.keyDown(nextList, { key: "ArrowUp" }); expect(selectedRows()).toEqual(["false", "true"]);
  });
  it.each(["not_written", "unknown"] as const)("copies by ID during pending preview, prevents duplicates, and keeps %s feedback", async outcome => {
    const f = fixture(); f.bridge.getText = vi.fn(() => new Promise<Awaited<ReturnType<DesktopBridge["getText"]>>>(() => {})); await panel(f);
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[1]); const list = screen.getByRole("list", { name: "历史条目" });
    expect(document.activeElement).toBe(list); expect(selectedRows()).toEqual(["false", "true"]); expect(f.writes).toHaveLength(0);
    fireEvent.keyDown(list, { key: "Enter" }); fireEvent.keyDown(list, { key: "Enter", repeat: true });
    pointerClick(screen.getByRole("button", { name: "复制中…" })); expect(f.writes).toHaveLength(1); expect(f.writes[0].clipId).toBe("2");
    await act(async () => f.writes[0].resolve({ request_id: f.writes[0].requestId, clip_id: "2", outcome, reason: outcome === "not_written" ? "busy" : "protocol_error" }));
    await screen.findByText(outcome === "not_written" ? "未执行复制：当前操作繁忙，请稍后手动重试。" : "无法确认复制结果，剪贴板可能已变化。请自行检查；不会自动重试。");
    expect(f.writes).toHaveLength(1); expect(f.hide).not.toHaveBeenCalled(); expect(screen.getByRole("button", { name: "返回搜索" })).toBeTruthy();
  });
  it("supports an independent keyboard Browse activation without implicitly copying", async () => {
    const f = await panel(); const button = screen.getByRole("button", { name: "浏览结果" }); button.focus();
    const nativeDefault = fireEvent.keyDown(button, { key: "Enter" }); expect(nativeDefault).toBe(true);
    if (nativeDefault) fireEvent.click(button, { detail: 0 });
    expect(document.activeElement).toBe(screen.getByRole("list", { name: "历史条目" })); expect(f.writes).toHaveLength(0);
    fireEvent.keyDown(screen.getByRole("list", { name: "历史条目" }), { key: "Enter", repeat: true }); expect(f.writes).toHaveLength(0);
  });
  it("explains an unavailable browse transition after a failed list refresh", async () => {
    const f = await panel(); f.bridge.list = vi.fn(async () => { throw new Error("synthetic failure"); });
    pointerClick(screen.getByRole("button", { name: "刷新" })); await screen.findByText("无法完成操作，请重试。");
    expect((screen.getByRole("button", { name: "浏览结果" }) as HTMLButtonElement).disabled).toBe(true);
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]);
    expect(selectedRows()).toEqual(["false", "false"]); expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull();
    expect(screen.getByText("暂时无法浏览，请刷新后重试。")).toBeTruthy(); expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
  });
  it("clears selection on close-preview, retains list focus and requires new selection before copying", async () => {
    const f = await panel(); const list = browse(); await screen.findByText("长正文仅首段");
    pointerClick(screen.getByRole("button", { name: "关闭正文预览" })); expect(document.activeElement).toBe(list); expect(selectedRows()).toEqual(["false", "false"]);
    fireEvent.keyDown(list, { key: "Enter" }); expect(f.writes).toHaveLength(0);
    fireEvent.keyDown(list, { key: "ArrowDown" }); expect(selectedRows()).toEqual(["true", "false"]);
    pointerClick(screen.getByRole("button", { name: "复制纯文本" })); expect(f.writes).toHaveLength(1);
  });
});
describe("manual panel keyboard", () => {
  it.each(["Enter", "Escape"].flatMap(code => [false, true].map(releaseFirst => ({ code, releaseFirst }))))("keeps search ArrowDown inert after Process/$code release (before end: $releaseFirst) until explicit entry", async ({ code, releaseFirst }) => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    fireEvent.compositionStart(search);
    fireEvent.keyDown(search, { key: "Process", code, isComposing: true });
    if (!releaseFirst) fireEvent.compositionEnd(search);
    fireEvent.keyUp(search, { key: "Process", code, isComposing: releaseFirst });
    if (releaseFirst) fireEvent.compositionEnd(search);
    fireEvent.keyDown(search, { key: "ArrowDown", code: "ArrowDown" });
    expect(selectedRows()).toEqual(["false", "false"]);
    completeAndBrowse(); expect(selectedRows()).toEqual(["true", "false"]);
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
  });
  it.each(["Enter", "Escape"].flatMap(code => ["Enter", "Escape"].map(next => ({ code, next }))))("accepts independent list $next after physical Process/$code and explicit completion/Browse", async ({ code, next }) => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: "Process", code, isComposing: true });
    fireEvent.compositionEnd(search); fireEvent.keyUp(search, { key: "Process", code, isComposing: false });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    completeAndBrowse();
    fireEvent.keyDown(screen.getByRole("list", { name: "历史条目" }), { key: next, code: next });
    expect(f.writes).toHaveLength(next === "Enter" ? 1 : 0); expect(f.hide).toHaveBeenCalledTimes(next === "Escape" ? 1 : 0);
  });
  it.each(["Enter", "Escape", "Space", "ArrowDown"].flatMap(code => ["down-end-up", "down-up-end", "end-down-up"].map(order => ({ code, order }))))("keeps same Process/$code candidate action and default click inert ($order)", async ({ code, order }) => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const button = screen.getByRole("button", { name: "复制纯文本" });
    fireEvent.compositionStart(search);
    if (order === "end-down-up") fireEvent.compositionEnd(search);
    expect(fireEvent.keyDown(button, { key: "Process", code, isComposing: true })).toBe(false);
    if (order === "down-end-up") fireEvent.compositionEnd(search);
    fireEvent.keyUp(button, { key: "Process", code, isComposing: order === "down-up-end" });
    if (order === "down-up-end") fireEvent.compositionEnd(search);
    fireEvent.click(button, { detail: 0 });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    expect(screen.getAllByRole("button", { name: "查看正文" }).map(button => button.getAttribute("aria-pressed"))).toEqual(["false", "false"]);
  });
  it.each(["copy", "hide"])("accepts exactly one native Space %s after Process/Enter release, completion/Browse and Tab", async action => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const button = screen.getByRole("button", { name: action === "copy" ? "复制纯文本" : "隐藏快捷面板" });
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: "Process", code: "Enter", isComposing: true });
    fireEvent.compositionEnd(search); fireEvent.keyUp(search, { key: "Process", code: "Enter", isComposing: false });
    fireEvent.click(button, { detail: 0 }); expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    completeAndBrowse();
    fireEvent.keyDown(search, { key: "Tab", code: "Tab" }); fireEvent.keyUp(search, { key: "Tab", code: "Tab" }); button.focus();
    const down = fireEvent.keyDown(button, { key: " ", code: "Space" });
    const up = fireEvent.keyUp(button, { key: " ", code: "Space" });
    expect(down && up).toBe(true); expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    if (down && up) fireEvent.click(button, { detail: 0 });
    expect(f.writes).toHaveLength(action === "copy" ? 1 : 0); expect(f.hide).toHaveBeenCalledTimes(action === "hide" ? 1 : 0);
  });
  it.each(["ArrowUp", "ArrowDown"].flatMap(arrow => ["Enter", "Escape"].flatMap(commit => [false, true].map(selected => ({ arrow, commit, selected })))))("blocks $arrow navigation then end-before-$commit with selected=$selected", async ({ arrow, commit, selected }) => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    if (selected) { pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段"); }
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: arrow }); fireEvent.keyUp(search, { key: arrow });
    fireEvent.compositionEnd(search); fireEvent.keyDown(search, { key: commit });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    expect(screen.getAllByRole("button", { name: "查看正文" }).map(button => button.getAttribute("aria-pressed"))).toEqual(["false", "false"]);
    fireEvent.keyUp(search, { key: commit });
    completeInput();
    if (selected) pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); else browse();
    fireEvent.keyDown(screen.getByRole("list", { name: "历史条目" }), { key: "Escape" }); expect(f.hide).toHaveBeenCalledTimes(1);
  });
  it.each(["Enter", " "].flatMap(candidate => ["copy", "hide"].map(action => ({ candidate, action }))))("accepts native Tab then independent Space for $action after candidate $candidate and explicit completion/Browse", async ({ candidate, action }) => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const button = screen.getByRole("button", { name: action === "copy" ? "复制纯文本" : "隐藏快捷面板" });
    fireEvent.compositionStart(search);
    const candidateDefault = fireEvent.keyDown(button, { key: candidate }); fireEvent.keyUp(button, { key: candidate });
    if (candidateDefault) fireEvent.click(button, { detail: 0 });
    if (candidate === " ") { fireEvent.keyDown(button, { key: "Enter" }); fireEvent.keyUp(button, { key: "Enter" }); }
    fireEvent.compositionEnd(search); fireEvent.click(button, { detail: 0 });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    completeAndBrowse();
    fireEvent.keyDown(search, { key: "Tab" }); fireEvent.keyUp(search, { key: "Tab" }); button.focus();
    const spaceDownDefault = fireEvent.keyDown(button, { key: " " }); const spaceUpDefault = fireEvent.keyUp(button, { key: " " });
    expect(spaceDownDefault).toBe(true); expect(spaceUpDefault).toBe(true);
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    if (spaceDownDefault && spaceUpDefault) fireEvent.click(button, { detail: 0 });
    expect(f.writes).toHaveLength(action === "copy" ? 1 : 0); expect(f.hide).toHaveBeenCalledTimes(action === "hide" ? 1 : 0);
    if (action === "copy") expect(f.writes[0].clipId).toBe("1");
  });
  it("observes native Tab and repeated text without consuming their defaults or authorizing a terminal key", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: "Enter" }); fireEvent.keyUp(search, { key: "Enter" });
    expect(fireEvent.keyDown(search, { key: "Tab" })).toBe(true); fireEvent.keyUp(search, { key: "Tab" });
    expect(fireEvent.keyDown(search, { key: "a", repeat: true })).toBe(true); fireEvent.keyUp(search, { key: "a" });
    fireEvent.compositionEnd(search); fireEvent.keyDown(search, { key: "Escape" });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    fireEvent.keyUp(search, { key: "Escape" }); fireEvent.keyDown(search, { key: "Escape" }); expect(f.hide).not.toHaveBeenCalled();
    completeInput(); pointerClick(screen.getByRole("button", { name: "隐藏快捷面板" })); expect(f.hide).toHaveBeenCalledTimes(1);
  });
  it.each(["select", "copy", "hide"])("requires completion/Browse before a fresh pointer %s after mouse candidate completion without keyup", async action => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    fireEvent.compositionStart(search); fireEvent.compositionEnd(search);
    completeAndBrowse();
    if (action === "select") {
      pointerClick(screen.getAllByRole("button", { name: "查看正文" })[1]);
      expect(screen.getAllByRole("button", { name: "查看正文" })[1].getAttribute("aria-pressed")).toBe("true");
      expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    } else if (action === "copy") {
      pointerClick(screen.getByRole("button", { name: "复制纯文本" })); expect(f.writes).toHaveLength(1); expect(f.writes[0].clipId).toBe("1");
    } else {
      pointerClick(screen.getByRole("button", { name: "隐藏快捷面板" })); expect(f.hide).toHaveBeenCalledTimes(1); expect(f.writes).toHaveLength(0);
    }
  });
  it.each([false, true])("blocks the pointer gesture that ends composition, then requires completion/Browse before a new pointer gesture (candidate already released: %s)", async released => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const button = screen.getByRole("button", { name: "复制纯文本" });
    fireEvent.compositionStart(search);
    if (released) { fireEvent.keyDown(search, { key: "Enter" }); fireEvent.keyUp(search, { key: "Enter" }); }
    fireEvent.pointerDown(button); fireEvent.compositionEnd(search);
    fireEvent.pointerUp(button); fireEvent.click(button, { detail: 1 }); expect(f.writes).toHaveLength(0);
    completeAndBrowse();
    pointerClick(button); expect(f.writes).toHaveLength(1);
  });
  it("blocks a candidate button default click even when keyup precedes compositionEnd", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const button = screen.getByRole("button", { name: "复制纯文本" }); fireEvent.compositionStart(search);
    expect(fireEvent.keyDown(button, { key: "Enter" })).toBe(false);
    fireEvent.keyUp(button, { key: "Enter" }); fireEvent.compositionEnd(search); fireEvent.click(button, { detail: 0 });
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    completeAndBrowse();
    fireEvent.keyDown(button, { key: "Enter" }); expect(f.writes).toHaveLength(1);
    fireEvent.keyDown(button, { key: "Enter", repeat: true }); fireEvent.click(button, { detail: 0 }); expect(f.writes).toHaveLength(1);
  });
  it.each(["Escape", "Enter"])("accepts independent list %s after candidate keyup precedes compositionEnd and explicit completion/Browse", async next => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    fireEvent.compositionStart(search); fireEvent.keyDown(search, { key: "Enter" }); fireEvent.keyUp(search, { key: "Enter" }); fireEvent.compositionEnd(search);
    expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled();
    completeAndBrowse();
    fireEvent.keyDown(screen.getByRole("list", { name: "历史条目" }), { key: next });
    expect(f.writes).toHaveLength(next === "Enter" ? 1 : 0); expect(f.hide).toHaveBeenCalledTimes(next === "Escape" ? 1 : 0);
  });
  it("explicitly enters with zero writes, copies stored ID with a partial preview, and stays open", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    fireEvent.keyDown(search, { key: "Enter" }); expect(f.writes).toHaveLength(0);
    const list = browse(); await screen.findByText("长正文仅首段"); expect(f.writes).toHaveLength(0);
    fireEvent.keyDown(list, { key: "Enter" }); fireEvent.keyDown(list, { key: "Enter", repeat: true }); expect(f.writes).toHaveLength(1); expect(f.writes[0].clipId).toBe("1");
    await act(async () => f.writes[0].resolve({ request_id: f.writes[0].requestId, clip_id: "1", outcome: "copied", reason: null }));
    await screen.findByText("已复制，可手动粘贴。"); expect(f.hide).not.toHaveBeenCalled(); expect(screen.getByRole("heading", { name: "片羽 · 快捷面板" })).toBeTruthy();
    fireEvent.keyDown(list, { key: "Escape" }); expect(f.hide).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(list, { key: "Escape", repeat: true }); expect(f.hide).toHaveBeenCalledTimes(1);
  });
  it("assigns button Enter to exactly one copy owner and suppresses its native default", async () => {
    const f = await panel(); fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const button = screen.getByRole("button", { name: "复制纯文本" });
    const nativeDefault = fireEvent.keyDown(button, { key: "Enter" }); if (nativeDefault) fireEvent.click(button);
    expect(nativeDefault).toBe(false); expect(f.writes).toHaveLength(1);
    fireEvent.keyDown(button, { key: "Enter", repeat: true }); expect(f.writes).toHaveLength(1);
  });
  it.each(["Enter", "Escape", "ArrowUp", "ArrowDown"])("protects IME %s in both compositionEnd/keydown orders", async key => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文");
    fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[0]); await screen.findByText("长正文仅首段");
    const list = screen.getByRole("list", { name: "历史条目" });
    for (const endFirst of [false, true]) {
      fireEvent.compositionStart(search); if (endFirst) fireEvent.compositionEnd(search);
      fireEvent.keyDown(list, { key }); if (!endFirst) fireEvent.compositionEnd(search);
      expect(f.writes).toHaveLength(0); expect(f.hide).not.toHaveBeenCalled(); expect(screen.getAllByRole("button", { name: "查看正文" })[0].getAttribute("aria-pressed")).toBe("false");
      fireEvent.keyUp(list, { key });
    }
    if (key === "ArrowUp" || key === "ArrowDown") {
      fireEvent.keyDown(list, { key: "Escape" }); expect(f.hide).not.toHaveBeenCalled();
      fireEvent.keyUp(list, { key: "Escape" });
    }
    completeAndBrowse(); fireEvent.keyDown(list, { key: "Escape" }); expect(f.hide).toHaveBeenCalledTimes(1);
  });
  it("blocks Process/229 and button activation during composition", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文"); fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[0]);
    fireEvent.keyDown(search, { key: "Process", keyCode: 229 }); fireEvent.keyDown(search, { key: "Enter" }); expect(f.writes).toHaveLength(0); fireEvent.keyUp(search, { key: "Enter" });
    fireEvent.compositionStart(search); const button = screen.getByRole("button", { name: "复制纯文本" }); fireEvent.keyDown(button, { key: "Enter" }); fireEvent.click(button); expect(f.writes).toHaveLength(0);
    fireEvent.compositionEnd(search); fireEvent.keyUp(search, { key: "Enter" }); completeAndBrowse(); pointerClick(button); expect(f.writes).toHaveLength(1);
  });
  it("invalidates selection on search and drops an old copy result without retry", async () => {
    const f = await panel(); const search = screen.getByLabelText("搜索历史正文"); const list = browse(); fireEvent.keyDown(list, { key: "Enter" });
    pointerClick(screen.getByRole("button", { name: "返回搜索" }));
    fireEvent.change(search, { target: { value: "新搜索" } }); fireEvent.keyDown(search, { key: "Enter" }); expect(f.writes).toHaveLength(1);
    await act(async () => f.writes[0].resolve({ request_id: f.writes[0].requestId, clip_id: "1", outcome: "copied", reason: null }));
    expect(screen.queryByText("已复制，可手动粘贴。")).toBeNull(); expect(f.writes).toHaveLength(1);
  });
});

function controller() {
  let current: ManualPanelSnapshot = { revision: "1", session_id: a, phase: "waiting", hotkey: "ready", error: null };
  let listener: ((value: ManualPanelSnapshot) => void) | undefined;
  const histories: ReturnType<typeof fixture>[] = []; const unsubscribe = vi.fn();
  const control: ManualPanelControl = {
    available: true, snapshot: vi.fn(async () => current), subscribe: vi.fn(async callback => { listener = callback; return unsubscribe; }),
    open: vi.fn(async () => current), ready: vi.fn(async () => current), hide: vi.fn(async () => current),
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
    const ack = screen.getByRole("button", { name: "完成输入" }); pointerClick(ack);
    const oldBrowse = screen.getByRole("button", { name: "浏览结果" });
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null })); await screen.findByText("摘要1");
    await act(async () => finish(await originalList("old-query", null, "旧输入")));
    fireEvent.compositionEnd(oldSearch); pointerClick(oldBrowse); fireEvent.keyUp(ack, { key: "Enter", code: "Enter" });
    const search = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
    expect(search.value).toBe(""); expect(search.readOnly).toBe(false); expect(document.activeElement).toBe(search);
    expect(screen.queryByRole("button", { name: "完成输入" })).toBeNull(); expect(selectedRows()).toEqual(["false", "false"]);
    expect(f.histories.every(h => h.writes.length === 0 && vi.mocked(h.bridge.getText).mock.calls.length === 0)).toBe(true);
    expect(f.control.hide).not.toHaveBeenCalled(); browse(); expect(selectedRows()).toEqual(["true", "false"]);
  });
  it("resets an active browse mode and list focus for every new accepted session", async () => {
    const f = controller(); render(<ManualPanel control={f.control} />); await screen.findByText("摘要1");
    const list = browse(); fireEvent.keyDown(list, { key: "ArrowDown" }); expect(selectedRows()).toEqual(["false", "true"]);
    expect((screen.getByLabelText("搜索历史正文") as HTMLInputElement).readOnly).toBe(true);
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
    const search = screen.getByLabelText("搜索历史正文"); const list = browse(); fireEvent.keyDown(list, { key: "Enter" });
    pointerClick(screen.getByRole("button", { name: "返回搜索" }));
    fireEvent.change(search, { target: { value: "旧查询" } });
    act(() => f.emit({ revision: "2", session_id: b, phase: "waiting", hotkey: "ready", error: null })); await screen.findByText("摘要1");
    expect((screen.getByLabelText("搜索历史正文") as HTMLInputElement).value).toBe(""); expect(screen.getAllByRole("button", { name: "查看正文" }).every(button => button.getAttribute("aria-pressed") === "false")).toBe(true);
    expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull(); expect(document.activeElement).toBe(screen.getByLabelText("搜索历史正文"));
    const old = f.histories[0].writes[0]; await act(async () => old.resolve({ request_id: old.requestId, clip_id: "1", outcome: "copied", reason: null })); expect(screen.queryByText("已复制，可手动粘贴。")).toBeNull();
    expect(f.control.ready).toHaveBeenCalledWith(a); expect(f.control.ready).toHaveBeenCalledWith(b);
    act(() => f.emit({ revision: "1", session_id: a, phase: "ready", hotkey: "ready", error: null }));
    fireEvent.keyDown(screen.getByLabelText("搜索历史正文"), { key: "Escape" }); expect(f.control.hide).not.toHaveBeenCalled();
    fireEvent.keyDown(browse(), { key: "Escape" }); expect(f.control.hide).toHaveBeenCalledWith(b);
  });
  it("unmounts the session on host hide and cleans its event subscription", async () => { const f = controller(); const mounted = render(<ManualPanel control={f.control} />); await screen.findByText("摘要1"); act(() => f.emit({ revision: "2", session_id: null, phase: "hidden", hotkey: "ready", error: null })); expect(screen.queryByLabelText("搜索历史正文")).toBeNull(); mounted.unmount(); expect(f.unsubscribe).toHaveBeenCalledTimes(1); });
  it("keeps the main entry available when hotkey registration fails", async () => { const f = controller(); f.control.snapshot = vi.fn(async () => ({ revision: "1", session_id: null, phase: "hidden", hotkey: "failed", error: "Ctrl+Alt+V 注册失败，可使用主窗口入口。" } as ManualPanelSnapshot)); const h = fixture(); render(<App bridge={h.bridge} panelControl={f.control} />); await screen.findByText("Ctrl+Alt+V 注册失败，可使用主窗口入口。"); const button = screen.getByRole("button", { name: "打开快捷面板" }) as HTMLButtonElement; expect(button.disabled).toBe(false); fireEvent.click(button); await waitFor(() => expect(f.control.open).toHaveBeenCalledTimes(1)); expect(h.writes).toHaveLength(0); });
});
