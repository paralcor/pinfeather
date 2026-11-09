// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { StatusResult } from "./lib/ipc";
import type { ResidencyControl, ResidencySnapshot } from "./lib/residency";

const originalScrollY = Object.getOwnPropertyDescriptor(window, "scrollY");
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  if (originalScrollY) Object.defineProperty(window, "scrollY", originalScrollY);
});
function setPageScroll(y: number, notify = true) {
  Object.defineProperty(window, "scrollY", { configurable: true, value: y });
  if (notify) fireEvent.scroll(window);
}
function pointerClick(target: HTMLElement) {
  fireEvent.pointerDown(target, { button: 0, pointerId: 1, isPrimary: true });
  fireEvent.pointerUp(target, { button: 0, pointerId: 1, isPrimary: true });
  fireEvent.click(target, { detail: 1 });
}
function fixture() {
  const status: StatusResult = {
    revision: "1", state: "paused", services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" },
    capture: { user_enabled: false, admission_open: false, policy_epoch: 1 },
    counters: { observed: "2", captured: "2", persisted: "2", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" },
    queue: { items: 0, bytes: 0 }, history: { items: 2, payload_bytes: 16 }, data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 1, worker_protocol: 1 }, last_paste: null,
  };
  const bridge: DesktopBridge = {
    available: true, status: vi.fn(async () => status),
    list: vi.fn(async (requestId, _cursor, query = "") => ({ request_id: requestId, normalized_query: query, summaries: ["1", "2"].map(id => ({ id, captured_at_ms: 1, kind: "text" as const, preview: `回顶合成摘要${id}`, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown" as const })), next_cursor: null })),
    getText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, text: `回顶完整正文${clipId}`, next_offset: null })),
    copyText: vi.fn(async (requestId, clipId) => ({ request_id: requestId, clip_id: clipId, outcome: "copied" as const, reason: null })),
    setEnabled: vi.fn(), subscribe: vi.fn(async () => () => {}),
  };
  let residencyListener: (value: ResidencySnapshot) => void = () => {};
  const residency: ResidencyControl = {
    available: true, snapshot: vi.fn(async (): Promise<ResidencySnapshot> => ({ revision: "1", phase: "running", tray_available: true, error: null })),
    subscribe: vi.fn(async callback => { residencyListener = callback; return () => {}; }),
    exit: vi.fn(), retrySaving: vi.fn(),
  };
  return { bridge, residency, hide: vi.fn(), emitClosing: () => residencyListener({ revision: "2", phase: "stopping", tray_available: true, error: null }) };
}
async function mount(panel: boolean, initialY = 0) {
  setPageScroll(initialY, false);
  const f = fixture();
  const mounted = render(<App bridge={f.bridge} residencyControl={f.residency} {...(panel ? { panel: { hide: f.hide } } : {})} />);
  await screen.findByText("回顶合成摘要1");
  return { ...f, ...mounted };
}
const topButton = () => screen.getByRole("button", { name: "返回顶部" });

it.each([false, true])("back to top: %s panel uses the page threshold and ignores internal scrolling", async panel => {
  await mount(panel);
  expect(screen.queryByRole("button", { name: "返回顶部" })).toBeNull();
  setPageScroll(200);
  expect(screen.queryByRole("button", { name: "返回顶部" })).toBeNull();
  const viewport = screen.getByRole("region", { name: "历史列表滚动区域" });
  viewport.scrollTop = 400;
  fireEvent.scroll(viewport);
  expect(screen.queryByRole("button", { name: "返回顶部" })).toBeNull();
  pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]);
  await screen.findByText("回顶完整正文1");
  const preview = document.querySelector<HTMLElement>(".preview-body")!;
  preview.scrollTop = 300;
  fireEvent.scroll(preview);
  expect(screen.queryByRole("button", { name: "返回顶部" })).toBeNull();
  setPageScroll(201);
  expect(topButton()).toBeTruthy();
  setPageScroll(0);
  expect(screen.queryByRole("button", { name: "返回顶部" })).toBeNull();
});

it.each([false, true])("back to top: %s panel synchronizes an existing page scroll on mount", async panel => {
  await mount(panel, 201);
  expect(topButton()).toBeTruthy();
});

it.each([false, true])("back to top: %s panel preserves history state and nested scrolls while focusing its heading", async panel => {
  const f = await mount(panel, 201);
  const search = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
  fireEvent.change(search, { target: { value: "保留查询" } });
  await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "保留查询"));
  await screen.findByText("回顶合成摘要1");
  pointerClick(screen.getAllByRole("button", { name: "查看正文" })[1]);
  await screen.findByText("回顶完整正文2");
  const viewport = screen.getByRole("region", { name: "历史列表滚动区域" });
  const preview = document.querySelector<HTMLElement>(".preview-body")!;
  viewport.scrollTop = 137;
  preview.scrollTop = 91;
  const heading = screen.getByRole("heading", { level: 1 });
  const focus = vi.spyOn(heading, "focus");
  const scrollTo = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  const lists = vi.mocked(f.bridge.list).mock.calls.length;
  const texts = vi.mocked(f.bridge.getText).mock.calls.length;
  pointerClick(topButton());
  expect(scrollTo).toHaveBeenCalledExactlyOnceWith({ top: 0, behavior: "instant" });
  expect(heading.getAttribute("tabindex")).toBe("-1");
  expect(focus).toHaveBeenCalledWith({ preventScroll: true });
  expect(document.activeElement).toBe(heading);
  expect(search.value).toBe("保留查询");
  expect(screen.getAllByRole("button", { name: "查看正文" })[1].getAttribute("aria-pressed")).toBe("true");
  expect(document.querySelector(".preview-body")?.textContent).toBe("回顶完整正文2");
  expect(viewport.scrollTop).toBe(137);
  expect(preview.scrollTop).toBe(91);
  expect(f.bridge.list).toHaveBeenCalledTimes(lists);
  expect(f.bridge.getText).toHaveBeenCalledTimes(texts);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  expect(f.hide).not.toHaveBeenCalled();
});

