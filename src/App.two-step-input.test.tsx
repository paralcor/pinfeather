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
function recorded(search: HTMLElement) {
  fireEvent.keyDown(search, { ...process, isComposing: false }); fireEvent.compositionStart(search);
  for (let n = 0; n < 3; n++) { fireEvent.keyDown(search, process); fireEvent.keyUp(search, process); fireEvent.keyUp(search, { key: "g", code: "KeyG", isComposing: true }); }
  fireEvent.compositionEnd(search);
}
function browse() { pointer(button("浏览结果")); return screen.getByRole("list", { name: "历史条目" }); }
function complete() { pointer(button("完成输入")); }

it("recorded end-before-pointer: all product surfaces stay inert, completion only locks, distinct Browse selects once", async () => {
  const f = await mount(); recorded(f.search);
  for (const name of ["浏览结果", "查看正文", "隐藏快捷面板", "复制纯文本"]) {
    // Remove disabled for dispatch to exercise the actual capture/handler gates too.
    const target = button(name); target.removeAttribute("disabled"); pointer(target);
    fireEvent.keyDown(target, { key: "Enter", code: "Enter" }); fireEvent.keyUp(target, { key: "Enter", code: "Enter" }); fireEvent.click(target, { detail: 0 });
    noAction(f);
  }
  complete(); noAction(f); expect(f.search.readOnly).toBe(true); expect(document.activeElement).not.toBe(f.search);
  expect(document.activeElement).not.toBe(button("浏览结果"));
  browse(); expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
});

it.each(["before-pointer", "during-pointer", "after-click"])("completion waits for observed end (%s), never replays blocked actions", async order => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  const ack = button("完成输入");
  if (order === "before-pointer") fireEvent.compositionEnd(f.search);
  fireEvent.pointerDown(ack); ack.focus();
  if (order === "during-pointer") fireEvent.compositionEnd(f.search);
  fireEvent.pointerUp(ack); fireEvent.click(ack, { detail: 1 }); noAction(f);
  if (order === "after-click") {
    expect(button("浏览结果").disabled).toBe(true);
    const entry = button("浏览结果"); entry.removeAttribute("disabled"); pointer(entry); noAction(f);
    fireEvent.compositionEnd(f.search); noAction(f);
  }
  expect(f.search.readOnly).toBe(true); expect(button("浏览结果").disabled).toBe(false);
  browse(); expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1);
});

it("no-end remains unconfirmed with a visible editable recovery, and late end cannot replay a click", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); complete();
  expect(f.search.readOnly).toBe(true); expect(button("浏览结果").disabled).toBe(true); noAction(f);
  pointer(button("修改搜索")); expect(f.search.readOnly).toBe(false); expect(document.activeElement).toBe(f.search);
  fireEvent.change(f.search, { target: { value: "继续搜索" } }); await waitFor(() => expect(f.bridge.list).toHaveBeenLastCalledWith(expect.any(String), null, "继续搜索"));
  const entry = button("浏览结果"); entry.removeAttribute("disabled"); pointer(entry);
  entry.setAttribute("disabled", ""); // Restore the rendered disabled attribute after the handler attack.
  fireEvent.compositionEnd(f.search); noAction(f); expect(button("浏览结果").disabled).toBe(true);
  complete(); browse(); expect(rows()).toEqual(["true"]);
});

it.each(["Enter", " "])("keyboard %s completion consumes a full activation and defaults/repeats cannot operate the next control", async key => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  const ack = button("完成输入"); ack.focus(); const code = key === " " ? "Space" : "Enter";
  expect(fireEvent.keyDown(ack, { key, code })).toBe(false); noAction(f);
  fireEvent.keyDown(ack, { key, code, repeat: true }); fireEvent.click(ack, { detail: 0 }); noAction(f);
  fireEvent.keyUp(ack, { key, code }); noAction(f); expect(f.search.readOnly).toBe(true); expect(ack.isConnected).toBe(false);
  for (const target of [button("浏览结果"), button("隐藏快捷面板"), button("复制纯文本")]) {
    target.focus(); fireEvent.keyDown(target, { key, code, repeat: true }); fireEvent.click(target, { detail: 0 }); noAction(f);
  }
  const entry = button("浏览结果"); entry.focus(); const down = fireEvent.keyDown(entry, { key, code }); const up = fireEvent.keyUp(entry, { key, code });
  if (down && up) fireEvent.click(entry, { detail: 0 });
  expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.hide).not.toHaveBeenCalled();
});

