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

describe("explicit completion after a non-action physical release", () => {
  it.each(["Process", "g"].flatMap(releasedKey => ["up-end-complete", "end-up-complete", "complete-end-up"].map(order => ({ releasedKey, order }))))("accepts only a distinct action after $releasedKey release ($order)", ({ releasedKey, order }) => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true }));
    for (const step of order.split("-")) {
      if (step === "up") input.keyUp({ key: releasedKey, code: "KeyG", isComposing: order.startsWith("up-") });
      else if (step === "end") input.compositionEnd();
      else { input.pointerDown("complete"); expect(input.inputClick("complete", 1)).toBe(true); input.acknowledge(); }
      expect(input.blocksActionClick(0)).toBe(true);
    }
    expect(input.completion).toBe("ready");
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(false);
    expect(input.blocksActionClick(0)).toBe(false);
  });
  it.each(["Process", "g"])("%s release proves only that physical key, never candidate completion", releasedKey => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true }));
    input.keyUp({ key: releasedKey, code: "KeyG", isComposing: true }); input.compositionEnd();
    expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true);
  });
  it.each([
    null,
    { key: "g", code: "KeyH", isComposing: false },
    { key: "g", isComposing: false },
    { key: "Process", code: "", isComposing: false },
    { key: "Escape", code: "KeyG", isComposing: false },
    { key: "Process", code: "KeyG", isComposing: true },
  ])("does not discard an unproven held non-action key on completion: %j", release => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true })); input.compositionEnd();
    if (release) input.keyUp(release);
    input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    expect(input.completion).toBe("ready"); expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(true);
  });
  it.each(["KeyH", "Space", "ArrowDown", "Enter"])("a late KeyG release cannot clear newer %s activity after completion", code => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true }));
    input.blocks(key("Process", { code, isComposing: true })); input.compositionEnd();
    input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    input.keyUp({ key: "g", code: "KeyG", isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(true);
    expect(input.blocksClick(0)).toBe(true);
  });
});

it.each([false, true])("new non-IME activity after end invalidates an older non-action release (has code: %s)", hasCode => {
  const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true })); input.compositionEnd();
  input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
  input.blocks(key("h", hasCode ? { code: "KeyH" } : {}));
  input.keyUp({ key: "g", code: "KeyG", isComposing: false });
  expect(input.blocks(key(" ", { code: "Space" }))).toBe(true);
  expect(input.blocksClick(0)).toBe(true);
});

describe("superseded physical release proof", () => {
  it.each(["Enter", "KeyG", ""].flatMap(heldCode => ["Tab", "KeyH"].flatMap(newCode =>
    ["new-only", "old-only", "new-then-old", "old-then-new"].map(order => ({ heldCode, newCode, order }))
  )))("requires every known held $heldCode and newer $newCode release ($order)", ({ heldCode, newCode, order }) => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: heldCode, isComposing: true })); input.compositionEnd();
    input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    input.blocks(key(newCode === "Tab" ? "Tab" : "h", { code: newCode }));
    const oldRelease = () => input.keyUp({ key: "Process", code: heldCode, isComposing: false });
    const newRelease = () => input.keyUp({ key: newCode === "Tab" ? "Tab" : "h", code: newCode, isComposing: false });
    if (order.startsWith("old")) oldRelease(); else newRelease();
    if (order === "new-then-old") oldRelease();
    if (order === "old-then-new") newRelease();
    const recovered = heldCode !== "" && (order === "new-then-old" || order === "old-then-new");
    // Only R2's rejected complete-known-release expectations change; partial/unknown stays blocked.
    input.pointerDown(); expect(input.blocksActionClick(1)).toBe(false);
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(!recovered); input.keyUp({ key: " ", code: "Space", isComposing: false });
    expect(input.blocksClick(0)).toBe(!recovered);
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(!recovered); input.keyUp({ key: "Escape", code: "Escape", isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(!recovered);
  });
  it.each(["Enter", "KeyG"])("does not taint repeated same physical %s before its matching release", code => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code, isComposing: true }));
    input.blocks(key("Process", { code, isComposing: true, repeat: true })); input.compositionEnd();
    input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    input.keyUp({ key: "Process", code, isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(false);
  });
  it.each(["end-complete", "complete-end", "terminal-end-complete"])("retains superseded release proof across %s", order => {
    const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true }));
    if (order === "complete-end") { input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge(); }
    const terminal = order.startsWith("terminal");
    input.blocks(key(terminal ? "Process" : "h", { code: terminal ? "Enter" : "KeyH", isComposing: terminal }));
    input.keyUp({ key: terminal ? "Process" : "h", code: terminal ? "Enter" : "KeyH", isComposing: terminal });
    if (order === "complete-end") expect(input.completion).toBe("pending");
    input.compositionEnd();
    if (order !== "complete-end") { input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge(); }
    expect(input.completion).toBe("ready");
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(true);
    expect(input.blocksActionClick(0)).toBe(true);
  });
});

