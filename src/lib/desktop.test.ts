import { beforeEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/response_dtos.json";
const mocks = vi.hoisted(() => ({ invoke: vi.fn<(...args: unknown[]) => Promise<unknown>>(), listen: vi.fn<(name: string, callback: (event: { payload: unknown }) => void) => Promise<() => void>>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
import { nativeBridge } from "./desktop";
beforeEach(() => { mocks.invoke.mockReset(); mocks.listen.mockReset(); });
it("uses the existing status contract and one request argument for changes", async () => {
  mocks.invoke.mockResolvedValue(fixture.status_cases.find(item => item.id === "st_ok")!.value);
  const status = await nativeBridge.status(); expect(status.revision).toBe("3");
  await nativeBridge.setEnabled(false); expect(mocks.invoke).toHaveBeenLastCalledWith("set_capture_enabled", { request: { enabled: false } });
});
it("verifies list correlation instead of displaying an unrelated response", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "wrong", normalized_query: "", summaries: [], next_cursor: null });
  await expect(nativeBridge.list("current", null)).rejects.toThrow();
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "current", query: "", limit: 50, cursor: null } });
});
it("rejects unknown status shapes and ignores malformed event revisions", async () => {
  mocks.invoke.mockResolvedValue({ private_data: "never display" }); await expect(nativeBridge.status()).rejects.toThrow("无法读取宿主数据");
  const off = vi.fn(); const callback = vi.fn(); mocks.listen.mockResolvedValue(off); const dispose = await nativeBridge.subscribe("history_changed", callback);
  const listener = mocks.listen.mock.calls[0][1]; listener({ payload: { revision: "01" } }); listener({ payload: { revision: "1", extra: true } }); expect(callback).not.toHaveBeenCalled();listener({ payload: { revision: "1" } });expect(callback).toHaveBeenCalledTimes(1);dispose();expect(off).toHaveBeenCalledTimes(1);
});

function summary(id: string) {
  return { id, captured_at_ms: 1, kind: "text" as const, preview: "p", format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 1, source_process: null, source_confidence: "unknown" as const };
}

it("keeps the legacy empty-query request shape for list without a query argument", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "r", normalized_query: "", summaries: [], next_cursor: null });
  await nativeBridge.list("r", null);
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "r", query: "", limit: 50, cursor: null } });
});

it("sends the normalized query literally and accepts its own non-empty echo", async () => {
  const sent = "剪贴  板%_q'\"\\z";
  mocks.invoke.mockResolvedValue({ request_id: "q1", normalized_query: sent, summaries: [], next_cursor: null });
  const result = await nativeBridge.list("q1", null, "\u{3000}\u{2003}剪贴  板%_q'\"\\z\u{a0}\t");
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "q1", query: sent, limit: 50, cursor: null } });
  expect(result.request_id).toBe("q1");
  expect(result.normalized_query).toBe(sent);
});

it("keeps the page snapshot upper_id and rejects mismatched correlation", async () => {
  const cursor = { normalized_query: "中", upper_id: "9", last_captured_at_ms: 5, last_id: "5" };
  const page = { request_id: "p1", normalized_query: "中", summaries: [], next_cursor: { normalized_query: "中", upper_id: "9", last_captured_at_ms: 4, last_id: "4" } };
  mocks.invoke.mockResolvedValue(page);
  await nativeBridge.list("p1", cursor, "中");
  expect(mocks.invoke).toHaveBeenCalledWith("list_clips", { request: { request_id: "p1", query: "中", limit: 50, cursor } });

  mocks.invoke.mockResolvedValue({ ...page, request_id: "other" });
  await expect(nativeBridge.list("p1", cursor, "中")).rejects.toThrow();
  mocks.invoke.mockResolvedValue({ ...page, normalized_query: "别的" });
  await expect(nativeBridge.list("p1", cursor, "中")).rejects.toThrow();

  mocks.invoke.mockClear();
  await expect(nativeBridge.list("p1", { normalized_query: "别的", upper_id: "9", last_captured_at_ms: 5, last_id: "5" }, "中")).rejects.toThrow();
  expect(mocks.invoke).not.toHaveBeenCalled();

  mocks.invoke.mockResolvedValue({ ...page, next_cursor: { normalized_query: "中", upper_id: "8", last_captured_at_ms: 4, last_id: "4" } });
  await expect(nativeBridge.list("p1", cursor, "中")).rejects.toThrow();
});

