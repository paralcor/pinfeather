// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { ClipSummary, GetClipTextResult, ListClipsResult, ListCursor, StatusResult } from "./lib/ipc";
import { createHistoryModel } from "./lib/history-store";
afterEach(cleanup);
const status = (enabled = false, revision = "1"): StatusResult => ({ revision, state: enabled ? "listening" : "paused", services: {listener:"ready",worker:"not_started",storage:"ready",hotkey:"not_started",target_observer:"not_started"}, capture:{user_enabled:enabled,admission_open:enabled,policy_epoch:1}, counters:{observed:"1",captured:"1",persisted:"1",ignored:"0",superseded:"0",failed:"0",overloaded:"0",cancelled:"0"},queue:{items:0,bytes:0},history:{items:1,payload_bytes:8},data_dir:"D:\\Synthetic",versions:{app:"0.1.0",schema:1,worker_protocol:1},last_paste:null });
function bridge(): DesktopBridge { return { available:true,status:vi.fn(async()=>status()),list:vi.fn(async id=>({request_id:id,normalized_query:"",summaries:[{id:"1",captured_at_ms:1,kind:"text" as const,preview:"合成历史",format_names:["CF_UNICODETEXT"],omitted_format_count:0,payload_bytes:8,source_process:null,source_confidence:"unknown" as const}],next_cursor:null})),getText:vi.fn(async(requestId,clipId)=>({request_id:requestId,clip_id:clipId,text:"合成正文",next_offset:null})),setEnabled:vi.fn(async enabled=>status(enabled,"2")),subscribe:vi.fn(async()=>()=>{}) }; }
it("shows desktop requirement without invoking a host in ordinary browsers",()=>{const api=bridge();api.available=false;render(<App bridge={api}/>);expect(screen.getByText(/需要桌面宿主/)).toBeTruthy();expect(api.status).not.toHaveBeenCalled();expect(api.list).not.toHaveBeenCalled();});
it("subscribes before fetching and renders only actual host summaries",async()=>{const api=bridge();render(<App bridge={api}/>);await screen.findByText("合成历史");expect(api.subscribe).toHaveBeenCalledTimes(2);expect(api.list).toHaveBeenCalledTimes(1);expect(screen.getByText("来源未知")).toBeTruthy();});
it("does not optimistically confirm pause before the host acknowledgement",async()=>{const api=bridge();api.status=vi.fn(async()=>status(true));let resolve!:(value:StatusResult)=>void;api.setEnabled=vi.fn(()=>new Promise<StatusResult>(r=>{resolve=r;}));render(<App bridge={api}/>);await screen.findByText("正在收录");fireEvent.click(screen.getByRole("button",{name:"暂停收录"}));expect(screen.getByText("正在收录")).toBeTruthy();resolve(status(false,"2"));await waitFor(()=>expect(screen.getByText("已暂停")).toBeTruthy());});
it("renders contract-valid timestamps outside the browser Date range",async()=>{const api=bridge();const list=api.list;api.list=async(id,cursor)=>{const page=await list(id,cursor);page.summaries[0].captured_at_ms=Number.MAX_SAFE_INTEGER;return page;};render(<App bridge={api}/>);await screen.findByText("合成历史");expect(screen.getByText("时间超出可显示范围")).toBeTruthy();});
it("does not present a failed query as an empty history",async()=>{const api=bridge();api.list=vi.fn(async()=>{throw new Error("private payload must not become a user error");});render(<App bridge={api}/>);await screen.findByText("无法完成操作，请重试。");expect(screen.queryByText(/暂无历史/)).toBeNull();expect(screen.queryByText(/private payload/)).toBeNull();});
it("coalesces notifications into one active list request and one refresh",async()=>{
  const api=bridge();const callbacks:Record<string,()=>void>={};api.subscribe=vi.fn(async(name,cb)=>{callbacks[name]=cb;return()=>{};});let resolve!:()=>void;const list=api.list;
  api.list=vi.fn(list).mockImplementationOnce((id,cursor)=>new Promise(r=>{resolve=()=>{void list(id,cursor).then(r);};}));
  const model=createHistoryModel(api);const stop=model.start();await waitFor(()=>expect(api.list).toHaveBeenCalledTimes(1));for(let i=0;i<100;i++)callbacks.history_changed();expect(api.list).toHaveBeenCalledTimes(1);resolve();await waitFor(()=>expect(api.list).toHaveBeenCalledTimes(2));await waitFor(()=>expect(model.store.getState().loading).toBe(false));stop();
});
it("cleans a subscription that resolves after unmount",async()=>{
  const api=bridge();let resolve!:(off:()=>void)=>void;api.subscribe=vi.fn(()=>new Promise<()=>void>(r=>{resolve=r;}));const model=createHistoryModel(api);const stop=model.start();stop();const off=vi.fn();resolve(off);await waitFor(()=>expect(off).toHaveBeenCalledTimes(1));expect(api.status).not.toHaveBeenCalled();
});
it("an old status response cannot overwrite a confirmed pause",async()=>{
  const api=bridge();let resolve!:(value:StatusResult)=>void;api.status=vi.fn(async()=>status(true)).mockImplementationOnce(async()=>status(true)).mockImplementationOnce(()=>new Promise<StatusResult>(r=>{resolve=r;}));
  render(<App bridge={api}/>);await screen.findByText("正在收录");fireEvent.click(screen.getByRole("button",{name:"刷新"}));await waitFor(()=>expect(api.status).toHaveBeenCalledTimes(2));fireEvent.click(screen.getByRole("button",{name:"暂停收录"}));await screen.findByText("已暂停");resolve(status(true,"1"));await waitFor(()=>expect(api.status).toHaveBeenCalledTimes(3));expect(screen.getByText("已暂停")).toBeTruthy();
});
it("refreshes history from status counts when a history event was missed",async()=>{
  const api=bridge();const callbacks:Record<string,()=>void>={};api.subscribe=vi.fn(async(name,cb)=>{callbacks[name]=cb;return()=>{};});render(<App bridge={api}/>);await screen.findByText("合成历史");
  api.status=vi.fn(async()=>({...status(false,"2"),history:{items:2,payload_bytes:16}}));const list=api.list;api.list=vi.fn(async(id,cursor)=>{const page=await list(id,cursor);page.summaries[0].preview="恢复丢失事件";return page;});callbacks.status_changed();await screen.findByText("恢复丢失事件");
});