it("a default click alone cannot acknowledge, and a held completion key cannot browse after pointer acknowledgement", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  const ack = button("完成输入"); fireEvent.click(ack, { detail: 0 }); expect(f.search.readOnly).toBe(false);
  ack.focus(); fireEvent.keyDown(ack, { key: "Enter", code: "Enter" }); pointer(ack); noAction(f);
  const entry = button("浏览结果"); fireEvent.keyDown(entry, { key: "Enter", code: "Enter", repeat: true }); fireEvent.click(entry, { detail: 0 }); noAction(f);
  fireEvent.keyUp(entry, { key: "Enter", code: "Enter" }); fireEvent.click(entry, { detail: 0 }); noAction(f);
  browse(); expect(rows()).toEqual(["true"]);
});

it.each(["composition", "update", "Process", "229", "isComposing"])("new %s activity invalidates ready and browse; Modify/Return preserve query and clear selection", async activity => {
  const f = await mount(); fireEvent.change(f.search, { target: { value: "样本" } }); await screen.findByText("两步合成摘要");
  fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search); complete(); browse(); await screen.findByText("合成正文");
  const reads = vi.mocked(f.bridge.getText).mock.calls.length;
  if (activity === "composition") fireEvent.compositionStart(f.search);
  else if (activity === "update") fireEvent.compositionUpdate(f.search);
  else fireEvent.keyDown(f.search, { key: activity === "Process" ? "Process" : "g", code: "KeyG", keyCode: activity === "229" ? 229 : 0, isComposing: activity === "isComposing" });
  expect(rows()).toEqual(["false"]); expect(f.search.readOnly).toBe(false); expect(button("浏览结果").disabled).toBe(true);
  pointer(button("隐藏快捷面板")); fireEvent.keyDown(screen.getByRole("list", { name: "历史条目" }), { key: "Escape" });
  expect(f.hide).not.toHaveBeenCalled(); expect(f.bridge.copyText).not.toHaveBeenCalled(); expect(f.bridge.getText).toHaveBeenCalledTimes(reads);
  fireEvent.compositionEnd(f.search); complete(); pointer(button("修改搜索"));
  expect(f.search.value).toBe("样本"); expect(f.search.readOnly).toBe(false); expect(rows()).toEqual(["false"]);
  complete(); browse(); pointer(button("返回搜索")); expect(f.search.value).toBe("样本"); expect(rows()).toEqual(["false"]);
});

it("ordinary ASCII search keeps one Browse activation and explicit copy", async () => {
  const f = await mount(); fireEvent.change(f.search, { target: { value: "ascii" } }); await waitFor(() => expect(f.bridge.list).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("button", { name: "完成输入" })).toBeNull();
  const list = browse(); expect(rows()).toEqual(["true"]); expect(f.bridge.copyText).not.toHaveBeenCalled();
  fireEvent.keyDown(list, { key: "Enter" }); await screen.findByText("已复制，可手动粘贴。"); expect(f.bridge.copyText).toHaveBeenCalledTimes(1); expect(f.hide).not.toHaveBeenCalled();
});

it("unmount rejects old completion controls", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  const ack = button("完成输入"); f.unmount();
  const next = await mount(); fireEvent.click(ack, { detail: 1 }); fireEvent.keyUp(ack, { key: "Enter", code: "Enter" });
  expect(next.search.readOnly).toBe(false); expect(screen.queryByRole("button", { name: "完成输入" })).toBeNull(); noAction(next);
  await act(async () => {});
});

