// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge, ManualPanelControl } from "./lib/desktop";
import type { ResidencyControl, ResidencySnapshot } from "./lib/residency";
import type { StatusResult } from "./lib/ipc";
afterEach(cleanup);
function status(): StatusResult { return { revision: "1", state: "paused", services: {listener:"ready",worker:"not_started",storage:"failed",hotkey:"not_started",target_observer:"not_started"}, capture:{user_enabled:false,admission_open:false,policy_epoch:1}, counters:{observed:"1",captured:"1",persisted:"0",ignored:"0",superseded:"0",failed:"1",overloaded:"0",cancelled:"0"}, queue:{items:1,bytes:4},history:{items:0,payload_bytes:0},data_dir:"D:\\Synthetic",versions:{app:"0.1.0",schema:1,worker_protocol:1},last_paste:null }; }
function fixture() {
  let event: (state: ResidencySnapshot) => void = () => {};
  const control: ResidencyControl = { available:true,snapshot:vi.fn(async()=>({revision:"1",phase:"running" as const,tray_available:true,error:null})),subscribe:vi.fn(async callback=>{event=callback;return()=>{};}),exit:vi.fn(async()=>({revision:"2",phase:"stopping" as const,tray_available:true,error:null})),retrySaving:vi.fn(async()=>{}) };
  const bridge: DesktopBridge = {available:true,status:vi.fn(async()=>status()),list:vi.fn(async request_id=>({request_id,normalized_query:"",summaries:[],next_cursor:null})),getText:vi.fn(),copyText:vi.fn(),setEnabled:vi.fn(async()=>status()),subscribe:vi.fn(async()=>()=>{})};
  const panel: ManualPanelControl = {available:true,snapshot:vi.fn(async()=>({revision:"1",session_id:null,phase:"hidden" as const,hotkey:"ready" as const,error:null})),subscribe:vi.fn(async()=>()=>{}),open:vi.fn(),ready:vi.fn(),hide:vi.fn(),history:()=>bridge};
  return {control,bridge,panel,emit:(state:ResidencySnapshot)=>event(state)};
}
it("m2 coalesces exit clicks, blocks management and retries only retained saving", async () => {
  const f=fixture();let resolve!:(s:ResidencySnapshot)=>void;f.control.exit=vi.fn(()=>new Promise<ResidencySnapshot>(r=>{resolve=r;}));render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control}/>);
  await waitFor(()=>expect((screen.getByRole("button",{name:"退出片羽"}) as HTMLButtonElement).disabled).toBe(false));
  const exit=screen.getByRole("button",{name:"退出片羽"});fireEvent.click(exit);fireEvent.click(exit);expect(f.control.exit).toHaveBeenCalledTimes(1);expect((screen.getByRole("button",{name:"打开快捷面板"}) as HTMLButtonElement).disabled).toBe(true);
  await act(async()=>resolve({revision:"2",phase:"stopping" as const,tray_available:true,error:null}));
  await act(async()=>f.emit({revision:"3",phase:"failed",tray_available:true,error:"storage pending"}));await screen.findByText(/退出未完成，仍有 1 条/);
  fireEvent.click(screen.getByRole("button",{name:"仅重试保存"}));await waitFor(()=>expect(f.control.retrySaving).toHaveBeenCalledTimes(1));expect(f.bridge.setEnabled).not.toHaveBeenCalled();expect(f.panel.open).not.toHaveBeenCalled();expect(f.bridge.copyText).not.toHaveBeenCalled();
});
it("m2 late capability response cannot undo a newer stopping event", async () => {
  const f=fixture();let resolve!:(s:ResidencySnapshot)=>void;f.control.snapshot=vi.fn(()=>new Promise<ResidencySnapshot>(r=>{resolve=r;}));render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control}/>);await waitFor(()=>expect(f.control.snapshot).toHaveBeenCalledTimes(1));
  await act(async()=>f.emit({revision:"5",phase:"stopping" as const,tray_available:true,error:null}));await act(async()=>resolve({revision:"1",phase:"running" as const,tray_available:true,error:null}));await screen.findByText(/正在退出片羽，等待保存/);expect((screen.getByRole("button",{name:"打开快捷面板"}) as HTMLButtonElement).disabled).toBe(true);
});
it("m2 unavailable old-host capability never reports exit success", async () => {
  const f=fixture();f.control.snapshot=vi.fn(async()=>{throw new Error("unknown command");});render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control}/>);await screen.findByText(/当前宿主不支持驻留与退出控制/);expect((screen.getByRole("button",{name:"退出片羽"}) as HTMLButtonElement).disabled).toBe(true);expect(f.control.exit).not.toHaveBeenCalled();
});
