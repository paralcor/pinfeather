// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { StatusResult } from "./lib/ipc";

afterEach(cleanup);
async function mount() {
  const hide = vi.fn();
  const bridge: DesktopBridge = {
    available: true,
    status: vi.fn(async () => ({ revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" }, capture: { user_enabled: false, admission_open: false, policy_epoch: 1 }, counters: { observed: "2", captured: "2", persisted: "2", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" }, queue: { items: 0, bytes: 0 }, history: { items: 2, payload_bytes: 16 }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 1, worker_protocol: 1 }, last_paste: null } as StatusResult)),
    list: vi.fn(async (requestId, _cursor, query = "") => ({ request_id: requestId, normalized_query: query, summaries: ["1", "2"].map(id => ({ id, captured_at_ms: 1, kind: "text" as const, preview: `条目${id}`, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown" as const })), next_cursor: null })),
    getText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, text: `完整正文${clipId}`, next_offset: null })),
    copyText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, outcome: "copied" as const, reason: null })),
    setEnabled: vi.fn(), subscribe: vi.fn(async () => () => {}),
  };
  const mounted = render(<App bridge={bridge} panel={{ hide }} />);
  await screen.findByText("条目1");
  return { bridge, hide, ...mounted, search: screen.getByLabelText("搜索历史正文") as HTMLInputElement };
}
const button = (name: string) => screen.getByRole("button", { name }) as HTMLButtonElement;
const entry = (n = 0) => screen.getAllByRole("button", { name: "查看正文" })[n] as HTMLButtonElement;
function down(el: HTMLElement, id = 1) { fireEvent.pointerDown(el, { button: 0, pointerId: id, isPrimary: true }); el.focus(); }
function up(el: HTMLElement, id = 1) { fireEvent.pointerUp(el, { button: 0, pointerId: id, isPrimary: true }); }
function pointer(el: HTMLElement) { down(el); up(el); fireEvent.click(el, { detail: 1 }); }
function keyboard(el: HTMLElement, key = "Enter", code = "Enter") { el.focus(); fireEvent.keyDown(el, { key, code }); fireEvent.keyUp(el, { key, code }); }
const ime = { key: "Process", code: "Enter", keyCode: 229, isComposing: true };

