type KeyIdentity = { key: string; code?: string; keyCode?: number; isComposing?: boolean; repeat?: boolean; ctrlKey?: boolean; altKey?: boolean; metaKey?: boolean; shiftKey?: boolean };
type CompositionQuery = { value: string; start: number; end: number };
export const mergeActions = ["copy-merged", "toggle-merge", "merge-remove", "merge-up", "merge-down", "merge-clear"] as const;
type MergeAction = typeof mergeActions[number];
export function isMergeAction(value: string | null | undefined): value is MergeAction {
  return mergeActions.some(action => action === value);
}
export type PanelAction = "copy" | "copy-png" | "favorite" | "hide" | MergeAction;
export type PanelIntent = { action: PanelAction; target: EventTarget; context: string; clipId?: string };
type Pointer = { button: number; pointerId: number; isPrimary?: boolean };
type HeldKey = "key" | "ime-confirmation" | "ended-confirmation";
const modifiers = new Set(["Control", "Alt", "Shift", "Meta"]);
const imeConfirmation = /^(?:Enter|NumpadEnter|Space| |Arrow(?:Up|Down|Left|Right)|(?:Digit|Numpad)?[0-9]|Escape|Tab|Home|End|PageUp|PageDown)$/;
const isConfirmation = (event: KeyIdentity) => imeConfirmation.test(event.key) || imeConfirmation.test(event.code ?? "");
const anonymousConfirmation = "ime:unknown-confirmation";
const identity = (event: KeyIdentity) => event.code ? `code:${event.code}` : `key:${event.key}`;
export const isImeKey = (event: KeyIdentity) => !!event.isComposing || event.key === "Process" || event.keyCode === 229;
const same = (a: PanelIntent, b: PanelIntent | null) => !!b && a.action === b.action && a.target === b.target && a.context === b.context && a.clipId === b.clipId;

/** Receipts authorize one explicit operation, never native IME completion. */
export class ManualPanelInput {
  private query: CompositionQuery | null = null;
  private compositionEnded = false;
  private releasedImeConfirmation = false;
  private seenIme = false;
  private held = new Map<string, HeldKey>();
  private overflow = false;
  private key: (PanelIntent & { key: string; identity: string }) | null = null;
  private pointer: (PanelIntent & { id: number }) | null = null;
  private click: PanelIntent | null = null;
  get imeSeen() { return this.seenIme; }
  get composingSearch() { return this.query !== null; }
  invalidate() { this.key = null; this.pointer = null; this.click = null; }
  focusLeft(target: EventTarget) {
    if (this.key?.target === target || this.pointer?.target === target || this.click?.target === target) this.invalidate();
  }
  compositionStart(query?: CompositionQuery) {
    this.query = query ?? null;
    this.compositionUpdate(query);
  }
  compositionUpdate(query?: CompositionQuery) {
    this.seenIme = true;
    this.compositionEnded = false;
    this.releasedImeConfirmation = false;
    this.query ??= query ?? null;
    this.invalidate();
  }
  compositionEnd(data = "", value?: string) {
    const query = this.query; this.query = null; this.compositionEnded = true; this.invalidate();
    for (const [id, press] of this.held) {
      if (press === "ime-confirmation") this.held.set(id, "ended-confirmation");
    }
    if (!query) return null;
    return data ? query.value.slice(0, query.start) + data + query.value.slice(query.end) : value ?? null;
  }
  keyDown(event: KeyIdentity, intent: PanelIntent | null, query?: CompositionQuery, browseCopy = false): boolean {
    this.invalidate();
    const ime = isImeKey(event);
    const anonymous = !event.code && event.key === "Process";
    const id = anonymous ? anonymousConfirmation : identity(event), alreadyHeld = this.held.has(id);
    const otherHeld = this.held.size > (alreadyHeld ? 1 : 0);
    const unidentified = !event.code && event.key === "Unidentified" && event.keyCode === 229;
    const confirmation = ime && (anonymous || unidentified || isConfirmation(event));
    const lateConfirmation = confirmation && (this.compositionEnded || this.held.get(id) === "ended-confirmation");
    if (ime) {
      this.seenIme = true;
      // A candidate key can arrive after compositionend without starting another query.
      if (!lateConfirmation) this.compositionUpdate(query);
    } else {
      this.compositionEnded = false;
    }
    if (!modifiers.has(event.key)) {
      if (!alreadyHeld && this.held.size >= 8) this.overflow = true;
      else if (lateConfirmation) this.held.set(id, "ended-confirmation");
      else if (!alreadyHeld) this.held.set(id, confirmation ? "ime-confirmation" : "key");
    }
    // A fresh single-copy button press in browse can follow a released IME key
    // before the editor delivers compositionend.
    const freshBrowseCopy = browseCopy && intent?.action === "copy" && this.releasedImeConfirmation;
    const independent = !ime && (this.query === null || freshBrowseCopy) && !event.repeat && !alreadyHeld && !otherHeld && !this.overflow && !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    const validKey = event.key === "Enter" || event.key === " " || (event.key === "Escape" && intent?.action === "hide");
    if (intent && validKey && independent) this.key = { ...intent, key: event.key, identity: id };
    // Focus may move on ArrowDown. Keep tracking the press until keyUp reaches the panel.
    return event.key === "ArrowDown" && independent && this.query === null;
  }
  keyUp(event: KeyIdentity, intent: PanelIntent | null): PanelAction | null {
    const press = this.key; this.key = null; this.click = null;
    const id = identity(event);
    let released = this.held.get(id);
    // A code-less Process press may be released with its physical key.
    if (!this.held.delete(id) && !modifiers.has(event.key) && !event.repeat) {
      released = this.held.get(anonymousConfirmation);
      this.held.delete(anonymousConfirmation);
    }
    if (released === "ime-confirmation") this.releasedImeConfirmation = true;
    if (!press || !same(press, intent) || press.key !== event.key || press.identity !== id || isImeKey(event) || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return null;
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