it("back to top: removes its window scroll subscription on unmount", async () => {
  const add = vi.spyOn(window, "addEventListener");
  const remove = vi.spyOn(window, "removeEventListener");
  const f = await mount(false);
  const subscriptions = add.mock.calls.filter(call => call[0] === "scroll");
  expect(subscriptions).toHaveLength(1);
  f.unmount();
  expect(remove.mock.calls.some(call => call[0] === "scroll" && call[1] === subscriptions[0][1])).toBe(true);
});

it("back to top: closing disables the page action", async () => {
  const f = await mount(false, 201);
  await waitFor(() => expect(f.residency.snapshot).toHaveBeenCalledTimes(1));
  const scrollTo = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  await act(async () => f.emitClosing());
  expect((topButton() as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(topButton());
  expect(scrollTo).not.toHaveBeenCalled();
  expect(f.bridge.copyText).not.toHaveBeenCalled();
});

it.each([false, true])("back to top: %s panel disables the action when host status becomes stopping", async panel => {
  const f = await mount(panel, 201);
  const previous = await f.bridge.status();
  vi.mocked(f.bridge.status).mockResolvedValue({ ...previous, revision: "2", state: "stopping" });
  const scrollTo = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  fireEvent.click(screen.getByRole("button", { name: "刷新" }));
  await waitFor(() => expect((topButton() as HTMLButtonElement).disabled).toBe(true));
  expect(window.scrollY).toBe(201);
  fireEvent.click(topButton());
  expect(scrollTo).not.toHaveBeenCalled();
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  expect(f.hide).not.toHaveBeenCalled();
});

it("back to top: panel activation cannot complete an armed copy gesture", async () => {
  const f = await mount(true, 201);
  pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]);
  await screen.findByText("回顶完整正文1");
  const copy = screen.getByRole("button", { name: "复制纯文本" });
  copy.focus();
  fireEvent.keyDown(copy, { key: "Enter", code: "Enter" });
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  pointerClick(topButton());
  const heading = screen.getByRole("heading", { level: 1 });
  fireEvent.keyUp(heading, { key: "Enter", code: "Enter" });
  fireEvent.click(copy, { detail: 0 });
  expect(document.activeElement).toBe(heading);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  expect(f.hide).not.toHaveBeenCalled();
});

it("back to top: panel IME key and subsequent explicit page action never copy or hide", async () => {
  const f = await mount(true, 201);
  pointerClick(screen.getAllByRole("button", { name: "查看正文" })[0]);
  await screen.findByText("回顶完整正文1");
  const copy = screen.getByRole("button", { name: "复制纯文本" });
  fireEvent.keyDown(copy, { key: "Process", code: "Enter", keyCode: 229, isComposing: true });
  fireEvent.keyUp(topButton(), { key: "Enter", code: "Enter" });
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  pointerClick(topButton());
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  expect(f.hide).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(screen.getByRole("heading", { level: 1 }));
});