it.each(["Enter", " "])("keyboard %s acknowledgement before end waits, and a later keyboard Browse stays independent", async key => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  const ack = button("完成输入"); ack.focus(); const code = key === " " ? "Space" : "Enter";
  fireEvent.keyDown(ack, { key, code }); fireEvent.keyUp(ack, { key, code });
  expect(button("浏览结果").disabled).toBe(true); noAction(f);
  fireEvent.compositionEnd(f.search); noAction(f);
  const entry = button("浏览结果"); entry.focus(); const down = fireEvent.keyDown(entry, { key, code }); const up = fireEvent.keyUp(entry, { key, code });
  expect(down && up).toBe(true); fireEvent.click(entry, { detail: 0 }); expect(rows()).toEqual(["true"]);
});

it("a new episode cancels a pending acknowledgement, and stale end never approves it", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); complete();
  fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  expect(f.search.readOnly).toBe(false); expect(button("浏览结果").disabled).toBe(true); noAction(f);
  complete(); browse(); expect(rows()).toEqual(["true"]);
});

it.each(["浏览结果", "查看正文", "隐藏快捷面板"].flatMap(name => [false, true].map(start => ({ name, start }))))("a $name pointer begun while waiting cannot become actionable on late end (start observed: $start)", async ({ name, start }) => {
  const f = await mount(); if (start) fireEvent.compositionStart(f.search); else fireEvent.keyDown(f.search, process); complete();
  const target = button(name); fireEvent.pointerDown(target);
  fireEvent.compositionEnd(f.search); fireEvent.pointerUp(target); fireEvent.click(target, { detail: 1 });
  noAction(f); expect(button("浏览结果").disabled).toBe(false);
  browse(); expect(rows()).toEqual(["true"]);
});

it("a completion pointer begun before a new episode cannot acknowledge that new episode", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  const ack = button("完成输入"); fireEvent.pointerDown(ack);
  fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  fireEvent.pointerUp(ack); fireEvent.click(ack, { detail: 1 });
  expect(f.search.readOnly).toBe(false); noAction(f); expect(button("浏览结果").disabled).toBe(true);
  complete(); browse(); expect(rows()).toEqual(["true"]);
});

it("unmatched keyboard release cannot acknowledge, and list commands are gated before acknowledgement", async () => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.compositionEnd(f.search);
  const ack = button("完成输入"); ack.focus(); fireEvent.keyDown(ack, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(ack, { key: " ", code: "Space" }); fireEvent.click(ack, { detail: 0 });
  const list = screen.getByRole("list", { name: "历史条目" });
  for (const key of ["ArrowDown", "ArrowUp", "Enter", "Escape"]) { fireEvent.keyDown(list, { key }); fireEvent.keyUp(list, { key }); noAction(f); }
  expect(f.search.readOnly).toBe(false); complete(); browse(); expect(rows()).toEqual(["true"]);
});

it.each(["Enter", " "])("keyboard %s Modify restores editing without reusing its default click for completion", async key => {
  const f = await mount(); fireEvent.compositionStart(f.search); complete();
  const edit = button("修改搜索"); edit.focus(); const code = key === " " ? "Space" : "Enter";
  fireEvent.keyDown(edit, { key, code }); expect(f.search.readOnly).toBe(true);
  fireEvent.keyUp(edit, { key, code }); expect(f.search.readOnly).toBe(false); expect(document.activeElement).toBe(f.search);
  expect(edit.isConnected).toBe(false); fireEvent.click(edit, { detail: 0 });
  fireEvent.click(button("完成输入"), { detail: 0 }); fireEvent.compositionEnd(f.search);
  expect(f.search.readOnly).toBe(false); expect(button("浏览结果").disabled).toBe(true); noAction(f);
  complete(); browse(); expect(rows()).toEqual(["true"]);
});

