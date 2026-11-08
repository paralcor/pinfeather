import { describe, expect, it } from "vitest";
import { ManualPanelInput, type PanelIntent } from "./manual-panel-input";
const target = new EventTarget(), other = new EventTarget();
const copy: PanelIntent = { action: "copy", target, context: "1:2:3" };
const event = (key = "Enter", code = "Enter", extra = {}) => ({ key, code, isComposing: false, repeat: false, ...extra });
describe("C independent side-effect receipts", () => {
  it.each(["Enter", " "])("consumes one full fresh %s operation", key => {
    const input = new ManualPanelInput(), e = event(key, key === " " ? "Space" : "Enter");
    input.keyDown(e, copy); expect(input.keyUp(e, copy)).toBe("copy");
    expect(input.keyUp(e, copy)).toBeNull(); expect(input.takeClick(0, copy)).toBe(false);
  });
  it.each(["Enter", "Escape", " ", "ArrowUp", "ArrowDown", "g"].flatMap(key => [false,true].map(endFirst => ({key,endFirst}))))("candidate $key with endFirst=$endFirst cannot authorize a later release", ({key,endFirst}) => {
    const input=new ManualPanelInput(), code=key===" "?"Space":key;
    input.compositionStart(); if(endFirst)input.compositionEnd();
    input.keyDown(event("Process",code,{isComposing:true,keyCode:229}),copy);
    if(!endFirst)input.compositionEnd();
    expect(input.keyUp(event(key,code),copy)).toBeNull(); expect(input.takeClick(0,copy)).toBe(false);
  });
  it.each([{isComposing:true},{key:"Process"},{keyCode:229},{repeat:true},{ctrlKey:true},{altKey:true},{metaKey:true}])("rejects non-independent key metadata %j", flags => {
    const input=new ManualPanelInput(); input.keyDown(event("Enter","Enter",flags),copy);
    expect(input.keyUp(event(),copy)).toBeNull();
  });
  it.each([null,{...copy,target:other},{...copy,context:"1:3:4"},{...copy,action:"hide" as const}])("requires the same target, operation and current identity on release %j", changed => {
    const input=new ManualPanelInput(); input.keyDown(event(),copy); expect(input.keyUp(event(),changed)).toBeNull();
  });
  it.each([event("Escape","Enter"),event("Enter","Other"),event("Enter",""),event("Process","Enter"),event("Enter","Enter",{isComposing:true})])("rejects wrong or IME release %j", release => {
    const input=new ManualPanelInput(); input.keyDown(event(),copy); expect(input.keyUp(release,copy)).toBeNull();
  });
  it("does not discharge a held physical action on a wrong-key same-code release", () => {
    const input=new ManualPanelInput(); input.keyDown(event(),null);
    expect(input.keyUp(event("Escape","Enter"),copy)).toBeNull();
    input.keyDown(event(),copy); expect(input.keyUp(event(),copy)).toBeNull();
    input.keyDown(event(),copy); expect(input.keyUp(event(),copy)).toBe("copy");
  });
  it("keeps other held keys and superseded candidate releases from authorizing copy", () => {
    const input=new ManualPanelInput(); input.keyDown(event("Process","KeyG",{isComposing:true}),null);
    input.keyDown(event(),copy); expect(input.keyUp(event(),copy)).toBeNull();
    input.keyUp(event("g","KeyG"),null); input.keyDown(event(),copy); expect(input.keyUp(event(),copy)).toBe("copy");
  });
  it.each([8,9])("keeps held identities bounded at eight (count %s)", count => {
    const input=new ManualPanelInput();
    for(let n=0;n<count;n++)input.keyDown(event("g",`Key${n}`),null);
    for(let n=0;n<count;n++)input.keyUp(event("g",`Key${n}`),null);
    input.keyDown(event(),copy); expect(input.keyUp(event(),copy)).toBe(count===8?"copy":null);
  });
  it("repeats, invalidation and composition cancel an armed command without granting default clicks", () => {
    for(const cancel of ["repeat","invalidate","composition"]){
      const input=new ManualPanelInput();input.keyDown(event(),copy);
      if(cancel==="repeat")input.keyDown(event("Enter","Enter",{repeat:true}),copy);
      else if(cancel==="composition")input.compositionStart();else input.invalidate();
      expect(input.keyUp(event(),copy)).toBeNull();expect(input.takeClick(0,copy)).toBe(false);
    }
  });
  it.each(["copy","hide"] as const)("requires a complete subsequent primary pointer for %s", action => {
    const input=new ManualPanelInput(), intent={...copy,action};const p={button:0,pointerId:1};
    input.compositionStart();input.pointerDown(p,null);input.pointerUp(p,null);expect(input.takeClick(1,intent)).toBe(false);
    input.pointerDown(p,intent);input.pointerUp(p,intent);expect(input.takeClick(1,intent)).toBe(true);expect(input.takeClick(1,intent)).toBe(false);
  });
  it.each(["no-down","no-up","mismatch","identity","target","right","secondary","cancel","default"])("rejects invalid pointer receipt %s", kind => {
    const input=new ManualPanelInput(), p={button:0,pointerId:1};
    if(kind!=="no-down")input.pointerDown({...p,button:kind==="right"?2:0,isPrimary:kind!=="secondary"},copy);
    if(kind!=="no-up")input.pointerUp({...p,pointerId:kind==="mismatch"?2:1},kind==="identity"?{...copy,context:"other"}:kind==="target"?{...copy,target:other}:copy);
    if(kind==="cancel")input.invalidate();
    expect(input.takeClick(kind==="default"?0:1,copy)).toBe(false);
  });
  it("preserves final composition replacement and ignores a second orphan end", () => {
    const input=new ManualPanelInput();input.compositionStart({value:"前旧后",start:1,end:2});input.compositionUpdate();
    expect(input.compositionEnd("样")).toBe("前样后");expect(input.compositionEnd("过期")).toBeNull();
  });
  it("starts a new composition snapshot rather than restoring old replacement text", () => {
    const input=new ManualPanelInput();input.compositionStart({value:"旧",start:0,end:1});input.compositionStart({value:"新旧",start:1,end:2});
    expect(input.compositionEnd("样")).toBe("新样");
  });
});
