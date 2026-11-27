// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { ClipSummary, PngPreview, StatusResult } from "./lib/ipc";

const status: StatusResult = {
  revision: "1", state: "paused",
  services: { listener: "ready", worker: "not_started", storage: "ready", hotkey: "ready", target_observer: "not_started" },
  capture: { user_enabled: false, admission_open: false, policy_epoch: 1 },
  counters: { observed: "2", captured: "2", persisted: "2", ignored: "0", superseded: "0", failed: "0", overloaded: "0", cancelled: "0" },
  queue: { items: 0, bytes: 0 }, history: { items: 2, payload_bytes: 20, favorites: 0, revision: "0" },
  data_dir: "D:\\Synthetic", versions: { app: "0.1.0", schema: 2, worker_protocol: 1 }, last_paste: null,
};
const image: PngPreview = { sourceWidth: 640, sourceHeight: 320, width: 2, height: 1, rgba: new Uint8ClampedArray(8) };
const summaries: ClipSummary[] = [
  { id: "2", captured_at_ms: 2, kind: "png", preview: "", format_names: ["PNG"], omitted_format_count: 0, payload_bytes: 10, source_process: null, source_confidence: "unknown", favorited: false },
  { id: "1", captured_at_ms: 1, kind: "text", preview: "合成图文", format_names: ["CF_UNICODETEXT", "PNG"], omitted_format_count: 0, payload_bytes: 10, source_process: null, source_confidence: "unknown", favorited: false },
];
const fixture = () => ({
  available: true,
  status: vi.fn(async () => status),
  list: vi.fn<DesktopBridge["list"]>(async (requestId, _cursor, query = "") => ({
    request_id: requestId, normalized_query: query, summaries, next_cursor: null,
  })),
  getText: vi.fn<DesktopBridge["getText"]>(async (requestId, clipId) => ({
    request_id: requestId, clip_id: clipId, text: "实际文本正文", next_offset: null,
  })),
  getPngPreview: vi.fn<NonNullable<DesktopBridge["getPngPreview"]>>(async () => image),
  copyText: vi.fn<NonNullable<DesktopBridge["copyText"]>>(async (requestId, clipId) => ({
    request_id: requestId, clip_id: clipId, outcome: "copied", reason: null,
  })),
  copyPng: vi.fn<NonNullable<DesktopBridge["copyPng"]>>(async (requestId, clipId) => ({
    request_id: requestId, clip_id: clipId, outcome: "copied", reason: null,
  })),
  setEnabled: vi.fn(async () => status),
  subscribe: vi.fn(async () => () => {}),
}) satisfies DesktopBridge;

beforeEach(() => vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("loads PNG only for the selected ID and previews text on mixed entries", async () => {
  const bridge = fixture();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole("button", { name: "查看图片" }));
  expect(await screen.findByRole("img", { name: "PNG 缩略图" })).toBeTruthy();
  expect(bridge.getPngPreview).toHaveBeenCalledExactlyOnceWith(expect.any(String), "2");
  expect(bridge.getText).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "查看正文" }));
  expect(await screen.findByText("实际文本正文")).toBeTruthy();
  await waitFor(() => expect(bridge.getPngPreview).toHaveBeenLastCalledWith(expect.any(String), "1"));
  expect(await screen.findByRole("img", { name: "PNG 缩略图" })).toBeTruthy();
});

it("ignores a superseded PNG response and reports errors on the current entry", async () => {
  const bridge = fixture();
  let finish!: (value: PngPreview) => void;
  bridge.getPngPreview
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockRejectedValueOnce({ code: "invalid_input" });
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole("button", { name: "查看图片" }));
  fireEvent.click(screen.getByRole("button", { name: "查看正文" }));
  expect(bridge.getPngPreview).toHaveBeenCalledTimes(1);
  await act(async () => finish(image));
  expect(await screen.findByText("PNG 已损坏或超出预览限制。")).toBeTruthy();
  expect(screen.queryByRole("img", { name: "PNG 缩略图" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "重试读取图片" }));
  expect(await screen.findByRole("img", { name: "PNG 缩略图" })).toBeTruthy();
  expect(bridge.getPngPreview).toHaveBeenLastCalledWith(expect.any(String), "1");
});

it("copies a previewed image from the main window and keeps the preview", async () => {
  const bridge = fixture();
  render(<App bridge={bridge} />);
  fireEvent.click(await screen.findByRole("button", { name: "查看图片" }));
  await screen.findByRole("img", { name: "PNG 缩略图" });
  fireEvent.click(screen.getByRole("button", { name: "复制图片" }));
  expect(await screen.findByText("图片已复制，可手动粘贴。")).toBeTruthy();
  expect(bridge.copyPng).toHaveBeenCalledExactlyOnceWith(expect.any(String), "2");
  expect(bridge.copyText).not.toHaveBeenCalled();
  expect(screen.getByRole("img", { name: "PNG 缩略图" })).toBeTruthy();
});

it("requires a PNG button gesture in the panel and keeps Enter as text copy for mixed entries", async () => {
  const bridge = fixture();
  const hide = vi.fn(async () => {});
  render(<App bridge={bridge} panel={{ hide }} />);
  fireEvent.click(await screen.findByRole("button", { name: "查看图片" }));
  const enter = (target: HTMLElement) => {
    target.focus();
    fireEvent.keyDown(target, { key: "Enter", code: "Enter" });
    fireEvent.keyUp(target, { key: "Enter", code: "Enter" });
  };
  await screen.findByText("使用「复制图片」将此图片写入剪贴板。");
  const list = screen.getByRole("list", { name: "历史条目" });
  const imageButton = screen.getByRole("button", { name: "复制图片" });
  enter(list);
  fireEvent.click(imageButton, { detail: 0 });
  expect(bridge.copyPng).not.toHaveBeenCalled();
  expect(bridge.copyText).not.toHaveBeenCalled();
  expect(hide).not.toHaveBeenCalled();
  enter(imageButton);
  await screen.findByText("图片已复制，可手动粘贴。");
  expect(bridge.copyPng).toHaveBeenCalledExactlyOnceWith(expect.any(String), "2");
  expect(hide).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "查看正文" }));
  await screen.findByText("实际文本正文");
  enter(list);
  await waitFor(() => expect(hide).toHaveBeenCalledExactlyOnceWith(true));
  expect(bridge.copyText).toHaveBeenCalledExactlyOnceWith(expect.any(String), "1");
  expect(bridge.copyPng).toHaveBeenCalledTimes(1);
});