it("C: previews without an end receipt and has no completion gate or clipboard side effect", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  expect(entry().disabled).toBe(false); pointer(entry());
  await screen.findByText("完整正文1");
  expect(f.search.readOnly).toBe(false);
  expect(screen.queryByRole("button", { name: "完成输入" })).toBeNull();
  expect(screen.queryByRole("button", { name: "确认搜索词" })).toBeNull();
  expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
});
it("C: a candidate-ending click only previews; copying needs its own full pointer gesture", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); down(entry());
  fireEvent.compositionEnd(f.search); up(entry()); fireEvent.click(entry(), { detail: 1 });
  await screen.findByText("完整正文1");
  fireEvent.click(button("复制纯文本"), { detail: 1 });
  fireEvent.click(button("复制纯文本"), { detail: 0 });
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  pointer(button("复制纯文本")); await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
  fireEvent.click(button("复制纯文本"), { detail: 1 }); fireEvent.click(button("复制纯文本"), { detail: 0 });
  expect(f.bridge.copyText).toHaveBeenCalledTimes(1); expect(f.hide).not.toHaveBeenCalled();
});
it.each(["Enter", " "])("C: %s copy executes on a fresh matching release and never default click or repeats", async key => {
  const f = await mount(); pointer(entry()); await screen.findByText("完整正文1");
  const copy = button("复制纯文本"), code = key === " " ? "Space" : "Enter"; copy.focus();
  fireEvent.keyDown(copy, { key, code }); expect(f.bridge.copyText).not.toHaveBeenCalled();
  fireEvent.keyDown(copy, { key, code, repeat: true }); fireEvent.click(copy, { detail: 0 });
  fireEvent.keyUp(copy, { key, code }); expect(f.bridge.copyText).not.toHaveBeenCalled();
  keyboard(copy, key, code); await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
  fireEvent.keyUp(copy, { key, code }); fireEvent.click(copy, { detail: 0 });
  expect(f.bridge.copyText).toHaveBeenCalledTimes(1);
});
it("C: candidate release moved to the copy control cannot write or hide", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.keyDown(f.search, ime);
  pointer(entry()); await screen.findByText("完整正文1");
  const copy = button("复制纯文本"); copy.focus();
  fireEvent.keyUp(copy, { key: "Process", code: "Enter", isComposing: false }); fireEvent.click(copy, { detail: 0 });
  expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
  keyboard(copy); await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
});
it.each(["Enter", " ", "Escape", "ArrowDown", "ArrowUp"])("C: candidate %s in search never selects, reads, writes or hides", async key => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  fireEvent.keyDown(f.search, { key, code: key === " " ? "Space" : key, isComposing: true });
  fireEvent.keyUp(f.search, { key, code: key === " " ? "Space" : key, isComposing: false });
  fireEvent.click(f.search, { detail: 0 });
  expect(f.bridge.getText).not.toHaveBeenCalled(); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
});
it("C: wrong target, mismatched pointer, cancellation and blur cannot finish a copy", async () => {
  const f = await mount(); pointer(entry()); await screen.findByText("完整正文1");
  const copy = button("复制纯文本");
  down(entry()); up(copy); fireEvent.click(copy, { detail: 1 });
  down(copy, 1); up(copy, 2); fireEvent.click(copy, { detail: 1 });
  down(copy); fireEvent.pointerCancel(copy, { pointerId: 1 }); up(copy); fireEvent.click(copy, { detail: 1 });
  down(copy); fireEvent.blur(window); up(copy); fireEvent.click(copy, { detail: 1 });
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  pointer(copy); await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
});
it("C: new query invalidates a begun pointer even when the same item returns", async () => {
  const f = await mount(); pointer(entry()); await screen.findByText("完整正文1"); const copy = button("复制纯文本"); down(copy);
  fireEvent.change(f.search, { target: { value: "新查询" } });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "新查询"));
  await screen.findByText("条目1"); fireEvent.click(entry(), { detail: 1 }); await screen.findByText("完整正文1");
  up(copy); fireEvent.click(copy, { detail: 1 }); expect(f.bridge.copyText).not.toHaveBeenCalled();
  pointer(copy); await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
});
it("C: changing selection invalidates an armed key and only the current body can be copied", async () => {
  const f = await mount(); pointer(entry()); await screen.findByText("完整正文1"); const copy = button("复制纯文本"); copy.focus();
  fireEvent.keyDown(copy, { key: "Enter", code: "Enter" });
  fireEvent.click(entry(1), { detail: 1 }); await screen.findByText("完整正文2");
  fireEvent.keyUp(copy, { key: "Enter", code: "Enter" }); fireEvent.click(copy, { detail: 0 });
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  pointer(copy); await waitFor(() => expect(f.bridge.copyText).toHaveBeenLastCalledWith(expect.any(String), "2"));
});
it("C: final composition data preserves its original replacement range and invalidates old selection", async () => {
  const f = await mount(); fireEvent.change(f.search, { target: { value: "前旧后" } });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "前旧后"));
  f.search.setSelectionRange(1, 2); fireEvent.compositionStart(f.search); fireEvent.change(f.search, { target: { value: "前yang后" } });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "前yang后"));
  pointer(entry()); await screen.findByText("完整正文1");
  fireEvent.compositionEnd(f.search, { data: "样" });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "前样后"));
  expect(f.search.value).toBe("前样后"); expect(button("复制纯文本").disabled).toBe(true); expect(f.bridge.copyText).not.toHaveBeenCalled();
});
it("C: an admitted copy stays owned and its late result cannot claim a newer selection", async () => {
  const f = await mount(); let finish!: (value: Awaited<ReturnType<NonNullable<DesktopBridge["copyText"]>>>) => void;
  vi.mocked(f.bridge.copyText!).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  pointer(entry()); await screen.findByText("完整正文1"); pointer(button("复制纯文本"));
  await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
  pointer(entry(1)); await screen.findByText("完整正文2"); pointer(button("复制中…")); expect(f.bridge.copyText).toHaveBeenCalledTimes(1);
  const request = vi.mocked(f.bridge.copyText!).mock.calls[0];
  await act(async () => finish({ request_id: request[0], clip_id: request[1], outcome: "copied", reason: null }));
  expect(screen.queryByText("已复制，可手动粘贴。")).toBeNull();
  pointer(button("复制纯文本")); await waitFor(() => expect(f.bridge.copyText).toHaveBeenCalledTimes(2));
});
it("C: a destroyed session's controls and key releases cannot act on the next session", async () => {
  const old = await mount(); pointer(entry()); await screen.findByText("完整正文1"); const copy = button("复制纯文本"); down(copy); old.unmount();
  const current = await mount(); up(copy); fireEvent.click(copy, { detail: 1 }); fireEvent.keyUp(copy, { key: "Enter", code: "Enter" });
  expect(current.bridge.copyText).not.toHaveBeenCalled(); expect(current.bridge.getText).not.toHaveBeenCalled(); expect(current.hide).not.toHaveBeenCalled();
});