describe("bounded physical release ownership", () => {
  it.each([8, 9])("keeps %s distinct held codes bounded and fails closed only on overflow", count => {
    const input = new ManualPanelInput(); input.compositionStart();
    for (let n = 0; n < count; n++) input.blocks(key("Process", { code: `Key${n}`, isComposing: true }));
    input.compositionEnd(); input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    for (let n = 0; n < count; n++) input.keyUp({ key: "Process", code: `Key${n}`, isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(count > 8);
  });
  it("stores at most eight identities under arbitrarily many distinct codes and resets only at a new episode", () => {
    const input = new ManualPanelInput(); input.compositionStart();
    for (let n = 0; n < 1000; n++) input.blocks(key("Process", { code: `Key${n}`, isComposing: true }));
    // A cardinality assertion is needed here: blocking alone cannot prove a memory bound.
    expect(Reflect.get(input, "heldCodes").size).toBe(8);
    input.compositionEnd(); input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    for (let n = 0; n < 1000; n++) input.keyUp({ key: "Process", code: `Key${n}`, isComposing: false });
    expect(input.blocks(key("Enter", { code: "Enter" }))).toBe(true); input.keyUp({ key: "Enter", code: "Enter", isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(true);
    input.compositionStart(); input.blocks(key("Process", { code: "KeyG", isComposing: true }));
    input.keyUp({ key: "g", code: "KeyG", isComposing: true }); input.compositionEnd();
    input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(false);
  });
  it("does not count same-code repeats or previously released distinct codes against the bound", () => {
    const input = new ManualPanelInput(); input.compositionStart();
    for (let n = 0; n < 40; n++) input.blocks(key("Process", { code: "KeyG", isComposing: true, repeat: n > 0 }));
    input.keyUp({ key: "g", code: "KeyG", isComposing: true });
    for (let n = 0; n < 40; n++) {
      input.blocks(key("Process", { code: `Key${n}`, isComposing: true }));
      input.keyUp({ key: "Process", code: `Key${n}`, isComposing: true });
    }
    input.compositionEnd(); input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(false);
  });
  it.each(["Space", "ArrowDown"])("all known releases including %s still need explicit completion", code => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: "KeyG", isComposing: true })); input.blocks(key("Process", { code, isComposing: true }));
    input.keyUp({ key: "g", code: "KeyG", isComposing: true }); input.keyUp({ key: "Process", code, isComposing: true }); input.compositionEnd();
    expect(input.blocks(key("Escape", { code: "Escape" }))).toBe(true); expect(input.blocksActionClick(0)).toBe(true);
  });
  it.each([false, true])("missing-code activity mixed with known identities stays unpaired (missing first: %s)", missingFirst => {
    const input = new ManualPanelInput(); input.compositionStart();
    for (const code of missingFirst ? ["", "KeyG"] : ["KeyG", ""]) input.blocks(key("Process", { code, isComposing: true }));
    input.keyUp({ key: "g", code: "KeyG", isComposing: true }); input.keyUp({ key: "Process", code: "", isComposing: true }); input.compositionEnd();
    input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    input.blocks(key("h", { code: "KeyH" })); input.keyUp({ key: "h", code: "KeyH", isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(true);
  });
});

describe("R4 explicit completion release order", () => {
  const cases = ["Enter", "Escape", "Space", "ArrowUp", "ArrowDown"].flatMap(actionCode =>
    [false, true].flatMap(actionFirst => [false, true].map(actionReleasedFirst => ({ actionCode, actionFirst, actionReleasedFirst })))
  );
  it.each(cases)("restores after all $actionCode/KeyG releases (action down first: $actionFirst, up first: $actionReleasedFirst)", ({ actionCode, actionFirst, actionReleasedFirst }) => {
    const input = new ManualPanelInput(); input.compositionStart();
    for (const code of actionFirst ? [actionCode, "KeyG"] : ["KeyG", actionCode]) input.blocks(key("Process", { code, isComposing: true }));
    input.compositionEnd(); input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    for (const code of actionReleasedFirst ? [actionCode, "KeyG"] : ["KeyG", actionCode]) {
      input.keyUp({ key: "Process", code, isComposing: false });
      expect(input.blocksActionClick(0)).toBe(true);
    }
    expect(input.completion).toBe("ready");
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(false);
    expect(input.blocksActionClick(0)).toBe(false);
  });
  it.each(cases.filter(({ actionCode }) => ["Space", "ArrowUp", "ArrowDown"].includes(actionCode)))("does not infer completion from $actionCode/KeyG releases (action down first: $actionFirst, up first: $actionReleasedFirst)", ({ actionCode, actionFirst, actionReleasedFirst }) => {
    const input = new ManualPanelInput(); input.compositionStart();
    for (const code of actionFirst ? [actionCode, "KeyG"] : ["KeyG", actionCode]) input.blocks(key("Process", { code, isComposing: true }));
    input.compositionEnd();
    for (const code of actionReleasedFirst ? [actionCode, "KeyG"] : ["KeyG", actionCode]) input.keyUp({ key: "Process", code, isComposing: false });
    expect(input.completion).toBe("editing"); expect(input.blocksClick(0)).toBe(true);
    expect(input.blocks(key("Enter", { code: "Enter" }))).toBe(true);
  });
  it.each(["Enter", "Escape", "Space", "ArrowUp", "ArrowDown"].flatMap(actionCode =>
    ["held", "missing-code", "different-code", "contradictory-key", "still-composing"].map(boundary => ({ actionCode, boundary }))
  ))("keeps unproven $actionCode/$boundary blocked despite ready and a matching KeyG release", ({ actionCode, boundary }) => {
    const input = new ManualPanelInput(); input.compositionStart();
    input.blocks(key("Process", { code: actionCode, isComposing: true })); input.blocks(key("Process", { code: "KeyG", isComposing: true }));
    input.compositionEnd(); input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
    input.keyUp({ key: "g", code: "KeyG", isComposing: false });
    if (boundary !== "held") input.keyUp({
      key: boundary === "contradictory-key" ? "g" : "Process",
      code: boundary === "missing-code" ? "" : boundary === "different-code" ? "KeyJ" : actionCode,
      isComposing: boundary === "still-composing",
    });
    input.blocks(key("Tab", { code: "Tab" })); input.keyUp({ key: "Tab", code: "Tab", isComposing: false });
    expect(input.blocks(key(" ", { code: "Space" }))).toBe(true); expect(input.blocksActionClick(0)).toBe(true);
  });
});

it.each([" ", "ArrowUp", "ArrowDown", "g"])("R4 legacy missing-code %s release does not prove physical recovery after completion", name => {
  const input = new ManualPanelInput(); input.compositionStart(); input.blocks(key(name, { isComposing: true })); input.compositionEnd();
  input.pointerDown("complete"); input.inputClick("complete", 1); input.acknowledge();
  input.keyUp({ key: name, isComposing: false });
  expect(input.blocks(key("Enter", { code: "Enter" }))).toBe(true); expect(input.blocksActionClick(0)).toBe(true);
});