// Defaults are synthesized at their native activation phase and only if uncancelled.
function keyboardActivation(target: HTMLElement, key: string, repeat = false) {
  const code = key === " " ? "Space" : "Enter";
  const down = fireEvent.keyDown(target, { key, code, repeat });
  if (key === "Enter" && down) fireEvent.click(target, { detail: 0 });
  const up = fireEvent.keyUp(target, { key, code });
  if (key === " " && down && up) fireEvent.click(target, { detail: 0 });
}

it.each(["Enter", " "].flatMap(key => ["浏览结果", "隐藏快捷面板"].flatMap(name =>
  ["release-end-complete", "end-release-complete", "release-complete-end", "complete-end-release"].map(order => ({ key, name, order }))
)))("P2 first independent $key on $name after matching non-action release ($order)", async ({ key, name, order }) => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.keyDown(f.search, process);
  const release = () => fireEvent.keyUp(f.search, { key: "g", code: "KeyG", isComposing: order.startsWith("release-") });
  const end = () => fireEvent.compositionEnd(f.search);
  for (const step of order.split("-")) {
    if (step === "release") release();
    else if (step === "end") end();
    else complete();
    noAction(f);
  }
  expect(f.search.readOnly).toBe(true); expect(button(name).disabled).toBe(false);
  // Focus represents a separate keyboard gesture; no intervening terminal key/pointer Browse.
  const target = button(name); target.focus(); keyboardActivation(target, key);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  if (name === "浏览结果") {
    expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1); expect(f.hide).not.toHaveBeenCalled();
  } else {
    expect(f.hide).toHaveBeenCalledTimes(1); expect(rows()).toEqual(["false"]); expect(f.bridge.getText).not.toHaveBeenCalled();
  }
});

it.each(["Enter", " "].flatMap(key => ["浏览结果", "隐藏快捷面板"].flatMap(name =>
  ["held", "different-code", "missing-code", "still-composing", "newer-key", "repeat", "newer-ordinary", "newer-ordinary-missing-code"].map(boundary => ({ key, name, boundary }))
)))("P2 safety: $boundary cannot activate $name with $key after completion", async ({ key, name, boundary }) => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.keyDown(f.search, process);
  if (boundary === "newer-key") fireEvent.keyDown(f.search, { ...process, code: "KeyH" });
  fireEvent.compositionEnd(f.search);
  const lateOrdinary = boundary.startsWith("newer-ordinary");
  if (boundary !== "held" && !lateOrdinary) fireEvent.keyUp(f.search, {
    key: "g", code: boundary === "different-code" ? "KeyH" : boundary === "missing-code" ? "" : "KeyG", isComposing: boundary === "still-composing",
  });
  complete(); noAction(f); expect(f.search.readOnly).toBe(true);
  if (lateOrdinary) {
    fireEvent.keyDown(f.search, { key: "h", code: boundary === "newer-ordinary" ? "KeyH" : "" });
    fireEvent.keyUp(f.search, { key: "g", code: "KeyG", isComposing: false }); noAction(f);
  }
  const target = button(name); target.focus(); keyboardActivation(target, key, boundary === "repeat"); noAction(f);
  // An unsolicited default click must remain inert after the blocked activation too.
  fireEvent.click(target, { detail: 0 }); noAction(f);
});

it.each(["Enter", "KeyG", ""].flatMap(heldCode => ["Tab", "h"].flatMap(intervening =>
  ["Enter", " "].flatMap(key => ["浏览结果", "隐藏快捷面板"].map(name => ({ heldCode, intervening, key, name })))
)))("P2 R2 held $heldCode survives $intervening down/up before first $key on $name", async ({ heldCode, intervening, key, name }) => {
  const f = await mount(); fireEvent.compositionStart(f.search); fireEvent.keyDown(f.search, { ...process, code: heldCode });
  fireEvent.compositionEnd(f.search); complete(); noAction(f);
  expect(f.search.readOnly).toBe(true); expect(button(name).disabled).toBe(false);
  // The original physical key has no keyup; another key proves only its own release.
  const code = intervening === "Tab" ? "Tab" : "KeyH";
  fireEvent.keyDown(f.search, { key: intervening, code }); fireEvent.keyUp(f.search, { key: intervening, code }); noAction(f);
  const target = button(name); target.focus(); keyboardActivation(target, key); noAction(f);
  fireEvent.click(target, { detail: 0 }); noAction(f);
});