// ---- E09.2b search page wiring ----

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function row(id: string, preview: string): ClipSummary {
  return { id, captured_at_ms: 1, kind: "text" as const, preview, format_names: ["CF_UNICODETEXT"], omitted_format_count: 0, payload_bytes: 8, source_process: null, source_confidence: "unknown" as const };
}

function listResult(call: { id: string; query: string | undefined }, previews: string[], idOffset = 0): ListClipsResult {
  return { request_id: call.id, normalized_query: call.query ?? "", summaries: previews.map((preview, index) => row(String(index + 1 + idOffset), preview)), next_cursor: null };
}

function searchBridge(options: { available?: boolean } = {}) {
  const listCalls: Array<{ id: string; cursor: ListCursor | null; query: string | undefined; d: ReturnType<typeof deferred<ListClipsResult>> }> = [];
  const getTextCalls: Array<{ requestId: string; clipId: string; offset: number; d: ReturnType<typeof deferred<GetClipTextResult>> }> = [];
  const listeners: Record<string, () => void> = {};
  const bridge: DesktopBridge = {
    available: options.available ?? true,
    status: vi.fn(async () => status()),
    list: vi.fn((id: string, cursor: ListCursor | null, query?: string) => {
      const d = deferred<ListClipsResult>();
      listCalls.push({ id, cursor, query, d });
      return d.promise;
    }),
    getText: vi.fn((requestId: string, clipId: string, offset: number) => {
      const d = deferred<GetClipTextResult>();
      getTextCalls.push({ requestId, clipId, offset, d });
      return d.promise;
    }),
    setEnabled: vi.fn(async (enabled: boolean) => status(enabled, "2")),
    subscribe: vi.fn(async (name: "history_changed" | "status_changed", callback: () => void) => {
      listeners[name] = callback;
      return () => {};
    }),
  };
  return { bridge, listCalls, getTextCalls, listeners };
}

function textFrame(call: { requestId: string; clipId: string }, text: string, nextOffset: number | null): GetClipTextResult {
  return { request_id: call.requestId, clip_id: call.clipId, text, next_offset: nextOffset };
}

