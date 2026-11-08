// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { CopyClipTextResult, StatusResult } from "./lib/ipc";
import { createHistoryModel } from "./lib/history-store";

afterEach(cleanup);
function fixture() {
  const calls: Array<{ requestId: string; clipId: string; resolve: (value: CopyClipTextResult) => void; reject: (error: unknown) => void }> = [];
  const bridge: DesktopBridge = {
    available: true,
    status: vi.fn(async () => ({ revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "not_started", target_observer: "not_started" }, capture: { user_enabled: false, admission_open: false, policy_epoch: 1 }, counters: { observed: "2", captured: "2", persisted: "2", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" }, queue: { items: 0, bytes: 0 }, history: { items: 2, payload_bytes: 100 }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 1, worker_protocol: 1 }, last_paste: null } as StatusResult)),
    list: vi.fn(async requestId => ({ request_id: requestId, normalized_query: "", summaries: ["1", "2"].map(id => ({ id, captured_at_ms: 1, kind: "text" as const, preview: `摘要${id}`, format_names: ["CF_UNICODETEXT", "HTML Format"], omitted_format_count: 0, payload_bytes: 100, source_process: null, source_confidence: "unknown" as const })), next_cursor: null })),
    getText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, text: "仅首段", next_offset: 9 })),
    copyText: vi.fn((requestId, clipId) => new Promise<CopyClipTextResult>((resolve, reject) => calls.push({ requestId, clipId, resolve, reject }))),
    setEnabled: vi.fn(), subscribe: vi.fn(async () => () => {}),
  };
  return { bridge, calls };
}
async function selected() {
  const h = fixture(); const mounted = render(<App bridge={h.bridge} />);
  await screen.findByText("摘要1"); fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[0]);
  await screen.findByText("仅首段"); return { ...h, ...mounted };
}
describe("explicit copy", () => {
  it("copies only on an explicit native button activation, by ID despite a partial preview", async () => {
    const h = await selected(); expect(h.calls).toHaveLength(0);
    const button = screen.getByRole("button", { name: "复制纯文本" });
    expect(button.tagName).toBe("BUTTON"); fireEvent.click(button); fireEvent.click(button);
    expect(h.calls).toHaveLength(1); expect(h.calls[0].clipId).toBe("1");
    await act(async () => h.calls[0].resolve({ request_id: h.calls[0].requestId, clip_id: "1", outcome: "copied", reason: null }));
    expect(await screen.findByText("已复制，可手动粘贴。")).toBeTruthy();
  });
  it.each(["selection", "search", "refresh", "unmount"])("does not attach late success or replay after %s", async action => {
    const h = await selected(); fireEvent.click(screen.getByRole("button", { name: "复制纯文本" }));
    if (action === "selection") fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[1]);
    if (action === "search") fireEvent.change(screen.getByLabelText("搜索历史正文"), { target: { value: "new" } });
    if (action === "refresh") fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    if (action === "unmount") h.unmount();
    await act(async () => h.calls[0].resolve({ request_id: h.calls[0].requestId, clip_id: "1", outcome: "copied", reason: null }));
    expect(screen.queryByText("已复制，可手动粘贴。")).toBeNull(); expect(h.calls).toHaveLength(1);
  });
  it.each(["unknown", "rejection", "correlation"])("treats %s as uncertain without automatic retry", async mode => {
    const h = await selected(); fireEvent.click(screen.getByRole("button", { name: "复制纯文本" }));
    await act(async () => mode === "rejection" ? h.calls[0].reject({ code: "busy" }) : h.calls[0].resolve({ request_id: h.calls[0].requestId, clip_id: mode === "correlation" ? "2" : "1", outcome: mode === "correlation" ? "copied" : "unknown", reason: mode === "correlation" ? null : "timeout" }));
    expect(await screen.findByText("无法确认复制结果，剪贴板可能已变化。请自行检查；不会自动重试。")).toBeTruthy();
    expect(h.calls).toHaveLength(1);
  });
  it("allows a new explicit attempt after a confirmed pre-write failure", async () => {
    const h = await selected(); fireEvent.click(screen.getByRole("button", { name: "复制纯文本" }));
    await act(async () => h.calls[0].resolve({ request_id: h.calls[0].requestId, clip_id: "1", outcome: "not_written", reason: "busy" }));
    expect(await screen.findByText("未执行复制：当前操作繁忙，请稍后手动重试。")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "复制纯文本" })); expect(h.calls).toHaveLength(2);
  });
  it("keeps one outstanding call when selection changes", async () => {
    const h = await selected(); fireEvent.click(screen.getByRole("button", { name: "复制纯文本" }));
    fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[1]);
    expect((screen.getByRole("button", { name: "复制中…" }) as HTMLButtonElement).disabled).toBe(true);
    expect(h.calls).toHaveLength(1);
    await act(async () => h.calls[0].resolve({ request_id: h.calls[0].requestId, clip_id: "1", outcome: "copied", reason: null }));
    await waitFor(() => expect((screen.getByRole("button", { name: "复制纯文本" }) as HTMLButtonElement).disabled).toBe(false));
  });
  it("disables copy for an older bridge without the capability", async () => {
    const h = fixture(); delete (h.bridge as Partial<typeof h.bridge>).copyText;
    render(<App bridge={h.bridge} />); await screen.findByText("摘要1");
    expect((screen.getByRole("button", { name: "复制纯文本" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("当前宿主不支持复制纯文本。")).toBeTruthy();
  });
  it.each(["nontext", "closing", "storage"])("disables the copy action for %s", async mode => {
    const h = fixture();
    if (mode === "nontext") {
      const list = h.bridge.list;
      h.bridge.list = async (...args) => { const page = await list(...args); page.summaries.forEach(row => row.format_names = ["HTML Format"]); return page; };
    } else {
      const status = h.bridge.status;
      h.bridge.status = async () => { const value = await status(); if (mode === "closing") value.state = "stopping"; else value.services.storage = "failed"; return value; };
    }
    render(<App bridge={h.bridge} />); await screen.findByText("摘要1"); fireEvent.click(screen.getAllByRole("button", { name: "查看正文" })[0]);
    expect((screen.getByRole("button", { name: "复制纯文本" }) as HTMLButtonElement).disabled).toBe(true); expect(h.calls).toHaveLength(0);
  });
  it("does not dispatch without a desktop host or selection", async () => {
    const h = fixture(); const model = createHistoryModel(h.bridge); const stop = model.start();
    await waitFor(() => expect(model.store.getState().summaries).toHaveLength(2)); await model.copyText(); expect(h.calls).toHaveLength(0); stop();
    h.bridge.available = false; render(<App bridge={h.bridge} />); expect(screen.getByText(/需要桌面宿主/)).toBeTruthy(); expect(h.calls).toHaveLength(0);
  });
  it("keeps the unsettled call across stop/start and ignores its old completion", async () => {
    const h = fixture(); const model = createHistoryModel(h.bridge); const stop = model.start();
    await waitFor(() => expect(model.store.getState().summaries).toHaveLength(2)); model.selectClip("1"); const call = model.copyText(); expect(h.calls).toHaveLength(1);
    stop(); const stopAgain = model.start(); await waitFor(() => expect(model.store.getState().loading).toBe(false)); model.selectClip("2"); await model.copyText(); expect(h.calls).toHaveLength(1);
    h.calls[0].resolve({ request_id: h.calls[0].requestId, clip_id: "1", outcome: "copied", reason: null }); await call;
    expect(model.store.getState().copyMessage).toBeNull(); expect(model.store.getState().copyBusy).toBe(false); stopAgain();
  });
});