it.each(["Enter", " "].flatMap(key => ["浏览结果", "隐藏快捷面板"].flatMap(name =>
  ["g-h", "h-g"].flatMap(releases => ["release-end-complete", "end-release-complete", "release-complete-end", "complete-end-release", "first-end-complete-last", "first-complete-end-last"].map(order => ({ key, name, releases, order })))
)))("P2 R3 overlap: complete $releases releases restore first $key on $name ($order)", async ({ key, name, releases, order }) => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  fireEvent.keyDown(f.search, process); fireEvent.keyDown(f.search, { ...process, code: "KeyH" }); noAction(f);
  const release = (letter: string, isComposing: boolean) => fireEvent.keyUp(f.search, { key: letter, code: letter === "g" ? "KeyG" : "KeyH", isComposing });
  for (const step of order.split("-")) {
    if (step === "release") for (const letter of releases.split("-")) {
      release(letter, order.startsWith("release-")); noAction(f);
    }
    else if (step === "first") release(releases.split("-")[0], true);
    else if (step === "last") release(releases.split("-")[1], false);
    else if (step === "end") fireEvent.compositionEnd(f.search);
    else complete();
    noAction(f);
  }
  expect(f.search.readOnly).toBe(true); expect(button(name).disabled).toBe(false);
  const target = button(name); target.focus(); keyboardActivation(target, key);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  if (name === "浏览结果") {
    expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1); expect(f.hide).not.toHaveBeenCalled();
  } else {
    expect(f.hide).toHaveBeenCalledTimes(1); expect(rows()).toEqual(["false"]); expect(f.bridge.getText).not.toHaveBeenCalled();
  }
});

it.each(["Enter", " "].flatMap(key => ["浏览结果", "隐藏快捷面板"].flatMap(name =>
  ["g", "h"].flatMap(held => ["held", "missing-code", "different-code", "still-composing", "contradictory-key"].map(boundary => ({ key, name, held, boundary })))
)))("P2 R3 unproven $held/$boundary cannot borrow Tab release for $key on $name", async ({ key, name, held, boundary }) => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  fireEvent.keyDown(f.search, process); fireEvent.keyDown(f.search, { ...process, code: "KeyH" });
  fireEvent.compositionEnd(f.search); complete(); noAction(f);
  const code = held === "g" ? "KeyG" : "KeyH";
  fireEvent.keyUp(f.search, { key: held === "g" ? "h" : "g", code: held === "g" ? "KeyH" : "KeyG" });
  if (boundary !== "held") fireEvent.keyUp(f.search, {
    key: boundary === "contradictory-key" ? "Escape" : held,
    code: boundary === "missing-code" ? "" : boundary === "different-code" ? "KeyJ" : code,
    isComposing: boundary === "still-composing",
  });
  noAction(f); fireEvent.keyDown(f.search, { key: "Tab", code: "Tab" }); fireEvent.keyUp(f.search, { key: "Tab", code: "Tab" }); noAction(f);
  const target = button(name); target.focus(); keyboardActivation(target, key); noAction(f);
  fireEvent.click(target, { detail: 0 }); noAction(f);
});

// Matching releases after explicit completion must not depend on the last key's category.
const r4OverlapCases = ["Enter", "Escape", "Space", "ArrowUp", "ArrowDown"].flatMap(actionCode =>
  ["g-action", "action-g"].flatMap(presses => ["g-action", "action-g"].flatMap(releases =>
    ["Enter", " "].flatMap(key => ["浏览结果", "隐藏快捷面板"].map(name => ({ actionCode, presses, releases, key, name })))
  ))
);
function r4Codes(order: string, actionCode: string) { return order.split("-").map(part => part === "g" ? "KeyG" : actionCode); }
function r4Down(search: HTMLElement, presses: string, actionCode: string) {
  for (const code of r4Codes(presses, actionCode)) fireEvent.keyDown(search, { ...process, code });
}
function r4Release(search: HTMLElement, code: string) { fireEvent.keyUp(search, { key: code === "KeyG" ? "g" : "Process", code, isComposing: false }); }

