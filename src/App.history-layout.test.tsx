// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { ClipSummary, ListClipsResult, ListCursor, StatusResult } from "./lib/ipc";

const originalScrollIntoView = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  if (originalScrollIntoView) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", originalScrollIntoView);
  else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
});

function summary(id: string): ClipSummary {
  return { id, captured_at_ms: 1, kind: "text", preview: `布局合成条目${id}`, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown" };
}
function fixture() {
  const calls: Array<{ id: string; cursor: ListCursor | null; query: string; resolve(value: ListClipsResult): void }> = [];
  const status: StatusResult = {
    revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" },
    capture: { user_enabled: false, admission_open: false, policy_epoch: 1 },
    counters: { observed: "3", captured: "3", persisted: "3", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" },
    queue: { items: 0, bytes: 0 }, history: { items: 3, payload_bytes: 24 }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 1, worker_protocol: 1 }, last_paste: null,
  };
  const bridge: DesktopBridge = {
    available: true,
    status: vi.fn(async () => status),
    list: vi.fn((id, cursor, query = "") => new Promise<ListClipsResult>(resolve => calls.push({ id, cursor, query, resolve }))),
    getText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, text: `布局完整正文${clipId}`, next_offset: null })),
    copyText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, outcome: "copied" as const, reason: null })),
    setEnabled: vi.fn(), subscribe: vi.fn(async () => () => {}),
  };
  const hide = vi.fn();
  async function reply(index: number, ids: string[], more = false) {
    const call = calls[index];
    await act(async () => call.resolve({ request_id: call.id, normalized_query: call.query, summaries: ids.map(summary), next_cursor: more ? { normalized_query: call.query, upper_id: "3", last_captured_at_ms: 1, last_id: ids[ids.length - 1] } : null }));
  }
  return { bridge, calls, hide, reply };
}
async function mount(panel: boolean, more = false) {
  const f = fixture();
  render(<App bridge={f.bridge} {...(panel ? { panel: { hide: f.hide } } : {})} />);
  await waitFor(() => expect(f.calls).toHaveLength(1));
  await f.reply(0, ["1", "2"], more);
  await screen.findByText("布局合成条目1");
  const viewport = screen.getByRole("region", { name: "历史列表滚动区域" });
  const list = screen.getByRole("list", { name: "历史条目" });
  return { ...f, viewport, list };
}
function pointerClick(target: HTMLElement) {
  fireEvent.pointerDown(target, { button: 0, pointerId: 1, isPrimary: true });
  fireEvent.pointerUp(target, { button: 0, pointerId: 1, isPrimary: true });
  fireEvent.click(target, { detail: 1 });
}
const rect = (top: number, bottom: number): DOMRect => ({ x: 0, y: top, top, bottom, left: 0, right: 320, width: 320, height: bottom - top, toJSON: () => ({}) });

it.each([false, true])("history layout: %s panel has a named list viewport with search and pagination outside it", async panel => {
  const f = await mount(panel, true);
  expect(f.viewport.contains(f.list)).toBe(true);
  expect(f.viewport.contains(screen.getByLabelText("搜索历史正文"))).toBe(false);
  expect(f.viewport.contains(screen.getByRole("button", { name: "加载更多" }))).toBe(false);
  expect(f.list.querySelector('[data-history-id="1"]')?.textContent).toContain("布局合成条目1");
});

it.each([false, true])("history layout: %s panel appends a page without resetting the list scroll", async panel => {
  const f = await mount(panel, true);
  f.viewport.scrollTop = 140;
  fireEvent.click(screen.getByRole("button", { name: "加载更多" }));
  await waitFor(() => expect(f.calls).toHaveLength(2));
  expect(f.calls[1].cursor).not.toBeNull();
  expect(f.viewport.scrollTop).toBe(140);
  await f.reply(1, ["3"]);
  await screen.findByText("布局合成条目3");
  expect(screen.getAllByRole("button", { name: "查看正文" })).toHaveLength(3);
  expect(f.viewport.scrollTop).toBe(140);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
});

it.each([false, true])("history layout: %s panel resets on a new query and again when its first page replaces results", async panel => {
  const f = await mount(panel);
  f.viewport.scrollTop = 140;
  fireEvent.change(screen.getByLabelText("搜索历史正文"), { target: { value: "新查询" } });
  expect(f.viewport.scrollTop).toBe(0);
  await waitFor(() => expect(f.calls).toHaveLength(2));
  expect(f.calls[1].query).toBe("新查询");
  expect(f.calls[1].cursor).toBeNull();
  f.viewport.scrollTop = 70;
  await f.reply(1, ["3"]);
  await screen.findByText("布局合成条目3");
  expect(f.viewport.scrollTop).toBe(0);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
});

it.each([false, true])("history layout: %s panel same-query refresh resets only after first-page success", async panel => {
  const f = await mount(panel);
  f.viewport.scrollTop = 140;
  fireEvent.click(screen.getByRole("button", { name: "刷新" }));
  await waitFor(() => expect(f.calls).toHaveLength(2));
  expect(f.calls[1].query).toBe("");
  expect(f.calls[1].cursor).toBeNull();
  expect(f.viewport.scrollTop).toBe(140);
  await f.reply(1, ["1", "2"]);
  await waitFor(() => expect(f.viewport.scrollTop).toBe(0));
  expect(f.bridge.copyText).not.toHaveBeenCalled();
});

