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


it.each(["before-end","after-end","without-end"])("C keeps final visible query and zero writes (%s)",async order=>{
 const f=await mount();fireEvent.compositionStart(f.search);fireEvent.change(f.search,{target:{value:"yang"}});await screen.findByText("两步合成摘要");
 if(order==="before-end")fireEvent.change(f.search,{target:{value:"样"}});
 if(order!=="without-end")fireEvent.compositionEnd(f.search,{data:"样"});else fireEvent.change(f.search,{target:{value:"样"}});
 await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"样"));expect(f.search.value).toBe("样");noAction(f);browse();expect(f.bridge.getText).toHaveBeenCalledTimes(1);
});
it("C final data replaces only the original selection, even after intermediate preview",async()=>{
 const f=await mount();fireEvent.change(f.search,{target:{value:"前旧后"}});await screen.findByText("两步合成摘要");f.search.setSelectionRange(1,2);fireEvent.compositionStart(f.search);fireEvent.change(f.search,{target:{value:"前yang后"}});await screen.findByText("两步合成摘要");browse();
 fireEvent.compositionEnd(f.search,{data:"样"});await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"前样后"));expect(f.search.value).toBe("前样后");expect(rows()).toEqual(["false"]);expect(f.bridge.copyText).not.toHaveBeenCalled();
 fireEvent.compositionEnd(f.search,{data:"过期"});expect(f.search.value).toBe("前样后");
});
it("C Process-only episode retains its original replacement range across repeated IME keys",async()=>{
 const f=await mount();fireEvent.keyDown(f.search,process);fireEvent.change(f.search,{target:{value:"yang"}});await screen.findByText("两步合成摘要");fireEvent.keyDown(f.search,process);fireEvent.compositionEnd(f.search,{data:"样"});
 await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"样"));expect(f.search.value).toBe("样");noAction(f);
});
it("C new start invalidates a copy pointer and uses its own final query snapshot",async()=>{
 const f=await mount();browse();const copy=button("复制纯文本");fireEvent.pointerDown(copy,{button:0,pointerId:1});
 fireEvent.compositionStart(f.search);fireEvent.compositionUpdate(f.search,{data:"新"});fireEvent.compositionEnd(f.search,{data:"新"});fireEvent.pointerUp(copy,{button:0,pointerId:1});fireEvent.click(copy,{detail:1});
 await waitFor(()=>expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String),null,"新"));expect(f.bridge.copyText).not.toHaveBeenCalled();expect(f.hide).not.toHaveBeenCalled();expect(f.search.readOnly).toBe(false);
});
it("C late old component events cannot change a new query or copy permission",async()=>{
 const old=await mount();fireEvent.compositionStart(old.search);old.unmount();const current=await mount();fireEvent.change(current.search,{target:{value:"新会话"}});await screen.findByText("两步合成摘要");
 fireEvent.compositionEnd(old.search,{data:"旧"});fireEvent.change(old.search,{target:{value:"旧"}});expect(current.search.value).toBe("新会话");noAction(current);
});
it("C same React batch cannot browse old rows once final text starts a new query",async()=>{
 const f=await mount();fireEvent.compositionStart(f.search);fireEvent.change(f.search,{target:{value:"yang"}});await screen.findByText("两步合成摘要");const entry=button("浏览结果");
 act(()=>{fireEvent.compositionEnd(f.search,{data:"样"});entry.removeAttribute("disabled");pointer(entry);});
 expect(screen.queryByRole("button",{name:"返回搜索"})).toBeNull();expect(f.bridge.getText).not.toHaveBeenCalled();
 await screen.findByText("两步合成摘要");expect(f.search.value).toBe("样");noAction(f);
});
it("C refuses delayed provisional rows and clears old selection while final results load",async()=>{
 const f=await mount();type Page=Awaited<ReturnType<DesktopBridge["list"]>>;const initial=await vi.mocked(f.bridge.list).mock.results[0].value as Page;const replies=new Map<string,()=>void>();
 vi.mocked(f.bridge.list).mockImplementation((requestId,_cursor,query="")=>new Promise(resolve=>{replies.set(query,()=>resolve({...initial,request_id:requestId,normalized_query:query,summaries:initial.summaries.map(row=>({...row,preview:`${query}结果`}))}));}));
 fireEvent.compositionStart(f.search);fireEvent.change(f.search,{target:{value:"yang"}});await waitFor(()=>expect(replies.has("yang")).toBe(true));fireEvent.compositionEnd(f.search,{data:"样"});
 expect(button("浏览结果").disabled).toBe(true);await waitFor(()=>expect(replies.has("样")).toBe(true));await act(async()=>replies.get("yang")!());expect(screen.queryByText("yang结果")).toBeNull();expect(f.bridge.getText).not.toHaveBeenCalled();
 await act(async()=>replies.get("样")!());await screen.findByText("样结果");noAction(f);browse();expect(f.bridge.getText).toHaveBeenCalledTimes(1);expect(f.bridge.copyText).not.toHaveBeenCalled();
});