it.each(r4OverlapCases)("P2 R4 complete overlap $actionCode down:$presses up:$releases restores first $key on $name", async ({ actionCode, presses, releases, key, name }) => {
  const f = await mount(); fireEvent.compositionStart(f.search); r4Down(f.search, presses, actionCode); noAction(f);
  fireEvent.compositionEnd(f.search); noAction(f); complete(); noAction(f);
  expect(f.search.readOnly).toBe(true); expect(button(name).disabled).toBe(false);
  for (const code of r4Codes(releases, actionCode)) { r4Release(f.search, code); noAction(f); }
  const target = button(name);
  // keyup may discharge the fence, but cannot replay a previously blocked default click.
  fireEvent.click(target, { detail: 0 }); noAction(f);
  target.focus(); keyboardActivation(target, key);
  expect(f.bridge.copyText).not.toHaveBeenCalled();
  if (name === "浏览结果") {
    expect(rows()).toEqual(["true"]); expect(f.bridge.getText).toHaveBeenCalledTimes(1); expect(f.hide).not.toHaveBeenCalled();
  } else {
    expect(f.hide).toHaveBeenCalledTimes(1); expect(rows()).toEqual(["false"]); expect(f.bridge.getText).not.toHaveBeenCalled();
  }
});

it.each(r4OverlapCases)("P2 R4 uncompleted overlap $actionCode down:$presses up:$releases blocks $key on $name", async ({ actionCode, presses, releases, key, name }) => {
  const f = await mount(); fireEvent.compositionStart(f.search); r4Down(f.search, presses, actionCode);
  fireEvent.compositionEnd(f.search);
  for (const code of r4Codes(releases, actionCode)) { r4Release(f.search, code); noAction(f); }
  expect(f.search.readOnly).toBe(false); expect(button(name).disabled).toBe(true);
  // Exercise the capture and handler guards as well as the disabled attribute.
  const target = button(name); target.removeAttribute("disabled"); target.focus();
  keyboardActivation(target, key); noAction(f); fireEvent.click(target, { detail: 0 }); noAction(f);
});

it.each(r4OverlapCases)("P2 R4 partial overlap $actionCode down:$presses up:$releases retains last held key before $key on $name", async ({ actionCode, presses, releases, key, name }) => {
  const f = await mount(); fireEvent.compositionStart(f.search); r4Down(f.search, presses, actionCode);
  fireEvent.compositionEnd(f.search); complete(); noAction(f);
  // Release just the first of the matching pair. The other physical key is still held.
  r4Release(f.search, r4Codes(releases, actionCode)[0]); noAction(f);
  fireEvent.keyDown(f.search, { key: "Tab", code: "Tab" }); fireEvent.keyUp(f.search, { key: "Tab", code: "Tab" }); noAction(f);
  const target = button(name); target.focus(); keyboardActivation(target, key); noAction(f);
  fireEvent.click(target, { detail: 0 }); noAction(f);
});

it.each([" ", "ArrowUp", "ArrowDown", "g"].flatMap(candidate => ["Enter", " "].flatMap(key =>
  ["浏览结果", "隐藏快捷面板"].map(name => ({ candidate, key, name }))
)))("P2 R4 legacy missing-code $candidate release cannot prove physical recovery for $key on $name", async ({ candidate, key, name }) => {
  const f = await mount(); fireEvent.compositionStart(f.search);
  fireEvent.keyDown(f.search, { key: candidate, isComposing: true });
  fireEvent.compositionEnd(f.search); complete(); noAction(f);
  fireEvent.keyUp(f.search, { key: candidate, isComposing: false }); noAction(f);
  const target = button(name); target.focus(); keyboardActivation(target, key); noAction(f);
  fireEvent.click(target, { detail: 0 }); noAction(f);
});