function previewBody(): string | null {
  return document.querySelector(".preview-body")?.textContent ?? null;
}

function buttonNamed(name: string): HTMLButtonElement {
  return screen.getByRole("button", { name }) as HTMLButtonElement;
}

async function mountWithRecent(previews: string[]) {
  const h = searchBridge();
  render(<App bridge={h.bridge} />);
  await waitFor(() => expect(h.listCalls.length).toBe(1));
  h.listCalls[0].d.resolve(listResult(h.listCalls[0], previews));
  await waitFor(() => expect(screen.queryByText("正在读取历史…")).toBeNull());
  return h;
}

it("renders a labeled controlled search input that keeps the raw typed value", async () => {
  await mountWithRecent(["合成历史"]);
  const input = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
  expect(input.value).toBe("");
  fireEvent.change(input, { target: { value: "  剪贴板  " } });
  expect(input.value).toBe("  剪贴板  ");
  fireEvent.change(input, { target: { value: "密".repeat(257) } });
  expect(input.value).toBe("密".repeat(257));
  expect(input.maxLength).toBe(-1);
});

it("sends the normalized query and renders only its matching summaries", async () => {
  const h = await mountWithRecent(["合成历史"]);
  fireEvent.change(screen.getByLabelText("搜索历史正文"), { target: { value: "  中文查询  " } });
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  expect(h.listCalls[1].query).toBe("中文查询");
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["命中摘要"]));
  await screen.findByText("命中摘要");
  expect(screen.queryByText("合成历史")).toBeNull();
});

it("hides the previous rows as soon as the input changes", async () => {
  const h = await mountWithRecent(["合成历史"]);
  fireEvent.change(screen.getByLabelText("搜索历史正文"), { target: { value: "下一句" } });
  expect(screen.queryByText("合成历史")).toBeNull();
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["新摘要"]));
  await screen.findByText("新摘要");
});

it("a late page from a previous query cannot overwrite the newer one", async () => {
  const h = await mountWithRecent(["合成历史"]);
  const input = screen.getByLabelText("搜索历史正文");
  fireEvent.change(input, { target: { value: "Q1" } });
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  fireEvent.change(input, { target: { value: "Q2" } });
  await waitFor(() => expect(h.listCalls.length).toBe(3));
  h.listCalls[2].d.resolve(listResult(h.listCalls[2], ["Q2摘要"]));
  await screen.findByText("Q2摘要");
  await act(async () => { h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["Q1摘要"])); });
  expect(screen.getByText("Q2摘要")).toBeTruthy();
  expect(screen.queryByText("Q1摘要")).toBeNull();
});

it("clearing the search restores the recent history", async () => {
  const h = await mountWithRecent(["合成历史"]);
  const input = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "查找" } });
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["命中摘要"]));
  await screen.findByText("命中摘要");
  fireEvent.click(screen.getByRole("button", { name: "清空搜索" }));
  expect(input.value).toBe("");
  expect(screen.queryByText("命中摘要")).toBeNull();
  await waitFor(() => expect(h.listCalls.length).toBe(3));
  expect(h.listCalls[2].query).toBe("");
  h.listCalls[2].d.resolve(listResult(h.listCalls[2], ["合成历史"]));
  await screen.findByText("合成历史");
});

it("distinguishes an empty recent history from a search with no match", async () => {
  const h = await mountWithRecent([]);
  await screen.findByText(/暂无历史/);
  const input = screen.getByLabelText("搜索历史正文");
  fireEvent.change(input, { target: { value: "无匹配词" } });
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], []));
  await waitFor(() => expect(screen.getByText(/没有匹配/)).toBeTruthy());
  expect(screen.queryByText(/暂无历史/)).toBeNull();
  fireEvent.change(input, { target: { value: "   " } });
  await waitFor(() => expect(h.listCalls.length).toBe(3));
  expect(h.listCalls[2].query).toBe("");
  h.listCalls[2].d.resolve(listResult(h.listCalls[2], []));
  await waitFor(() => expect(screen.getByText(/暂无历史/)).toBeTruthy());
  expect(screen.queryByText(/没有匹配/)).toBeNull();
});

