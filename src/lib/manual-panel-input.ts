type KeyIdentity = { key: string; code?: string; keyCode?: number; isComposing?: boolean; repeat?: boolean; ctrlKey?: boolean; altKey?: boolean; metaKey?: boolean };
type CompositionQuery = { value: string; start: number; end: number };
export type PanelAction = "copy" | "hide";
export type PanelIntent = { action: PanelAction; target: EventTarget; context: string };
type Pointer = { button: number; pointerId: number; isPrimary?: boolean };
const modifiers = new Set(["Control", "Alt", "Shift", "Meta"]);
const identity = (event: KeyIdentity) => event.code ? `code:${event.code}` : `key:${event.key}`;
export const isImeKey = (event: KeyIdentity) => !!event.isComposing || event.key === "Process" || event.keyCode === 229;
const same = (a: PanelIntent, b: PanelIntent | null) => !!b && a.action === b.action && a.target === b.target && a.context === b.context;

/** Receipts authorize one explicit operation, never native IME completion. */
export class ManualPanelInput {
  private query: CompositionQuery | null = null;
  private seenIme = false;
  private held = new Map<string, string>();
  private overflow = false;
  private key: (PanelIntent & { key: string; identity: string }) | null = null;
  private pointer: (PanelIntent & { id: number }) | null = null;
  private click: PanelIntent | null = null;
  get imeSeen() { return this.seenIme; }
  invalidate() { this.key = null; this.pointer = null; this.click = null; }
  focusLeft(target: EventTarget) {
    if (this.key?.target === target || this.pointer?.target === target || this.click?.target === target) this.invalidate();
  }
  compositionStart(query?: CompositionQuery) { this.seenIme = true; this.query = query ?? null; this.invalidate(); }
  compositionUpdate(query?: CompositionQuery) { this.seenIme = true; this.query ??= query ?? null; this.invalidate(); }
  compositionEnd(data = "", value?: string) {
    const query = this.query; this.query = null; this.invalidate();
    if (!query) return null;
    return data ? query.value.slice(0, query.start) + data + query.value.slice(query.end) : value ?? null;
  }
  keyDown(event: KeyIdentity, intent: PanelIntent | null, query?: CompositionQuery) {
    this.invalidate();
    const ime = isImeKey(event), id = identity(event), alreadyHeld = this.held.has(id);
    const otherHeld = this.held.size > (alreadyHeld ? 1 : 0);
    if (ime) this.compositionUpdate(query);
    if (!modifiers.has(event.key)) {
      if (!alreadyHeld && this.held.size >= 8) this.overflow = true;
      else if (!alreadyHeld) this.held.set(id, event.key);
    }
    const validKey = intent?.action === "hide" ? event.key === "Escape" || event.key === "Enter" || event.key === " " : event.key === "Enter" || event.key === " ";
    if (intent && validKey && !ime && !event.repeat && !alreadyHeld && !otherHeld && !this.overflow && !event.ctrlKey && !event.altKey && !event.metaKey) this.key = { ...intent, key: event.key, identity: id };
  }
  keyUp(event: KeyIdentity, intent: PanelIntent | null): PanelAction | null {
    const press = this.key; this.key = null; this.click = null;
    const id = identity(event), heldKey = this.held.get(id);
    const actionKey = ({ Enter: "Enter", NumpadEnter: "Enter", Space: " ", Escape: "Escape", ArrowUp: "ArrowUp", ArrowDown: "ArrowDown" } as Record<string, string>)[event.code ?? ""];
    if (heldKey === event.key || event.key === "Process" || (heldKey === "Process" && (actionKey ? event.key === actionKey : event.code?.startsWith("Key") && event.key.length === 1))) this.held.delete(id);
    if (!press || !same(press, intent) || press.key !== event.key || press.identity !== identity(event) || isImeKey(event) || event.ctrlKey || event.altKey || event.metaKey) return null;
    return press.action;
  }
  pointerDown(event: Pointer, intent: PanelIntent | null) {
    this.invalidate();
    if (intent && event.button === 0 && event.isPrimary !== false) this.pointer = { ...intent, id: event.pointerId };
  }
  pointerUp(event: Pointer, intent: PanelIntent | null) {
    const press = this.pointer; this.pointer = null; this.click = null;
    if (press && same(press, intent) && press.id === event.pointerId && event.button === 0 && event.isPrimary !== false) this.click = press;
  }
  takeClick(detail: number, intent: PanelIntent | null) {
    const click = this.click; this.invalidate();
    return detail > 0 && click !== null && same(click, intent);
  }
}
