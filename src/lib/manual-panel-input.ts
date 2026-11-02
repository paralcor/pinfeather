type KeyIdentity = { key: string; code?: string };
export type InputCompletion = "ordinary" | "editing" | "pending" | "ready";
type InputAction = "complete" | "edit";
type CompositionQuery = { value: string; start: number; end: number };
const actionCodes = new Map([["Enter", "Enter"], ["Escape", "Escape"], ["Space", " "], ["ArrowUp", "ArrowUp"], ["ArrowDown", "ArrowDown"]]);
function candidateKey(event: KeyIdentity) {
  if (!event.code) return event.key === "Process" ? null : event.key;
  const key = actionCodes.get(event.code);
  return key && (event.key === "Process" || event.key === key) ? key : null;
}
/** Tracks candidate-key releases and fresh pointer gestures without a timer. */
export class ManualPanelInput {
  private completionState: InputCompletion = "ordinary";
  private observedEnd = false;
  private compositionQuery: CompositionQuery | null = null;
  private inputPointer: InputAction | null = null;
  private actionPointerReady = false;
  private inputKey: (KeyIdentity & { action: InputAction }) | null = null;
  private composing = false;
  private awaitingRelease = false;
  private releaseKey: string | null = null;
  private releaseCode: string | null = null;
  private releasedCommitKey: string | null = null;
  private freshPointer = false;
  private pointerCommitsComposition = false;
  private suppressKeyboardClick = false;
  get completion() { return this.completionState; }
  get actionsAllowed() { return this.completionState === "ordinary" || this.completionState === "ready"; }
  private observeIme() {
    this.completionState = "editing";
    this.observedEnd = false;
    this.inputKey = null;
    this.inputPointer = null;
    this.actionPointerReady = false;
  }
  acknowledge() {
    if (this.completionState !== "editing") return;
    this.completionState = this.observedEnd ? "ready" : "pending";
    this.inputKey = null;
    this.freshPointer = false;
    this.pointerCommitsComposition = false;
    this.actionPointerReady = false;
    this.suppressKeyboardClick = true;
    if (this.observedEnd && this.releaseKey === null) this.awaitingRelease = false;
  }
  resumeEditing() {
    if (this.completionState !== "ordinary") this.completionState = "editing";
    this.compositionQuery = null;
    this.inputKey = null;
    this.suppressKeyboardClick = true;
  }
  inputKeyDown(action: InputAction, event: KeyIdentity & { repeat: boolean; isComposing: boolean; keyCode?: number }) {
    if (event.repeat || event.isComposing || event.key === "Process" || event.keyCode === 229 || this.inputKey) return;
    this.inputKey = { key: event.key, code: event.code, action };
  }
  inputKeyUp(event: KeyIdentity & { isComposing: boolean }) {
    const held = this.inputKey;
    if (!held || held.key !== event.key || held.code !== event.code || event.isComposing) return null;
    this.inputKey = null;
    return held.action;
  }
  inputClick(action: InputAction, detail: number) {
    const allowed = detail > 0 && this.inputPointer === action;
    this.inputPointer = null;
    return allowed;
  }
  compositionStart(query?: CompositionQuery) {
    this.observeIme();
    this.compositionQuery = query ?? null;
    this.composing = true;
    this.awaitingRelease = true;
    this.releaseKey = null;
    this.releaseCode = null;
    this.releasedCommitKey = null;
    this.freshPointer = false;
    this.pointerCommitsComposition = false;
    this.suppressKeyboardClick = true;
  }
  compositionUpdate(query?: CompositionQuery) {
    if (!this.composing) this.compositionStart(query);
  }
  compositionEnd(data = "", value?: string) {
    const query = this.compositionQuery;
    this.compositionQuery = null;
    this.composing = false;
    this.awaitingRelease = this.releasedCommitKey === null;
    this.observedEnd = true;
    if (this.completionState === "pending") {
      this.completionState = "ready";
      if (this.releaseKey === null) this.awaitingRelease = false;
    }
    // End data belongs to this episode, even when the final change arrives later.
    if (!query) return null;
    if (!data) return value ?? null;
    return query.value.slice(0, query.start) + data + query.value.slice(query.end);
  }
  keyUp(release: string | (KeyIdentity & { isComposing: boolean })) {
    const event: KeyIdentity & { isComposing?: boolean } = typeof release === "string" ? { key: release } : release;
    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;
    if (!this.composing && event.isComposing) return;
    if (this.releaseKey === null || this.releaseKey !== candidateKey(event) || this.releaseCode !== (event.code || null)) return;
    const terminal = this.releaseKey === "Enter" || this.releaseKey === "Escape";
    if (this.composing && terminal) this.releasedCommitKey = this.releaseKey;
    this.releaseKey = null;
    this.releaseCode = null;
    if (!this.composing && terminal) this.awaitingRelease = false;
  }
  pointerDown(action: InputAction | null = null) {
    this.inputPointer = action;
    this.actionPointerReady = this.actionsAllowed;
    this.freshPointer = !this.composing;
    this.pointerCommitsComposition = this.composing;
    // A pointer begun during composition may itself commit the candidate.
    // Only a new gesture after the end resolves a fence with no held candidate key.
    if (this.freshPointer && this.releaseKey === null) this.awaitingRelease = false;
  }
  blocks(event: KeyIdentity & { keyCode?: number; isComposing: boolean; repeat: boolean }, query?: CompositionQuery) {
    this.freshPointer = false;
    const ime = event.isComposing || event.key === "Process" || event.keyCode === 229;
    if (ime) { if (!this.composing && this.compositionQuery === null) this.compositionQuery = query ?? null; this.observeIme(); this.awaitingRelease = true; this.releaseKey = null; this.releaseCode = null; this.releasedCommitKey = null; this.suppressKeyboardClick = true; }
    if ((this.composing || this.awaitingRelease) && !["Control", "Alt", "Shift", "Meta"].includes(event.key)) {
      // Process marks IME handling; code identifies the physical key it obscures.
      // Only the latest matching terminal key release can complete this fence.
      // Navigation/Space may select or page candidates; their releases prove only themselves.
      this.releasedCommitKey = null;
      if (this.composing || ime || ["Enter", "Escape", "ArrowUp", "ArrowDown", " "].includes(event.key)) {
        this.releaseKey = candidateKey(event) ?? event.key;
        this.releaseCode = event.code || null;
      }
    }
    const blocked = this.composing || this.awaitingRelease || event.repeat;
    if (["Enter", "Escape", " "].includes(event.key)) this.suppressKeyboardClick = blocked;
    return blocked;
  }
  blocksClick(detail = 0) {
    const pointer = detail > 0 && this.freshPointer;
    const commitsComposition = detail > 0 && this.pointerCommitsComposition;
    this.freshPointer = false;
    this.pointerCommitsComposition = false;
    return this.composing || commitsComposition || (detail === 0 && this.suppressKeyboardClick) || (this.awaitingRelease && !pointer);
  }
  blocksActionClick(detail: number) {
    // A late end may enable controls, but cannot authorize an already begun gesture.
    const beganBlocked = detail > 0 && !this.actionPointerReady;
    this.actionPointerReady = false;
    const gestureBlocked = this.blocksClick(detail);
    return !this.actionsAllowed || beganBlocked || gestureBlocked;
  }
}