it("a failed query never shows an empty result and never leaks the raw error", async () => {
  const h = await mountWithRecent(["合成历史"]);
  fireEvent.change(screen.getByLabelText("搜索历史正文"), { target: { value: "会失败" } });
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  h.listCalls[1].d.reject(new Error("private payload must not become a user error"));
  await screen.findByText("无法完成操作，请重试。");
  expect(screen.queryByText(/没有匹配/)).toBeNull();
  expect(screen.queryByText(/暂无历史/)).toBeNull();
  expect(screen.queryByText(/private payload/)).toBeNull();
  expect(screen.queryByText(/会失败/)).toBeNull();
});

it("makes no host calls in an ordinary browser", async () => {
  const h = searchBridge({ available: false });
  render(<App bridge={h.bridge} />);
  await screen.findByText(/需要桌面宿主/);
  expect(h.bridge.status).not.toHaveBeenCalled();
  expect(h.bridge.list).not.toHaveBeenCalled();
  expect(screen.queryByLabelText("搜索历史正文")).toBeNull();
});

it("keeps the search input usable while a load is in flight", async () => {
  const h = searchBridge();
  render(<App bridge={h.bridge} />);
  await waitFor(() => expect(h.listCalls.length).toBe(1));
  const input = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
  expect(screen.getByText("正在读取历史…")).toBeTruthy();
  fireEvent.change(input, { target: { value: "加载中输入" } });
  expect(input.value).toBe("加载中输入");
  expect(screen.getByText("正在读取历史…")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "清空搜索" }));
  expect(input.value).toBe("");
  await act(async () => { h.listCalls[0].d.resolve(listResult(h.listCalls[0], ["旧摘要"])); });
  expect(screen.queryByText("旧摘要")).toBeNull();
  await waitFor(() => expect(h.listCalls.length).toBeGreaterThan(1));
  const fresh = h.listCalls[h.listCalls.length - 1];
  expect(fresh.query).toBe("");
  await act(async () => { fresh.d.resolve(listResult(fresh, ["合成历史"])); });
  await screen.findByText("合成历史");
});

it("preserves native composition input and never steals focus or candidate keys", async () => {
  const h = await mountWithRecent(["合成历史"]);
  const input = screen.getByLabelText("搜索历史正文") as HTMLInputElement;
  input.focus();
  fireEvent.compositionStart(input);
  fireEvent.change(input, { target: { value: "拼" } });
  fireEvent.compositionUpdate(input, { data: "pinyin" });
  fireEvent.change(input, { target: { value: "拼音" } });
  fireEvent.compositionEnd(input, { data: "拼音" });
  expect(input.value).toBe("拼音");
  expect(document.activeElement).toBe(input);
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.keyDown(input, { key: "Escape" });
  fireEvent.keyDown(input, { key: "Process", code: "Process" });
  expect(document.activeElement).toBe(input);
  expect(input.value).toBe("拼音");
  await waitFor(() => expect(h.listCalls.length).toBeGreaterThan(1));
  expect(h.listCalls[h.listCalls.length - 1].query).toBe("拼音");
});

it("states that history text search is now available", async () => {
  await mountWithRecent(["合成历史"]);
  expect(screen.queryByText(/搜索、快捷面板与回贴尚未开放/)).toBeNull();
  expect(screen.getByText(/已开放历史正文搜索/)).toBeTruthy();
  expect(screen.getByText(/快捷面板与回贴尚未开放/)).toBeTruthy();
});

// ---- E09.3c body preview UI ----

function openRow(index: number) {
  const buttons = screen.getAllByRole("button", { name: "查看正文" });
  fireEvent.click(buttons[index]);
  return buttons;
}

it("C: does not request body text before an explicit open", async () => {
  const h = await mountWithRecent(["甲", "乙"]);
  expect(h.getTextCalls.length).toBe(0);
  expect(screen.getByText("从列表中选择一条历史以预览正文。")).toBeTruthy();
  expect(document.querySelector(".preview-body")).toBeNull();
});