it("history layout: panel arrows reveal selection only within the viewport and retain list focus without copying", async () => {
  const f = await mount(true);
  Object.defineProperty(f.viewport, "clientHeight", { configurable: true, value: 100 });
  Object.defineProperty(f.viewport, "scrollHeight", { configurable: true, value: 400 });
  vi.spyOn(f.viewport, "getBoundingClientRect").mockImplementation(() => rect(100, 200));
  const first = f.list.querySelector<HTMLElement>('[data-history-id="1"]')!;
  const second = f.list.querySelector<HTMLElement>('[data-history-id="2"]')!;
  vi.spyOn(first, "getBoundingClientRect").mockImplementation(() => rect(100 - f.viewport.scrollTop, 150 - f.viewport.scrollTop));
  vi.spyOn(second, "getBoundingClientRect").mockImplementation(() => rect(225 - f.viewport.scrollTop, 275 - f.viewport.scrollTop));
  const scrollIntoView = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scrollIntoView });
  const windowScroll = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const windowScrollBy = vi.spyOn(window, "scrollBy").mockImplementation(() => {});
  const focus = vi.spyOn(HTMLElement.prototype, "focus");
  document.documentElement.scrollTop = 330;
  document.body.scrollTop = 120;
  pointerClick(screen.getByRole("button", { name: "浏览结果" }));
  await screen.findByText("布局完整正文1");
  expect(document.activeElement).toBe(f.list);
  const listFocusCalls = focus.mock.calls.filter((_call, i) => focus.mock.contexts[i] === f.list);
  expect(listFocusCalls).toContainEqual([{ preventScroll: true }]);
  fireEvent.keyDown(f.list, { key: "ArrowDown", code: "ArrowDown" });
  fireEvent.keyUp(f.list, { key: "ArrowDown", code: "ArrowDown" });
  await screen.findByText("布局完整正文2");
  expect(f.viewport.scrollTop).toBe(75);
  expect(second.getBoundingClientRect().bottom).toBeLessThanOrEqual(f.viewport.getBoundingClientRect().bottom);
  expect(second.querySelector("button")?.getAttribute("aria-pressed")).toBe("true");
  expect(document.activeElement).toBe(f.list);
  fireEvent.keyDown(f.list, { key: "ArrowUp", code: "ArrowUp" });
  fireEvent.keyUp(f.list, { key: "ArrowUp", code: "ArrowUp" });
  await screen.findByText("布局完整正文1");
  expect(f.viewport.scrollTop).toBe(0);
  expect(document.activeElement).toBe(f.list);
  expect(document.documentElement.scrollTop).toBe(330);
  expect(document.body.scrollTop).toBe(120);
  expect(windowScroll).not.toHaveBeenCalled();
  expect(windowScrollBy).not.toHaveBeenCalled();
  expect(scrollIntoView).not.toHaveBeenCalled();
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  expect(f.hide).not.toHaveBeenCalled();
});

it("history layout: an ArrowDown at the last selected row reveals it again after manual scrolling", async () => {
  const f = await mount(true);
  Object.defineProperty(f.viewport, "clientHeight", { configurable: true, value: 100 });
  Object.defineProperty(f.viewport, "scrollHeight", { configurable: true, value: 400 });
  vi.spyOn(f.viewport, "getBoundingClientRect").mockImplementation(() => rect(100, 200));
  const first = f.list.querySelector<HTMLElement>('[data-history-id="1"]')!;
  const last = f.list.querySelector<HTMLElement>('[data-history-id="2"]')!;
  vi.spyOn(first, "getBoundingClientRect").mockImplementation(() => rect(100 - f.viewport.scrollTop, 150 - f.viewport.scrollTop));
  vi.spyOn(last, "getBoundingClientRect").mockImplementation(() => rect(225 - f.viewport.scrollTop, 275 - f.viewport.scrollTop));
  const scrollIntoView = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scrollIntoView });
  const windowScroll = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const windowScrollBy = vi.spyOn(window, "scrollBy").mockImplementation(() => {});
  document.documentElement.scrollTop = 330;
  document.body.scrollTop = 120;
  pointerClick(screen.getByRole("button", { name: "浏览结果" }));
  await screen.findByText("布局完整正文1");
  fireEvent.keyDown(f.list, { key: "ArrowDown", code: "ArrowDown" });
  fireEvent.keyUp(f.list, { key: "ArrowDown", code: "ArrowDown" });
  await screen.findByText("布局完整正文2");
  expect(f.viewport.scrollTop).toBe(75);
  expect(last.querySelector("button")?.getAttribute("aria-pressed")).toBe("true");
  f.viewport.scrollTop = 0;
  fireEvent.scroll(f.viewport);
  expect(last.getBoundingClientRect().bottom).toBeGreaterThan(f.viewport.getBoundingClientRect().bottom);
  fireEvent.keyDown(f.list, { key: "ArrowDown", code: "ArrowDown" });
  fireEvent.keyUp(f.list, { key: "ArrowDown", code: "ArrowDown" });
  expect(f.viewport.scrollTop).toBe(75);
  expect(last.querySelector("button")?.getAttribute("aria-pressed")).toBe("true");
  expect(document.activeElement).toBe(f.list);
  expect(document.documentElement.scrollTop).toBe(330);
  expect(document.body.scrollTop).toBe(120);
  expect(windowScroll).not.toHaveBeenCalled();
  expect(windowScrollBy).not.toHaveBeenCalled();
  expect(scrollIntoView).not.toHaveBeenCalled();
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  expect(f.hide).not.toHaveBeenCalled();
});