it("rejects more summaries than this request's limit even under the global cap", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "s1", normalized_query: "", summaries: Array.from({ length: 51 }, (_, i) => summary(String(i + 1))), next_cursor: null });
  await expect(nativeBridge.list("s1", null)).rejects.toThrow();
});

it("rejects invalid list inputs before invoke and never echoes the raw query", async () => {
  const long = "密".repeat(257);
  mocks.invoke.mockClear();
  const longError = await nativeBridge.list("q", null, long).then(() => null, (e: unknown) => e as Error);
  expect(longError).toBeInstanceOf(Error);
  expect(String(longError)).not.toContain("密");
  expect(mocks.invoke).not.toHaveBeenCalled();
  await expect(nativeBridge.list("", null)).rejects.toThrow();
  await expect(nativeBridge.list("q\n", null)).rejects.toThrow();
  await expect(nativeBridge.list("q", { normalized_query: "q", upper_id: "0", last_captured_at_ms: 1, last_id: "1" })).rejects.toThrow();
  expect(mocks.invoke).not.toHaveBeenCalled();
});

it("invokes get_clip_text with the exact request and UTF-8 byte progress", async () => {
  mocks.invoke.mockResolvedValue({ request_id: "t1", clip_id: "3", text: "中😀", next_offset: 10 });
  const frame = await nativeBridge.getText("t1", "3", 3);
  expect(mocks.invoke).toHaveBeenCalledWith("get_clip_text", { request: { request_id: "t1", clip_id: "3", offset: 3 } });
  expect(frame.text).toBe("中😀");
  expect(frame.next_offset).toBe(10);

  mocks.invoke.mockResolvedValue({ request_id: "t1", clip_id: "3", text: "尾", next_offset: null });
  const end = await nativeBridge.getText("t1", "3", 10);
  expect(end.text).toBe("尾");
  expect(end.next_offset).toBeNull();
});

it("rejects invalid text requests and responses without inventing text", async () => {
  const badRequests: [string, string, number][] = [
    ["t", "0", 0], ["t", "abc", 0], ["t", "1", -1], ["t", "1", 1.5], ["t", "1", 9007199254740992], ["", "1", 0],
  ];
  for (const [rid, cid, offset] of badRequests) {
    mocks.invoke.mockClear();
    await expect(nativeBridge.getText(rid, cid, offset)).rejects.toThrow();
    expect(mocks.invoke).not.toHaveBeenCalled();
  }
  const bad: unknown[] = [
    { request_id: "other", clip_id: "1", text: "a", next_offset: 2 },
    { request_id: "t", clip_id: "2", text: "a", next_offset: 2 },
    { request_id: "t", clip_id: "1", text: "a", next_offset: 99 },
    { request_id: "t", clip_id: "1", text: "x".repeat(65537), next_offset: null },
    { request_id: "t", clip_id: "1", text: "a" },
    { request_id: "t", clip_id: "1", text: "a", next_offset: 2, extra: 1 },
    { request_id: "t", clip_id: "1", text: "\ud800", next_offset: null },
    { request_id: "t", clip_id: "1", text: "", next_offset: 2 },
    "shape",
  ];
  for (const value of bad) {
    mocks.invoke.mockResolvedValue(value);
    await expect(nativeBridge.getText("t", "1", 1)).rejects.toThrow();
  }
  mocks.invoke.mockRejectedValue({ code: "not_found", message: "clip text not found", retryable: false });
  await expect(nativeBridge.getText("t", "1", 0)).rejects.toThrow();
});