it("C: opens the preview for the clicked row's real id and switches rows", async () => {
  const h = await mountWithRecent(["甲", "乙"]);
  let buttons = openRow(1);
  expect(h.getTextCalls.length).toBe(1);
  expect(h.getTextCalls[0].clipId).toBe("2");
  expect(h.getTextCalls[0].offset).toBe(0);
  expect(buttons[1].getAttribute("aria-pressed")).toBe("true");
  expect(buttons[0].getAttribute("aria-pressed")).toBe("false");
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "乙正文", null));
  await waitFor(() => expect(previewBody()).toBe("乙正文"));
  buttons = openRow(0);
  expect(h.getTextCalls.length).toBe(2);
  expect(h.getTextCalls[1].clipId).toBe("1");
  expect(h.getTextCalls[1].offset).toBe(0);
  expect(buttons[0].getAttribute("aria-pressed")).toBe("true");
  h.getTextCalls[1].d.resolve(textFrame(h.getTextCalls[1], "甲正文", null));
  await waitFor(() => expect(previewBody()).toBe("甲正文"));
  expect(screen.queryByText("乙正文")).toBeNull();
});

it("C: renders body text as plain text, never as HTML", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  const payload = "<b>bold</b> & <script>alert(1)</script>";
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], payload, null));
  await screen.findByText(/<b>bold<\/b>/);
  expect(document.querySelector("b")).toBeNull();
  expect(document.querySelector("script")).toBeNull();
  expect(previewBody()).toBe(payload);
});

it("C: treats an empty body as a successful empty preview", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "", null));
  await screen.findByText("无可预览正文。");
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText("正在读取正文…")).toBeNull();
});

it("C: blocks duplicate continuations while a frame is loading", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "甲", 3));
  await waitFor(() => expect(previewBody()).toBe("甲"));
  fireEvent.click(screen.getByRole("button", { name: "继续读取正文" }));
  expect(h.getTextCalls.length).toBe(2);
  expect(buttonNamed("继续读取正文").disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "继续读取正文" }));
  expect(h.getTextCalls.length).toBe(2);
  h.getTextCalls[1].d.resolve(textFrame(h.getTextCalls[1], "乙", null));
  await screen.findByText("读取完毕。");
});

it("C: disables row buttons while a list request is in flight", async () => {
  const h = searchBridge();
  render(<App bridge={h.bridge} />);
  await waitFor(() => expect(h.listCalls.length).toBe(1));
  h.listCalls[0].d.resolve({
    ...listResult(h.listCalls[0], ["甲"]),
    next_cursor: { normalized_query: "", upper_id: "9", last_captured_at_ms: 1, last_id: "5" },
  });
  await waitFor(() => expect(screen.getByText("甲")).toBeTruthy());
  fireEvent.click(screen.getByRole("button", { name: "加载更多" }));
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  expect((screen.getAllByRole("button", { name: "查看正文" })[0] as HTMLButtonElement).disabled).toBe(true);
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["丙"], 1));
  await waitFor(() => expect(screen.getByText("丙")).toBeTruthy());
  expect((screen.getAllByRole("button", { name: "查看正文" })[0] as HTMLButtonElement).disabled).toBe(false);
});

it("C: retries a failed first frame from offset 0", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  h.getTextCalls[0].d.reject(new Error("private body must not leak"));
  await screen.findByText("无法完成操作，请重试。");
  expect(screen.queryByText(/private body/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "重试读取正文" }));
  expect(h.getTextCalls.length).toBe(2);
  expect(h.getTextCalls[1].offset).toBe(0);
  h.getTextCalls[1].d.resolve(textFrame(h.getTextCalls[1], "甲正文", null));
  await waitFor(() => expect(previewBody()).toBe("甲正文"));
});

it("C: keeps the read prefix on a failed continuation and retries the same offset", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "甲文", 6));
  await waitFor(() => expect(previewBody()).toBe("甲文"));
  fireEvent.click(screen.getByRole("button", { name: "继续读取正文" }));
  h.getTextCalls[1].d.reject(new Error("boom"));
  await screen.findByText("无法完成操作，请重试。");
  expect(previewBody(), "the read prefix survives a failed continuation").toBe("甲文");
  fireEvent.click(screen.getByRole("button", { name: "重试继续读取正文" }));
  expect(h.getTextCalls.length).toBe(3);
  expect(h.getTextCalls[2].offset).toBe(6);
  h.getTextCalls[2].d.resolve(textFrame(h.getTextCalls[2], "后", 9));
  await waitFor(() => expect(previewBody()).toBe("甲文后"));
});

