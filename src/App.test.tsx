// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge } from "./lib/desktop";
import type { StatusResult } from "./lib/ipc";
import { createHistoryModel } from "./lib/history-store";
afterEach(cleanup);
const status = (enabled = false, revision = "1"): StatusResult => ({ revision, state: enabled ? "listening" : "paused", services: {listener:"ready",worker:"not_started",storage:"ready",hotkey:"not_started",target_observer:"not_started"}, capture:{user_enabled:enabled,admission_open:enabled,policy_epoch:1}, counters:{observed:"1",captured:"1",persisted:"1",ignored:"0",superseded:"0",failed:"0",overloaded:"0",cancelled:"0"},queue:{items:0,bytes:0},history:{items:1,payload_bytes:8},data_dir:"D:\\Synthetic",versions:{app:"0.1.0",schema:1,worker_protocol:1},last_paste:null });
function bridge(): DesktopBridge { return { available:true,status:vi.fn(async()=>status()),list:vi.fn(async id=>({request_id:id,normalized_query:"",summaries:[{id:"1",captured_at_ms:1,kind:"text" as const,preview:"合成历史",format_names:["CF_UNICODETEXT"],omitted_format_count:0,payload_bytes:8,source_process:null,source_confidence:"unknown" as const}],next_cursor:null})),setEnabled:vi.fn(async enabled=>status(enabled,"2")),subscribe:vi.fn(async()=>()=>{}) }; }
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
