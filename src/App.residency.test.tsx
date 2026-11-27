// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import type { DesktopBridge, ManualPanelControl } from "./lib/desktop";
import type { ResidencyControl, ResidencySnapshot } from "./lib/residency";
import type { StatusResult } from "./lib/ipc";
afterEach(cleanup);
function status(): StatusResult { return { revision: "1", state: "paused", services: {listener:"ready",worker:"not_started",storage:"failed",hotkey:"not_started",target_observer:"not_started"}, capture:{user_enabled:false,admission_open:false,policy_epoch:1}, counters:{observed:"1",captured:"1",persisted:"0",ignored:"0",superseded:"0",failed:"1",overloaded:"0",cancelled:"0"}, queue:{items:1,bytes:4},history:{items:0,payload_bytes:0, favorites: 0, revision: "0"},data_dir:"D:\\Synthetic",versions:{app:"0.1.0",schema:2,worker_protocol:1},last_paste:null }; }
function fixture() {
  let event: (state: ResidencySnapshot) => void = () => {};
  const control: ResidencyControl = { available:true,snapshot:vi.fn(async()=>({revision:"1",phase:"running" as const,tray_available:true,error:null})),subscribe:vi.fn(async callback=>{event=callback;return()=>{};}),exit:vi.fn(async()=>({revision:"2",phase:"stopping" as const,tray_available:true,error:null})),retrySaving:vi.fn(async()=>{}) };
  const bridge: DesktopBridge = {available:true,status:vi.fn(async()=>status()),list:vi.fn(async request_id=>({request_id,normalized_query:"",summaries:[],next_cursor:null})),getText:vi.fn(),copyText:vi.fn(),setEnabled:vi.fn(async()=>status()),subscribe:vi.fn(async()=>()=>{})};
  const panel: ManualPanelControl = {available:true,snapshot:vi.fn(async()=>({revision:"1",session_id:null,phase:"hidden" as const,hotkey:"ready" as const,error:null})),subscribe:vi.fn(async()=>()=>{}),open:vi.fn(),ready:vi.fn(),hide:vi.fn(),history:()=>bridge};
  return {control,bridge,panel,emit:(state:ResidencySnapshot)=>event(state)};
}

it("keeps common capture controls visible and expands diagnostics when a queue needs attention", async () => {
  const f = fixture();
  const initial = status();
  const ready: StatusResult = {
    ...initial,
    state: "listening",
    services: { ...initial.services, storage: "ready" },
    capture: { ...initial.capture, user_enabled: true, admission_open: true },
    history: { items: 4, payload_bytes: 16, favorites: 0, revision: "0" },
  };
  let current: StatusResult = ready;
  let statusChanged!: () => void;
  f.bridge.status = vi.fn(async () => current);
  f.bridge.subscribe = vi.fn(async (name, receive) => {
    if (name === "status_changed") statusChanged = receive;
    return () => {};
  });
  render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control} />);
  await screen.findByText("正在收录");
  const diagnostics = screen.getByText("收录诊断").closest("details")!;
  expect(diagnostics.open).toBe(false);
  expect(screen.getByText("已存历史").closest("details")).toBeNull();
  expect(screen.getByRole("button", { name: "暂停收录" }).closest("details")).toBeNull();

  current = { ...ready, revision: "2", queue: { items: 2, bytes: 8 } };
  act(() => statusChanged());
  await screen.findByText("2 条 · 1 KiB");
  expect(diagnostics.open).toBe(false);

  current = { ...current, revision: "3", state: "paused", capture: { ...ready.capture, user_enabled: false, admission_open: false } };
  act(() => statusChanged());
  await screen.findByText(/还有 2 条内容待处理/);
  expect(diagnostics.open).toBe(true);

  current = { ...ready, revision: "4" };
  act(() => statusChanged());
  await waitFor(() => expect(diagnostics.open).toBe(false));
});

it.each(["overloaded", "failed"] as const)("opens capture diagnostics for %s with its recovery guidance", async state => {
  const f = fixture();
  const snapshot = status();
  if (state === "overloaded") {
    snapshot.services.storage = "ready";
    snapshot.capture.user_enabled = true;
  }
  f.bridge.status = vi.fn(async () => ({ ...snapshot, state }));
  render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control} />);
  const guidance = await screen.findByText(state === "overloaded" ? /队列处理完后/ : /仍有未保存内容/);
  expect(guidance.closest("details")?.open).toBe(true);
  const captureButton = screen.getByRole("button", { name: state === "overloaded" ? "正在更改…" : "重试保存" }) as HTMLButtonElement;
  expect(captureButton.disabled).toBe(state === "overloaded");
});

it("expands exit diagnostics even when the queue is empty", async () => {
  const f = fixture();
  const initial = status();
  f.bridge.status = vi.fn(async (): Promise<StatusResult> => ({
    ...initial,
    services: { ...initial.services, storage: "ready" },
    queue: { items: 0, bytes: 0 },
  }));
  render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control} />);
  await waitFor(() => expect((screen.getByRole("button", { name: "退出片羽" }) as HTMLButtonElement).disabled).toBe(false));
  act(() => f.emit({ revision: "2", phase: "failed", tray_available: true, error: "合成退出错误" }));
  expect((await screen.findByText("合成退出错误")).closest("details")?.open).toBe(true);
  expect(screen.getByRole("button", { name: "重试退出片羽" })).toBeTruthy();
});

it("reopens collapsed storage diagnostics for a new exit failure", async () => {
  const f = fixture();
  f.bridge.status = vi.fn(async (): Promise<StatusResult> => ({ ...status(), state: "failed" }));
  render(<App bridge={f.bridge} panelControl={f.panel} residencyControl={f.control} />);
  const guidance = await screen.findByText(/仍有未保存内容，可重试保存/);
  const diagnostics = guidance.closest("details")!;
  expect(diagnostics.open).toBe(true);
  diagnostics.open = false;
  fireEvent(diagnostics, new Event("toggle"));
  expect(diagnostics.open).toBe(false);

  const exit = screen.getByRole("button", { name: "退出片羽" }) as HTMLButtonElement;
  await waitFor(() => expect(exit.disabled).toBe(false));
  fireEvent.click(exit);
  await screen.findByText(/正在退出片羽，等待保存/);
  expect(diagnostics.open).toBe(false);
  act(() => f.emit({ revision: "3", phase: "failed", tray_available: true, error: "合成退出错误" }));
  expect(diagnostics.open).toBe(true);
  expect(screen.getByText("合成退出错误").closest("details")).toBe(diagnostics);
  expect(screen.getByRole("button", { name: "仅重试保存" })).toBeTruthy();
});

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
