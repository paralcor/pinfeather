// @vitest-environment jsdom
// KeyG/g stand for sanitized "other" identities, not recovered native keys.
// This is an App/DOM ordering regression, not native IME or clipboard proof.
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
    status: vi.fn(async () => ({ revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" }, capture: { user_enabled: false, admission_open: false, policy_epoch: 1 }, counters: { observed: "1", captured: "1", persisted: "1", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" }, queue: { items: 0, bytes: 0 }, history: { items: 1, payload_bytes: 8 }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 1, worker_protocol: 1 }, last_paste: null } as StatusResult)),
    list: vi.fn(async (requestId, _cursor, query = "") => ({ request_id: requestId, normalized_query: query, summaries: [{ id: "1", captured_at_ms: 1, kind: "text" as const, preview: "两步合成摘要", format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown" as const }], next_cursor: null })),
    getText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, text: "合成正文", next_offset: null })),
    copyText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, outcome: "copied" as const, reason: null })),
    setEnabled: vi.fn(), subscribe: vi.fn(async () => () => {}),
  };
  const mounted = render(<App bridge={bridge} panel={{ hide }} />);
  await screen.findByText("两步合成摘要");
  return { bridge, hide, ...mounted, search: screen.getByLabelText("搜索历史正文") as HTMLInputElement };
}
function pointer(button: HTMLElement) { fireEvent.pointerDown(button, { button: 0, pointerId: 1 }); button.focus(); fireEvent.pointerUp(button, { button: 0, pointerId: 1 }); fireEvent.click(button, { detail: 1 }); }
function button(name: string) { return screen.getByRole("button", { name }) as HTMLButtonElement; }
function rows() { return screen.getAllByRole("button", { name: "查看正文" }).map(row => row.getAttribute("aria-pressed")); }
function noAction(f: Awaited<ReturnType<typeof mount>>) { expect(rows()).toEqual(["false"]); expect(f.bridge.getText).not.toHaveBeenCalled(); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled(); }
const process = { key: "Process", code: "KeyG", keyCode: 229, isComposing: true };
function browse() { pointer(button("浏览结果")); return screen.getByRole("list", { name: "历史条目" }); }
function complete() { pointer(button("完成输入")); }

it("owner control: end before completion acknowledges without product effects", async () => {
 const f=await mount(); fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search); complete(); noAction(f); expect(f.search.readOnly).toBe(true); expect(button("浏览结果").disabled).toBe(false);
});
it("owner: final update during the same completion pointer must not require a third activation", async () => {
 const f=await mount(); fireEvent.compositionStart(f.search);
 fireEvent.change(f.search,{target:{value:"yang"}}); await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"yang"));
 const ack=button("完成输入"); fireEvent.pointerDown(ack); ack.focus();
 fireEvent.compositionUpdate(f.search,{data:"样"}); fireEvent.change(f.search,{target:{value:"样"}}); fireEvent.compositionEnd(f.search,{data:"样"});
 fireEvent.pointerUp(ack); fireEvent.click(ack,{detail:1});
 await screen.findByText("两步合成摘要"); noAction(f); expect(f.search.readOnly).toBe(true); await waitFor(()=>expect(button("浏览结果").disabled).toBe(false)); expect(f.search.value).toBe("样");
});
it("owner: pending completion must preserve the final committed query before allowing browse", async () => {
 const f=await mount(); fireEvent.compositionStart(f.search); fireEvent.change(f.search,{target:{value:"yang"}});
 await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"yang")); complete();
 expect(button("浏览结果").disabled).toBe(true);
 fireEvent.compositionEnd(f.search,{data:"样"}); fireEvent.change(f.search,{target:{value:"样"}});
 await screen.findByText("两步合成摘要"); noAction(f); await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"样")); expect(f.search.value).toBe("样");
});

it.each(["before-complete", "pending-before-end", "pending-after-end", "end-before-complete"])("synthetic final query delivery: %s preserves the acknowledged episode without an action", async order => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  fireEvent.change(f.search, { target: { value: "yang" } }); await screen.findByText("两步合成摘要");
  if (order === "before-complete") fireEvent.change(f.search, { target: { value: "样" } });
  if (order === "end-before-complete") fireEvent.compositionEnd(f.search, { data: "样" });
  complete();
  if (order === "pending-before-end") fireEvent.change(f.search, { target: { value: "样" } });
  if (order !== "end-before-complete") fireEvent.compositionEnd(f.search, { data: order === "pending-before-end" ? "" : "样" });
  if (order === "pending-after-end" || order === "end-before-complete") fireEvent.change(f.search, { target: { value: "样" } });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "样"));
  await screen.findByText("两步合成摘要"); noAction(f); expect(f.search.value).toBe("样"); expect(f.search.readOnly).toBe(true);
  fireEvent.change(f.search, { target: { value: "arbitrary ready edit" } });
  expect(f.search.value).toBe("样"); expect(button("浏览结果").disabled).toBe(false);
  browse(); expect(f.bridge.getText).toHaveBeenCalledTimes(1); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
});