it("C: continues a Chinese emoji body and stops at EOF without extra calls", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "中文😀", 10));
  await waitFor(() => expect(previewBody()).toBe("中文😀"));
  fireEvent.click(screen.getByRole("button", { name: "继续读取正文" }));
  expect(h.getTextCalls[1].offset).toBe(10);
  h.getTextCalls[1].d.resolve(textFrame(h.getTextCalls[1], "尾", null));
  await screen.findByText("读取完毕。");
  expect(previewBody()).toBe("中文😀尾");
  expect(h.getTextCalls.length).toBe(2);
  expect(screen.queryByRole("button", { name: "继续读取正文" })).toBeNull();
});

it("C: closing, searching and refreshing clear the preview", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "甲文", 6));
  await waitFor(() => expect(previewBody()).toBe("甲文"));
  fireEvent.click(screen.getByRole("button", { name: "关闭正文预览" }));
  expect(screen.getByText("从列表中选择一条历史以预览正文。")).toBeTruthy();
  expect(previewBody()).toBeNull();

  openRow(0);
  h.getTextCalls[1].d.resolve(textFrame(h.getTextCalls[1], "甲文", 6));
  await waitFor(() => expect(previewBody()).toBe("甲文"));
  fireEvent.change(screen.getByLabelText("搜索历史正文"), { target: { value: "换" } });
  expect(screen.getByText("从列表中选择一条历史以预览正文。")).toBeTruthy();
  expect(previewBody()).toBeNull();
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["乙"]));
  await waitFor(() => expect(screen.getByText("乙")).toBeTruthy());

  openRow(0);
  h.getTextCalls[2].d.resolve(textFrame(h.getTextCalls[2], "乙文", 6));
  await waitFor(() => expect(previewBody()).toBe("乙文"));
  fireEvent.click(screen.getByRole("button", { name: "刷新" }));
  expect(screen.getByText("从列表中选择一条历史以预览正文。")).toBeTruthy();
  expect(previewBody()).toBeNull();
});

it("C: a late frame cannot pollute a cleared preview", async () => {
  const h = await mountWithRecent(["甲"]);
  openRow(0);
  const call = h.getTextCalls[0];
  fireEvent.click(screen.getByRole("button", { name: "关闭正文预览" }));
  await act(async () => { call.d.resolve(textFrame(call, "陈旧", null)); });
  expect(previewBody()).toBeNull();
  expect(screen.getByText("从列表中选择一条历史以预览正文。")).toBeTruthy();
});

it("C: history paging and a pure status refresh keep the preview", async () => {
  const h = searchBridge();
  render(<App bridge={h.bridge} />);
  await waitFor(() => expect(h.listCalls.length).toBe(1));
  h.listCalls[0].d.resolve({
    ...listResult(h.listCalls[0], ["甲"]),
    next_cursor: { normalized_query: "", upper_id: "9", last_captured_at_ms: 1, last_id: "5" },
  });
  await waitFor(() => expect(screen.getByText("甲")).toBeTruthy());
  openRow(0);
  h.getTextCalls[0].d.resolve(textFrame(h.getTextCalls[0], "甲文", 6));
  await waitFor(() => expect(previewBody()).toBe("甲文"));
  fireEvent.click(screen.getByRole("button", { name: "加载更多" }));
  await waitFor(() => expect(h.listCalls.length).toBe(2));
  h.listCalls[1].d.resolve(listResult(h.listCalls[1], ["丙"], 1));
  await waitFor(() => expect(screen.getByText("丙")).toBeTruthy());
  expect(previewBody()).toBe("甲文");
  expect(screen.getAllByRole("button", { name: "查看正文" })[0].getAttribute("aria-pressed")).toBe("true");
  h.listeners.status_changed();
  await waitFor(() => expect(h.listCalls.length).toBeGreaterThanOrEqual(2));
  expect(previewBody()).toBe("甲文");
});

it("C: ordinary browsers without a host make no preview calls", async () => {
  const h = searchBridge({ available: false });
  render(<App bridge={h.bridge} />);
  await screen.findByText(/需要桌面宿主/);
  expect(h.getTextCalls.length).toBe(0);
  expect(screen.queryByRole("button", { name: "查看正文" })).toBeNull();
  expect(screen.queryByText("正文预览")).toBeNull();
});
