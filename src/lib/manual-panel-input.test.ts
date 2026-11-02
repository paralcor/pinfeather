import { describe, expect, it } from "vitest";
import { ManualPanelInput } from "./manual-panel-input";
const key = (name: string, extra = {}) => ({ key: name, keyCode: 0, isComposing: false, repeat: false, ...extra });
describe("manual panel physical IME boundary", () => {
  it.each(["Enter", "Escape"].flatMap(code => [false, true].map(releaseFirst => ({ code, releaseFirst }))))("matches Process to physical terminal $code (release before end: $releaseFirst)", ({ code, releaseFirst }) => {
    const input = new ManualPanelInput(); input.compositionStart();
    expect(input.blocks(key("Process", { code, isComposing: true }))).toBe(true);
    if (!releaseFirst) input.compositionEnd();
    input.keyUp({ key: "Process", code, isComposing: releaseFirst });
    if (releaseFirst) input.compositionEnd();
    expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key("ArrowDown", { code: "ArrowDown" }))).toBe(false);
  });
  it.each(["Enter", "Escape"])("blocks end-before-Process/%s and accepts only the next action after matching release", code => {
    const input = new ManualPanelInput(); input.compositionStart(); input.compositionEnd();
    expect(input.blocks(key("Process", { code, isComposing: true }))).toBe(true);
    expect(input.blocksClick(0)).toBe(true);
    input.keyUp({ key: "Process", code, isComposing: false });
    expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key("ArrowDown", { code: "ArrowDown" }))).toBe(false);
  });
  it("accepts Process/Enter with unknown keyCode but never guesses an unknown release code", () => {
    const input = new ManualPanelInput(); input.compositionStart();
    expect(input.blocks({ key: "Process", code: "Enter", isComposing: true, repeat: false })).toBe(true);
    input.compositionEnd(); input.keyUp({ key: "Enter", isComposing: false });
    expect(input.blocksClick(0)).toBe(true);
    input.keyUp({ key: "Enter", code: "Enter", isComposing: false });
    expect(input.blocks(key("ArrowDown", { code: "ArrowDown" }))).toBe(false);
  });
  it.each(["Space", "ArrowDown"].flatMap(code => [false, true].map(releaseFirst => ({ code, releaseFirst }))))("keeps nonterminal Process/$code release conservative (release before end: $releaseFirst)", ({ code, releaseFirst }) => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code, isComposing: true }));
    if (!releaseFirst) input.compositionEnd();
    input.keyUp({ key: "Process", code, isComposing: releaseFirst });
    if (releaseFirst) input.compositionEnd();
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
    expect(input.blocksClick(0)).toBe(true);
  });
  it.each([
    { key: "Process", code: "Escape", isComposing: false },
    { key: "Process", code: "", isComposing: false },
    { key: "Process", code: "Other", isComposing: false },
    { key: "Escape", code: "Enter", isComposing: false },
    { key: "Process", code: "Enter", isComposing: true },
  ])("does not release physical Enter on mismatched, unknown or still-composing release %j", release => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "Enter", isComposing: true })); input.compositionEnd();
    input.keyUp(release);
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
  });
  it.each(["Space", "ArrowDown", "Other"])("does not let a stale Process/Enter release clear newer Process/%s activity", code => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "Enter", isComposing: true }));
    input.blocks(key("Process", { code, isComposing: true })); input.compositionEnd();
    input.keyUp({ key: "Process", code: "Enter", isComposing: false });
    input.keyUp({ key: "Process", code, isComposing: false });
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
  });
  it.each(["Space", "ArrowDown"])("invalidates released Process/Enter on newer code-bearing %s before end", code => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "Enter", isComposing: true }));
    input.keyUp({ key: "Process", code: "Enter", isComposing: true });
    input.blocks(key("Process", { code, isComposing: true }));
    input.keyUp({ key: "Process", code, isComposing: true }); input.compositionEnd();
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
  });
  it("keeps a physical terminal held until its own release despite repeat, modifier release and a fresh pointer", () => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "Enter", isComposing: true })); input.compositionEnd();
    input.pointerDown(); expect(input.blocksClick(1)).toBe(false);
    expect(input.blocks(key("Process", { code: "Enter", repeat: true }))).toBe(true);
    input.keyUp({ key: "Control", code: "ControlLeft", isComposing: false });
    expect(input.blocksClick(0)).toBe(true);
    input.keyUp({ key: "Process", code: "Enter", isComposing: false });
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(false);
  });
  it("invalidates held physical Enter on newer 229 modifier activity", () => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "Enter", isComposing: true }));
    input.blocks(key("Shift", { code: "ShiftLeft", keyCode: 229 })); input.compositionEnd();
    input.keyUp({ key: "Process", code: "Enter", isComposing: false });
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
  });
  it("does not let a fresh pointer discharge an unknown held physical key", () => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "Other", isComposing: true })); input.compositionEnd();
    input.keyUp({ key: "Process", code: "Enter", isComposing: false });
    input.pointerDown(); expect(input.blocksClick(1)).toBe(false);
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
  });
  it.each(["ArrowUp", "ArrowDown"].flatMap(arrow => ["Enter", "Escape"].map(commit => [arrow, commit])))("does not authorize candidate %s navigation release followed by end-before-%s", (arrow, commit) => {
    const input = new ManualPanelInput(); input.compositionStart();
    expect(input.blocks(key(arrow))).toBe(true); input.keyUp(arrow); input.compositionEnd();
    expect(input.blocks(key(commit))).toBe(true); expect(input.blocksClick(0)).toBe(true);
    input.keyUp(commit); expect(input.blocks(key(commit))).toBe(false);
  });
  it.each(["Enter", " "])("keeps candidate %s default click blocked and accepts Tab/Space after observed Enter completion", candidate => {
    const input = new ManualPanelInput(); input.compositionStart();
    expect(input.blocks(key(candidate))).toBe(true); input.keyUp(candidate);
    if (candidate === " ") { expect(input.blocksClick(0)).toBe(true); input.blocks(key("Enter")); input.keyUp("Enter"); }
    input.compositionEnd();
    expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key("Tab"))).toBe(false); input.keyUp("Tab");
    expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key(" "))).toBe(false); input.keyUp(" ");
    expect(input.blocksClick(0)).toBe(false);
  });
  it("uses a fresh pointer boundary without releasing a still-held candidate key", () => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Enter")); input.compositionEnd();
    input.pointerDown(); expect(input.blocksClick(1)).toBe(false);
    expect(input.blocks(key("Enter", { repeat: true }))).toBe(true);
    input.keyUp("Control"); expect(input.blocksClick()).toBe(true);
    input.keyUp("Enter"); expect(input.blocks(key("Escape"))).toBe(false);
  });
  it("blocks the gesture that commits composition and a candidate default click after keyup", () => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Enter")); input.keyUp("Enter");
    input.pointerDown(); input.compositionEnd(); expect(input.blocksClick(1)).toBe(true);
    expect(input.blocksClick(0)).toBe(true);
    input.pointerDown(); expect(input.blocksClick(1)).toBe(false);
    expect(input.blocks(key("Escape"))).toBe(false);
  });
  it.each(["Enter", "Escape"])("accepts the next independent key when candidate %s was released before compositionEnd", name => {
    const input = new ManualPanelInput(); input.compositionStart();
    expect(input.blocks(key(name))).toBe(true); input.keyUp(name); input.compositionEnd();
    expect(input.blocks(key("Escape"))).toBe(false);
    expect(input.blocks(key("Enter"))).toBe(false);
  });
  it.each(["Enter", "Escape"])("suppresses candidate %s even when compositionEnd precedes keydown", name => {
    const input = new ManualPanelInput(); input.compositionStart();
    expect(input.blocks(key(name))).toBe(true); input.compositionEnd();
    expect(input.blocks(key(name))).toBe(true); expect(input.blocksClick()).toBe(true);
    input.keyUp(name); expect(input.blocks(key(name))).toBe(false); expect(input.blocksClick()).toBe(false);
  });
  it.each(["ArrowUp", "ArrowDown", " "])("does not infer terminal candidate release from %s keyup after compositionEnd", name => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key(name)); input.compositionEnd(); input.keyUp(name);
    expect(input.blocksClick(0)).toBe(true); expect(input.blocks(key("Enter"))).toBe(true);
    input.keyUp("Enter"); expect(input.blocks(key("Escape"))).toBe(false);
  });
  it.each(["ArrowDown", " ", "Process", "a"])("invalidates an older released terminal key when newer candidate activity is %s", name => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Enter")); input.keyUp("Enter");
    input.blocks(key(name, name === "Process" ? { keyCode: 229 } : {})); input.keyUp(name); input.compositionEnd();
    input.keyUp("Tab"); expect(input.blocks(key("Escape"))).toBe(true);
    input.keyUp("Escape"); expect(input.blocks(key("Enter"))).toBe(false);
  });
  it("does not reuse a held terminal key across a newer 229 event with a modifier identity", () => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Enter"));
    input.blocks(key("Shift", { keyCode: 229 })); input.keyUp("Shift"); input.keyUp("Enter"); input.compositionEnd();
    expect(input.blocks(key("Escape"))).toBe(true); input.keyUp("Escape"); expect(input.blocks(key("Enter"))).toBe(false);
  });
  it("protects Process/229 and repeat, then accepts a later independent Esc", () => {
    const input = new ManualPanelInput(); expect(input.blocks(key("Process", { keyCode: 229 }))).toBe(true);
    expect(input.blocks(key("Escape"))).toBe(true); input.keyUp("Escape");
    expect(input.blocks(key("Escape"))).toBe(false); expect(input.blocks(key("Enter", { repeat: true }))).toBe(true);
  });
  it("does not unlock while composition is still live", () => { const input = new ManualPanelInput(); input.compositionStart(); input.keyUp("a"); expect(input.blocks(key("Enter"))).toBe(true); input.compositionEnd(); input.keyUp("Enter"); expect(input.blocks(key("Enter"))).toBe(false); });
  it("does not treat a modifier release as the candidate key release", () => { const input = new ManualPanelInput(); input.compositionStart(); input.compositionEnd(); input.keyUp("Control"); expect(input.blocks(key("Enter"))).toBe(true); input.keyUp("Enter"); expect(input.blocks(key("Escape"))).toBe(false); });
});