it("synthetic final composition data replaces only the original selected search range", async () => {
  const f = await mount(); fireEvent.change(f.search, { target: { value: "前旧后" } }); await screen.findByText("两步合成摘要");
  f.search.setSelectionRange(1, 2); fireEvent.compositionStart(f.search);
  fireEvent.change(f.search, { target: { value: "前yang后" } }); await screen.findByText("两步合成摘要"); complete();
  fireEvent.compositionEnd(f.search, { data: "样" }); fireEvent.change(f.search, { target: { value: "前样后" } });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "前样后"));
  await screen.findByText("两步合成摘要"); noAction(f); expect(f.search.value).toBe("前样后");
  fireEvent.compositionEnd(f.search, { data: "过期" }); fireEvent.change(f.search, { target: { value: "过期" } });
  expect(f.search.value).toBe("前样后"); noAction(f);
});

it("same-episode update after pending completion preserves acknowledgement and commits final text", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); complete();
  fireEvent.compositionUpdate(f.search, { data: "样" }); fireEvent.change(f.search, { target: { value: "样" } });
  expect(f.search.readOnly).toBe(true); expect(button("浏览结果").disabled).toBe(true);
  fireEvent.compositionEnd(f.search, { data: "样" }); await screen.findByText("两步合成摘要");
  noAction(f); expect(button("浏览结果").disabled).toBe(false); expect(f.search.value).toBe("样");
});

it("Process-only episode preserves its original replacement range across repeated IME keys", async () => {
  const f = await mount(); fireEvent.keyDown(f.search, process);
  fireEvent.change(f.search, { target: { value: "yang" } }); await screen.findByText("两步合成摘要");
  fireEvent.keyDown(f.search, process); complete(); fireEvent.compositionEnd(f.search, { data: "样" });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "样"));
  await screen.findByText("两步合成摘要"); noAction(f); expect(f.search.value).toBe("样");
});

it("final query loading rejects Browse and stale provisional results until the final result arrives", async () => {
  const f = await mount();
  type Page = Awaited<ReturnType<DesktopBridge["list"]>>;
  const initial = await vi.mocked(f.bridge.list).mock.results[0].value as Page;
  const replies = new Map<string, () => void>();
  vi.mocked(f.bridge.list).mockImplementation((requestId, _cursor, query = "") => new Promise(resolve => {
    replies.set(query, () => resolve({ ...initial, request_id: requestId, normalized_query: query, summaries: initial.summaries.map(row => ({ ...row, preview: `${query}结果` })) }));
  }));
  fireEvent.compositionStart(f.search); fireEvent.change(f.search, { target: { value: "yang" } });
  await waitFor(() => expect(replies.has("yang")).toBe(true)); complete();
  fireEvent.compositionEnd(f.search, { data: "样" });
  expect(button("浏览结果").disabled).toBe(true);
  const entry = button("浏览结果"); entry.removeAttribute("disabled"); pointer(entry);
  expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull(); expect(f.bridge.getText).not.toHaveBeenCalled();
  await waitFor(() => expect(replies.has("样")).toBe(true));
  await act(async () => replies.get("yang")!());
  expect(screen.queryByText("yang结果")).toBeNull(); expect(f.search.value).toBe("样");
  expect(f.bridge.getText).not.toHaveBeenCalled(); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
  await act(async () => replies.get("样")!()); await screen.findByText("样结果"); noAction(f);
  browse(); expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1);
});

it("true new start cancels the old completion pointer and replaces its final-text snapshot", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  const ack = button("完成输入"); fireEvent.pointerDown(ack); ack.focus();
  fireEvent.compositionStart(f.search); fireEvent.compositionUpdate(f.search, { data: "新" });
  fireEvent.compositionEnd(f.search, { data: "新" }); fireEvent.pointerUp(ack); fireEvent.click(ack, { detail: 1 });
  await screen.findByText("两步合成摘要"); noAction(f); expect(f.search.readOnly).toBe(false); expect(f.search.value).toBe("新");
  expect(button("浏览结果").disabled).toBe(true); complete(); expect(f.search.readOnly).toBe(true); noAction(f);
});

it("old App final events cannot change a new App query or readiness", async () => {
  const old = await mount(); fireEvent.compositionStart(old.search); complete(); old.unmount();
  const current = await mount(); fireEvent.change(current.search, { target: { value: "新会话" } }); await screen.findByText("两步合成摘要");
  fireEvent.compositionEnd(old.search, { data: "旧" }); fireEvent.change(old.search, { target: { value: "旧" } });
  expect(current.search.value).toBe("新会话"); expect(current.search.readOnly).toBe(false); noAction(current);
  expect(current.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "新会话");
});

it("same React batch cannot browse provisional rows after final text starts a new query", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.change(f.search, { target: { value: "yang" } });
  await screen.findByText("两步合成摘要"); complete(); const entry = button("浏览结果");
  act(() => {
    fireEvent.compositionEnd(f.search, { data: "样" }); entry.removeAttribute("disabled"); pointer(entry);
  });
  expect(screen.queryByRole("button", { name: "返回搜索" })).toBeNull(); expect(f.bridge.getText).not.toHaveBeenCalled();
  await screen.findByText("两步合成摘要"); noAction(f); expect(f.search.value).toBe("样");
});
