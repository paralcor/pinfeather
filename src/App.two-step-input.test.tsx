// @vitest-environment jsdom
// KeyG/g stand for sanitized "other" identities, not recovered native keys.
// This is an App/DOM ordering regression, not native IME or clipboard proof.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
function pointer(button: HTMLElement) { fireEvent.pointerDown(button, { button: 0, pointerId: 1, isPrimary: true }); button.focus(); fireEvent.pointerUp(button, { button: 0, pointerId: 1, isPrimary: true }); fireEvent.click(button, { detail: 1 }); }
function button(name: string) { return screen.getByRole("button", { name }) as HTMLButtonElement; }
function rows() { return screen.getAllByRole("button", { name: "查看正文" }).map(row => row.getAttribute("aria-pressed")); }
function noAction(f: Awaited<ReturnType<typeof mount>>) { expect(rows()).toEqual(["false"]); expect(f.bridge.getText).not.toHaveBeenCalled(); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled(); }
const process = { key: "Process", code: "KeyG", keyCode: 229, isComposing: true };
function recorded(search: HTMLElement) {
  fireEvent.keyDown(search, { ...process, isComposing: false }); fireEvent.compositionStart(search);
  for (let n = 0; n < 3; n++) { fireEvent.keyDown(search, process); fireEvent.keyUp(search, process); fireEvent.keyUp(search, { key: "g", code: "KeyG", isComposing: true }); }
  fireEvent.compositionEnd(search);
}
function browse() { pointer(button("浏览结果")); return screen.getByRole("list", { name: "历史条目" }); }


it.each(["composition","update","Process","229","isComposing"])("C allows readonly browsing after %s without completion and invalidates old selection",async activity=>{
  const f=await mount();const first=browse();await screen.findByText("合成正文");
  if(activity==="composition")fireEvent.compositionStart(f.search);else if(activity==="update")fireEvent.compositionUpdate(f.search);else fireEvent.keyDown(f.search,{key:activity==="Process"?"Process":"g",code:"KeyG",keyCode:activity==="229"?229:0,isComposing:activity==="isComposing"});
  expect(rows()).toEqual(["false"]);expect(f.bridge.copyText).not.toHaveBeenCalled();expect(f.hide).not.toHaveBeenCalled();expect(f.search.readOnly).toBe(false);expect(button("浏览结果").disabled).toBe(false);
  browse();expect(rows()).toEqual(["true"]);expect(f.bridge.copyText).not.toHaveBeenCalled();expect(f.hide).not.toHaveBeenCalled();expect(first).toBeTruthy();
});
it("sanitized event sequence permits only readonly entry, never an implied copy",async()=>{
 const f=await mount();recorded(f.search);browse();await screen.findByText("合成正文");
 fireEvent.click(button("复制纯文本"),{detail:0});expect(f.bridge.copyText).not.toHaveBeenCalled();expect(f.hide).not.toHaveBeenCalled();
});
it.each(["Enter"," "])("C fresh full %s keyboard copy has no keydown/default-click side effect",async key=>{
 const f=await mount();browse();await screen.findByText("合成正文");const target=button("复制纯文本"),code=key===" "?"Space":"Enter";target.focus();
 fireEvent.keyDown(target,{key,code});expect(f.bridge.copyText).not.toHaveBeenCalled();fireEvent.keyUp(target,{key,code});
 await waitFor(()=>expect(f.bridge.copyText).toHaveBeenCalledTimes(1));fireEvent.click(target,{detail:0});expect(f.bridge.copyText).toHaveBeenCalledTimes(1);expect(f.hide).not.toHaveBeenCalled();
});
it.each(["Enter","Space","ArrowUp","ArrowDown"].flatMap(code=>[false,true].map(endFirst=>({code,endFirst}))))("C overlap $code endFirst=$endFirst cannot borrow a candidate release for copy or hide",async({code,endFirst})=>{
 const f=await mount();fireEvent.compositionStart(f.search);if(endFirst)fireEvent.compositionEnd(f.search);
 fireEvent.keyDown(f.search,{key:"Process",code,isComposing:true});if(!endFirst)fireEvent.compositionEnd(f.search);browse();
 const target=button("复制纯文本");target.focus();fireEvent.keyUp(target,{key:"Process",code,isComposing:false});fireEvent.click(target,{detail:0});
 expect(f.bridge.copyText).not.toHaveBeenCalled();expect(f.hide).not.toHaveBeenCalled();
});
it("C Tab focus movement and a pointer's old default click never copy",async()=>{
 const f=await mount();browse();const copy=button("复制纯文本");fireEvent.keyDown(f.search,{key:"Tab",code:"Tab"});fireEvent.keyUp(f.search,{key:"Tab",code:"Tab"});copy.focus();fireEvent.click(copy,{detail:0});
 expect(f.bridge.copyText).not.toHaveBeenCalled();pointer(copy);await waitFor(()=>expect(f.bridge.copyText).toHaveBeenCalledTimes(1));
});
it("C key focus leaving and returning cannot revive a pending copy",async()=>{
 const f=await mount();browse();const copy=button("复制纯文本");copy.focus();fireEvent.keyDown(copy,{key:"Enter",code:"Enter"});
 f.search.focus();copy.focus();fireEvent.keyUp(copy,{key:"Enter",code:"Enter"});expect(f.bridge.copyText).not.toHaveBeenCalled();
});
it("C unmount rejects old controls and leaves a new session untouched",async()=>{
 const old=await mount();browse();const copy=button("复制纯文本");fireEvent.pointerDown(copy,{button:0,pointerId:1});old.unmount();
 const current=await mount();fireEvent.pointerUp(copy,{button:0,pointerId:1});fireEvent.click(copy,{detail:1});noAction(current);
});
