import { createStore } from "zustand/vanilla";
import type { ClipSummary, CopyMergedTextResult, FavoriteChange, ListClipsResult, ListCursor, PngPreview, StatusResult } from "./ipc";
import { MAX_MERGE_CLIP_IDS, MAX_PREVIEW_SCALARS, MIN_MERGE_CLIP_IDS, normalizeQuery, parseGetClipTextRequest, parseGetClipTextResult, parseCopyClipTextResult, parseCopyMergedTextResult } from "./ipc";
import { readableError, type DesktopBridge } from "./desktop";
import type { Identity, Receipt } from "./measurement";
type PngPreviewState = { phase: "loading" } | { phase: "ready"; image: PngPreview } | { phase: "failed"; message: string };
type CaptureChange = { action: "start"; afterRevision: string } | { action: "retry" | "pause" };
export interface HistoryState { status: StatusResult | null; summaries: ClipSummary[]; listRevision: number; cursor: ListCursor | null; loading: boolean; changing: CaptureChange | false; error: string | null; hostMissing: boolean; query: string; favoritesOnly: boolean; favoriteBusy: boolean; favoriteMessage: string | null; selectedId: string | null; previewText: string; previewLoading: boolean; previewError: string | null; previewNextOffset: number | null; pngPreview: PngPreviewState | null; copyBusy: "text" | "png" | "merged" | false; copyMessage: string | null; copyAvailable: boolean;
  mergeItems: MergeItem[];
  mergeRevision: number;
  mergeCopyMessage: string | null;
  mergeMissingClipId: string | null;
  mergeError: string | null;
}
type MergeItem = Pick<ClipSummary, "id" | "preview">;
const emptyMerge: Pick<HistoryState, "mergeItems" | "mergeRevision" | "mergeCopyMessage" | "mergeMissingClipId" | "mergeError"> = {
  mergeItems: [], mergeRevision: 0, mergeCopyMessage: null, mergeMissingClipId: null, mergeError: null,
};

const uncertainMergedCopy = "无法确认合并复制结果，剪贴板可能已变化。请自行检查；不会自动重试。";

function mergedCopyMessage(result: CopyMergedTextResult) {
  if (result.outcome === "copied") return `已合并复制 ${result.clip_ids.length} 条正文，可手动粘贴。`;
  if (result.outcome === "unknown") return uncertainMergedCopy;
  switch (result.reason) {
    case "not_found": return result.missing_clip_id
      ? `未执行合并复制：条目 #${result.missing_clip_id} 已不存在，请从已选列表中移出后重试。`
      : "未执行合并复制：有已选条目不存在，请检查已选列表。";
    case "invalid_input": return "未执行合并复制：合并正文超出 2 MiB 上限或包含非纯文本条目。";
    case "storage_unavailable": return "未执行合并复制：历史存储暂不可用。";
    case "busy": return "未执行合并复制：当前操作繁忙，请稍后手动重试。";
    case "timeout": return "未执行合并复制：请求在写入前已到期。";
    case "permission_unknown": return "未执行合并复制：宿主未允许当前面板会话复制。";
    default: return "未执行合并复制：无法开始复制，请稍后手动重试。";
  }
}
export function canPauseWhileStarting({ changing, status }: Pick<HistoryState, "changing" | "status">) {
  return changing !== false && changing.action === "start" && status !== null &&
    status.capture.user_enabled && status.capture.admission_open && status.services.storage !== "failed" &&
    BigInt(status.revision) > BigInt(changing.afterRevision);
}
export function isDrainingCaptureQueue(status: StatusResult | null) {
  return status !== null && status.capture.user_enabled && !status.capture.admission_open &&
    status.queue.items > 0 && status.services.storage === "ready";
}
const QUERY_DEBOUNCE_MS = 50;
const MAX_LIST_CALLS = 2;
const MAX_PREVIEW_CALLS = 2;
export function createHistoryModel(bridge: DesktopBridge) {
  const measurement = bridge.measurement;
  let measuredIdentity: Identity | null = null;
  let measuredStart: Promise<Identity> | null = null;
  let measuredSerial: Promise<unknown> = Promise.resolve();
  let measuredSequence = 0n;
  let committedReceipt: Receipt | null = null;
  let submittedReceipt: Receipt | null = null;
  // Clears synchronously at the initiating event, before debounce/IPC can yield.
  function invalidateMeasurement() {
    committedReceipt = null;
    measuredSequence++;
    if (measurement && measuredStart) {
      const started = measuredStart, gen = String(generation), sequence = String(measuredSequence);
      measuredSerial = measuredSerial.then(async () => measurement.invalidate(await started, gen, sequence));
      void measuredSerial.catch(() => { committedReceipt = null; });
    }
  }
  const store = createStore<HistoryState>(() => ({ status: null, summaries: [], listRevision: 0, cursor: null, loading: false, changing: false, error: null, hostMissing: !bridge.available, query: "", favoritesOnly: false, favoriteBusy: false, favoriteMessage: null, selectedId: null, previewText: "", previewLoading: false, previewError: null, previewNextOffset: null, pngPreview: null, copyBusy: false, copyMessage: null, copyAvailable: bridge.available && typeof bridge.copyText === "function", ...emptyMerge }));
  let active = false, ready = false, lifetime = 0, generation = 0, request = 0;
  let timer: ReturnType<typeof setTimeout> | null = null, wantPage1 = false, statusBusy = false, statusAgain = false;
  let selection = 0, previewRequest = 0;
  let copyRequest = 0, copyPending = false;
  let captureRequest = 0, favoriteRequest = 0;
  let knownHistoryRevision: bigint | null = null;
  // null once the first status response has established the startup baseline.
  let listStartedBeforeFirstStatus: boolean | null = false;
  let listEpoch = 0;
  // Rebuild the loaded range through its previous last key, then honor a pending append.
  let favoriteRefresh: { anchor: ClipSummary | null; resumeAppend: boolean } | null = null;
  let unconfirmedFavorite: { clipId: string; desired: boolean } | null = null;
  // Latest single pending preview need; a selection never owns a queue.
  type PreviewNeed = { clipId: string; offset: number; append: boolean };
  let pendingPreview: PreviewNeed | null = null;
  let pendingPng: string | null = null;
  let pngInFlight = false;
  // Physical unsettled calls; a generation may own at most one of each kind at a time.
  const inflight: Array<{ token: number; gen: number; appendPatches: Map<string, boolean> | null }> = [];
  const previewInflight: Array<{ token: number; sel: number; gen: number }> = [];
  function acceptStatus(status: StatusResult) {
    const current = store.getState().status;
    if (!current || BigInt(status.revision) >= BigInt(current.revision)) store.setState({ status });
  }
  function owns(token: number, gen: number) { return active && token === lifetime && gen === generation; }
  function ownsPreview(token: number, sel: number, gen: number) { return active && token === lifetime && sel === selection && gen === generation; }
  function canDispatch(token: number, gen: number) {
    return inflight.length < MAX_LIST_CALLS && !inflight.some(entry => entry.token === token && entry.gen === gen);
  }
  function releaseSlot(token: number, gen: number) {
    const index = inflight.findIndex(entry => entry.token === token && entry.gen === gen);
    if (index >= 0) inflight.splice(index, 1);
  }
  function releasePreviewSlot(token: number, sel: number, gen: number) {
    const index = previewInflight.findIndex(entry => entry.token === token && entry.sel === sel && entry.gen === gen);
    if (index >= 0) previewInflight.splice(index, 1);
  }
  /** Normalized current query, or null when the raw input must not reach the host. */
  function currentQuery(): string | null {
    const parsed = normalizeQuery(store.getState().query);
    return parsed.ok ? parsed.value : null;
  }
  function clearTimer() { if (timer !== null) { clearTimeout(timer); timer = null; } }
  /** Drops the selection and any pending preview request, and ages out in-flight frames. */
  function clearSelection() {
    selection += 1;
    pendingPreview = null;
    pendingPng = null;
    unconfirmedFavorite = null;
    store.setState({ selectedId: null, previewText: "", previewLoading: false, previewError: null, previewNextOffset: null, pngPreview: null, copyMessage: null, favoriteMessage: null });
  }
  /** A list write is authoritative for the rows: a selection that left them is dropped. */
  function syncSelectionWithRows() {
    const { selectedId, summaries } = store.getState();
    if (selectedId === null) return;
    const selected = summaries.find(item => item.id === selectedId);
    if (!selected) clearSelection();
    else if (unconfirmedFavorite?.clipId === selectedId && selected.favorited === unconfirmedFavorite.desired) {
      unconfirmedFavorite = null;
      store.setState({ favoriteMessage: null });
    }
  }
  function pump() {
    if (!active || !ready || !bridge.available || !wantPage1) return;
    // Single dispatch gate: every path (timer, old-call settle, refresh, status
    // compensation) must honour the pending debounce instead of jumping ahead.
    if (timer !== null) return;
    const normalized = currentQuery();
    if (normalized === null) {
      // An unsendable retained query never loads: end the round instead of
      // leaving `start`'s loading up forever with no future to clear it.
      wantPage1 = false;
      store.setState({ loading: false });
      return;
    }
    const token = lifetime, gen = generation;
    if (!canDispatch(token, gen)) return;
    wantPage1 = false;
    void runList(token, gen, normalized, null, favoriteRefresh);
  }
  function pumpPreview() {
    if (!active || !ready || !bridge.available || pendingPreview === null) return;
    const need = pendingPreview;
    const state = store.getState();
    if (state.selectedId !== need.clipId || !state.summaries.some(item => item.id === need.clipId)) {
      pendingPreview = null;
      return;
    }
    const token = lifetime, sel = selection, gen = generation;
    if (previewInflight.some(entry => entry.token === token && entry.sel === sel && entry.gen === gen)) return;
    if (previewInflight.length >= MAX_PREVIEW_CALLS) return;
    pendingPreview = null;
    void runPreview(token, sel, gen, need.clipId, need.offset, need.append);
  }
  async function pumpPng() {
    const id = pendingPng;
    if (!active || !ready || pngInFlight || !id || !bridge.getPngPreview) return;
    pendingPng = null;
    if (store.getState().selectedId !== id) return;
    pngInFlight = true;
    const token = lifetime, sel = selection, gen = generation;
    try {
      const image = await bridge.getPngPreview(`png-${++previewRequest}`, id);
      if (ownsPreview(token, sel, gen)) store.setState({ pngPreview: { phase: "ready", image } });
    } catch (error) {
      if (ownsPreview(token, sel, gen)) {
        const message = typeof error === "object" && error !== null && "code" in error
          ? error.code === "invalid_input" ? "PNG 已损坏或超出预览限制。"
            : error.code === "not_found" ? "这条历史已不存在，请刷新。"
            : readableError(error)
          : "无法读取 PNG 预览，请重试。";
        store.setState({ pngPreview: { phase: "failed", message } });
      }
    } finally {
      pngInFlight = false;
      void pumpPng();
    }
  }
  async function runList(
    token: number, gen: number, normalized: string, cursor: ListCursor | null,
    reconcile: typeof favoriteRefresh = null,
  ) {
    const epoch = listEpoch;
    const favoritesOnly = store.getState().favoritesOnly;
    const ownsList = () => owns(token, gen) && epoch === listEpoch;
    const appendPatches = cursor ? new Map<string, boolean>() : null;
    inflight.push({ token, gen, appendPatches });
    if (listStartedBeforeFirstStatus === false) listStartedBeforeFirstStatus = true;
    store.setState({ loading: true, error: null });
    if (!cursor) invalidateMeasurement();
    const measuredVersion = measuredSequence;
    const identityPromise = measuredStart;
    const invalidated = measuredSerial;
    let receipt: Receipt | null = null;
    async function fetchPage(next: ListCursor | null): Promise<ListClipsResult> {
      const id = `${next ? "more" : "page1"}-${++request}`;
      let result: ListClipsResult;
      if (measurement && identityPromise) {
        try {
          await invalidated;
          const identity = await identityPromise;
          const measured = await measurement.list(identity, String(gen), String(measuredVersion), id, next, normalized, favoritesOnly);
          if (!next) receipt = measured.receipt;
          result = measured.result;
        } catch (error) {
          if (!["stopped", "expired", "limit", "output", "clock", "stale"].includes(String(error))) throw error;
          result = await bridge.list(id, next, normalized, favoritesOnly);
        }
      } else {
        result = await bridge.list(id, next, normalized, favoritesOnly);
      }
      if (result.request_id !== id || result.normalized_query !== normalized) throw new Error("invalid list correlation");
      return result;
    }
    try {
      const first = await fetchPage(cursor);
      if (!ownsList() || (measurement && measuredVersion !== measuredSequence)) return;
      const summaries = [...first.summaries];
      let next = first.next_cursor;
      const anchor = reconcile?.anchor;
      let resumeAppend = reconcile?.resumeAppend;
      while (next) {
        const beforeAnchor = anchor && (next.last_captured_at_ms > anchor.captured_at_ms ||
          (next.last_captured_at_ms === anchor.captured_at_ms && BigInt(next.last_id) > BigInt(anchor.id)));
        if (!beforeAnchor && !resumeAppend) break;
        if (!beforeAnchor) resumeAppend = false;
        const page = await fetchPage(next);
        if (!ownsList() || (measurement && measuredVersion !== measuredSequence)) return;
        if (page.next_cursor?.last_id === next.last_id) throw new Error("list cursor did not advance");
        summaries.push(...page.summaries);
        next = page.next_cursor;
      }
      if (appendPatches) {
        const current = store.getState().summaries;
        const seen = new Set(current.map(item => item.id));
        const appended = summaries.flatMap(row => {
          const favorited = appendPatches.get(row.id);
          if (seen.has(row.id) || (favoritesOnly && favorited === false)) return [];
          return [favorited === undefined || favorited === row.favorited ? row : { ...row, favorited }];
        });
        store.setState({ summaries: [...current, ...appended], cursor: next });
      } else {
        if (reconcile === favoriteRefresh) favoriteRefresh = null;
        committedReceipt = receipt;
        store.setState(state => ({ summaries, cursor: next, listRevision: state.listRevision + (reconcile ? 0 : 1) }));
      }
      syncSelectionWithRows();
    } catch (error) {
      if (ownsList()) {
        committedReceipt = null;
        if (reconcile === favoriteRefresh) favoriteRefresh = null;
        store.setState({ error: readableError(error) });
      }
    } finally {
      releaseSlot(token, gen);
      if (active) pump();
      if (owns(token, gen) && !inflight.some(entry => entry.token === lifetime && entry.gen === generation)) store.setState({ loading: false });
    }
  }
  async function runPreview(token: number, sel: number, gen: number, clipId: string, offset: number, append: boolean) {
    const requestId = `preview-${++previewRequest}`;
    const request = parseGetClipTextRequest({ request_id: requestId, clip_id: clipId, offset });
    if (!request.ok) {
      if (ownsPreview(token, sel, gen)) store.setState({ previewError: readableError(request.error), previewLoading: false });
      return;
    }
    previewInflight.push({ token, sel, gen });
    try {
      const raw = await bridge.getText(requestId, clipId, offset);
      const parsed = parseGetClipTextResult(raw, request.value);
      if (!parsed.ok) throw new Error("invalid preview frame");
      if (!ownsPreview(token, sel, gen)) return;
      // A continuation only lands while the frontier still points at its offset.
      if (append && store.getState().previewNextOffset !== offset) return;
      const prior = store.getState().previewText;
      store.setState({
        previewText: append ? prior + parsed.value.text : parsed.value.text,
        previewNextOffset: parsed.value.next_offset,
        previewLoading: false,
        previewError: null,
      });
    } catch (error) {
      if (ownsPreview(token, sel, gen)) store.setState({ previewError: readableError(error), previewLoading: false });
    } finally {
      releasePreviewSlot(token, sel, gen);
      if (active) pumpPreview();
      if (ownsPreview(token, sel, gen) && !previewInflight.some(entry => entry.token === lifetime && entry.sel === selection && entry.gen === generation)) {
        store.setState({ previewLoading: false });
      }
    }
  }
  function refreshFavoriteRows() {
    // Retain the anchor and append intent if another refresh supersedes this one.
    favoriteRefresh ??= {
      anchor: store.getState().summaries.at(-1) ?? null,
      resumeAppend: inflight.some(entry => entry.token === lifetime && entry.gen === generation && entry.appendPatches !== null),
    };
    listEpoch++;
    invalidateMeasurement();
    store.setState({ cursor: null });
    wantPage1 = true;
    pump();
  }
  function applyFavoriteChange(change: FavoriteChange & { changed?: boolean }) {
    const revision = BigInt(change.revision);
    if (knownHistoryRevision !== null && revision < knownHistoryRevision) return;
    const advanced = knownHistoryRevision === null || revision > knownHistoryRevision;
    const missed = knownHistoryRevision !== null && revision > knownHistoryRevision + 1n;
    const startupListNeedsRefresh = knownHistoryRevision === null && listStartedBeforeFirstStatus === true;
    if (startupListNeedsRefresh) listStartedBeforeFirstStatus = null;
    knownHistoryRevision = revision;
    const { summaries, favoritesOnly } = store.getState();
    const item = summaries.find(row => row.id === change.clip_id);
    const list = inflight.find(entry => entry.token === lifetime && entry.gen === generation);
    const needsRefresh = missed || startupListNeedsRefresh || change.changed === false ||
      (list && !list.appendPatches) || (favoritesOnly && change.favorited && !item);
    if (advanced && needsRefresh) refreshFavoriteRows();
    else list?.appendPatches?.set(change.clip_id, change.favorited);
    if (item && item.favorited !== change.favorited) {
      const remove = favoritesOnly && !change.favorited;
      store.setState({
        summaries: remove ? summaries.filter(row => row.id !== change.clip_id)
          : summaries.map(row => row.id === change.clip_id ? { ...row, favorited: change.favorited } : row),
      });
      syncSelectionWithRows();
    }
    if (advanced) void refreshStatus();
  }
  async function setFavorite() {
    const state = store.getState();
    const selected = state.summaries.find(row => row.id === state.selectedId);
    if (!active || !ready || !selected || !bridge.setFavorite || state.favoriteBusy || state.loading ||
        state.status?.services.storage !== "ready" || state.status.state === "stopping") return;
    const token = lifetime, sel = selection, gen = generation;
    const clipId = selected.id, desired = !selected.favorited;
    unconfirmedFavorite = null;
    store.setState({ favoriteBusy: true, favoriteMessage: null });
    try {
      const change = await bridge.setFavorite(`favorite-${++favoriteRequest}`, clipId, desired);
      if (active && token === lifetime) applyFavoriteChange(change);
    } catch (error) {
      if (!active || token !== lifetime) return;
      const { summaries, favoritesOnly } = store.getState();
      const current = summaries.find(row => row.id === clipId);
      const removedFromFavorites = !current && state.favoritesOnly && favoritesOnly && gen === generation && !desired;
      if (current?.favorited === desired || removedFromFavorites) return;
      if (typeof error === "object" && error !== null && "code" in error && error.code === "timeout") {
        if (sel === selection) {
          unconfirmedFavorite = { clipId, desired };
          store.setState({ favoriteMessage: "无法确认收藏结果，请核对当前状态。" });
        }
        void refreshStatus();
        refreshFavoriteRows();
      } else if (sel === selection) {
        store.setState({ favoriteMessage: readableError(error) });
        void refreshStatus();
      }
    } finally {
      if (token === lifetime) store.setState({ favoriteBusy: false });
    }
  }
  async function refreshStatus() {
    if (!active || !bridge.available) return;
    if (statusBusy) { statusAgain = true; return; }
    statusBusy = true;
    const token = lifetime;
    try {
      do {
        statusAgain = false;
        const status = await bridge.status();
        if (!active || token !== lifetime) return;
        const previous = store.getState().status;
        if (previous && BigInt(status.revision) < BigInt(previous.revision)) continue;
        const revision = BigInt(status.history.revision);
        const missedRevision = knownHistoryRevision !== null && revision > knownHistoryRevision;
        const startupListNeedsRefresh = listStartedBeforeFirstStatus === true && revision > 0n;
        listStartedBeforeFirstStatus = null;
        if (knownHistoryRevision === null || missedRevision) knownHistoryRevision = revision;
        const missedHistory = previous !== null && (status.history.items !== previous.history.items || status.history.payload_bytes !== previous.history.payload_bytes);
        acceptStatus(status);
        if (missedHistory) {
          favoriteRefresh = null;
          invalidateMeasurement();
          clearSelection();
          store.setState({ cursor: null });
          wantPage1 = true;
          void pump();
        } else if (missedRevision || startupListNeedsRefresh) {
          refreshFavoriteRows();
        }
      } while (statusAgain && active && token === lifetime);
    } catch (error) {
      if (active && token === lifetime) { invalidateMeasurement(); store.setState({ error: readableError(error) }); }
    } finally {
      statusBusy = false;
      if (active && statusAgain) { statusAgain = false; void refreshStatus(); }
    }
  }
  function setQuery(value: string) {
    if (value === store.getState().query) return;
    clearSelection();
    if (!bridge.available) { store.setState({ query: value }); return; }
    generation += 1;
    favoriteRefresh = null;
    invalidateMeasurement();
    clearTimer();
    wantPage1 = false;
    store.setState({ query: value, summaries: [], cursor: null });
    const parsed = normalizeQuery(value);
    if (!parsed.ok) {
      store.setState({ loading: false, error: readableError(parsed.error) });
      return;
    }
    store.setState({ loading: true, error: null });
    timer = setTimeout(() => { timer = null; wantPage1 = true; void pump(); }, QUERY_DEBOUNCE_MS);
  }
  function setFavoritesOnly(favoritesOnly: boolean) {
    if (favoritesOnly === store.getState().favoritesOnly) return;
    generation += 1;
    favoriteRefresh = null;
    invalidateMeasurement();
    clearSelection();
    clearTimer();
    store.setState({ favoritesOnly, summaries: [], cursor: null });
    wantPage1 = true;
    pump();
  }
  async function refresh(history = true): Promise<void> {
    if (!active || !bridge.available) return;
    void refreshStatus();
    if (history) {
      favoriteRefresh = null;
      invalidateMeasurement();
      clearSelection();
      store.setState({ cursor: null });
      wantPage1 = true;
      void pump();
    }
  }
  function selectClip(id: string | null) {
    if (!active || !bridge.available) return;
    if (id === null) { clearSelection(); return; }
    const state = store.getState();
    // While a list is loading the rendered rows are not the current authority.
    if (state.loading) return;
    if (state.selectedId === id) return;
    const selected = state.summaries.find(item => item.id === id);
    if (!selected) return;
    selection += 1;
    unconfirmedFavorite = null;
    const hasPng = selected.format_names.includes("PNG");
    const hasText = !hasPng || selected.format_names.includes("CF_UNICODETEXT");
    pendingPng = hasPng && bridge.getPngPreview ? id : null;
    pendingPreview = hasText ? { clipId: id, offset: 0, append: false } : null;
    store.setState({ selectedId: id, previewText: "", previewLoading: hasText, previewError: null, previewNextOffset: null, pngPreview: pendingPng ? { phase: "loading" } : null, copyMessage: null, favoriteMessage: null });
    void pumpPreview();
    void pumpPng();
  }
  function retryPreview() {
    if (!active || !bridge.available) return;
    const state = store.getState();
    if (state.previewLoading || state.previewError === null || state.selectedId === null) return;
    const id = state.selectedId;
    if (!state.summaries.some(item => item.id === id)) return;
    selection += 1;
    store.setState({ previewText: "", previewLoading: true, previewError: null, previewNextOffset: null });
    pendingPreview = { clipId: id, offset: 0, append: false };
    if (state.pngPreview?.phase === "loading") pendingPng = id;
    void pumpPreview();
    void pumpPng();
  }
  function retryPng() {
    const { selectedId, pngPreview } = store.getState();
    if (!active || !ready || !selectedId || !bridge.getPngPreview || pngPreview?.phase !== "failed") return;
    pendingPng = selectedId;
    store.setState({ pngPreview: { phase: "loading" } });
    void pumpPng();
  }
  /** Explicit next frame only: never loops, and never reuses another selection's offset. */
  function morePreview() {
    if (!active || !ready || !bridge.available) return;
    const state = store.getState();
    if (state.loading || state.previewLoading || state.selectedId === null || state.previewNextOffset === null) return;
    const id = state.selectedId;
    if (!state.summaries.some(item => item.id === id)) return;
    store.setState({ previewLoading: true, previewError: null });
    pendingPreview = { clipId: id, offset: state.previewNextOffset, append: true };
    void pumpPreview();
  }
  function start() {
    if (!bridge.available) return () => {};
    active = true;
    const token = ++lifetime;
    if (measurement) {
      committedReceipt = null; submittedReceipt = null;
      measuredStart = measurement.start();
      const started = measuredStart;
      measuredSerial = started.then(identity => { if (active && token === lifetime) measuredIdentity = identity; });
      void measuredSerial.catch(() => { committedReceipt = null; });
    }
    const disposers: Array<() => void> = [];
    store.setState({ loading: true, changing: false, favoriteBusy: false });
    void (async () => {
      try {
        if (bridge.subscribeFavorite) {
          const off = await bridge.subscribeFavorite(change => {
            if (active && token === lifetime) applyFavoriteChange(change);
          });
          if (!active || token !== lifetime) { off(); return; }
          disposers.push(off);
        }
        for (const name of ["history_changed", "status_changed"] as const) {
          const off = await bridge.subscribe(name, () => { if (active && token === lifetime) void refresh(name === "history_changed"); });
          if (!active || token !== lifetime) { off(); return; }
          disposers.push(off);
        }
        ready = true;
        void refreshStatus();
        wantPage1 = true;
        void pump();
      } catch (error) {
        disposers.splice(0).forEach(off => off());
        if (active && token === lifetime) store.setState({ error: readableError(error), loading: false });
      }
    })();
    return () => {
      clearTimer();
      ready = false;
      if (token === lifetime) {
        committedReceipt = null;
        const started = measuredStart;
        measuredStart = null; measuredIdentity = null;
        if (measurement && started) void started.then(identity => measurement.stop(identity)).catch(() => {});
        active = false;
        favoriteRefresh = null;
        knownHistoryRevision = null;
        listStartedBeforeFirstStatus = false;
        lifetime++;
        store.setState(emptyMerge);
        clearSelection();
      }
      disposers.splice(0).forEach(off => off());
    };
  }
  async function more() {
    const state = store.getState();
    if (!active || !ready || !bridge.available || state.loading || !state.cursor) return;
    const normalized = currentQuery();
    if (normalized === null) return;
    const token = lifetime, gen = generation;
    if (!canDispatch(token, gen)) return;
    void runList(token, gen, normalized, state.cursor);
  }
  async function toggle() {
    const current = store.getState();
    if (!active || current.changing || !current.status || isDrainingCaptureQueue(current.status)) return;
    const retry = current.status.services.storage === "failed" && current.status.queue.items > 0;
    const enabled = retry || current.status.state !== "listening";
    return setEnabled(enabled);
  }
  async function setEnabled(enabled: boolean) {
    const current = store.getState();
    if (!active || !current.status || (!enabled && isDrainingCaptureQueue(current.status)) ||
        (current.changing && (enabled || !canPauseWhileStarting(current)))) return;
    const token = lifetime, request = ++captureRequest;
    const ownsRequest = () => active && token === lifetime && request === captureRequest;
    const changing: CaptureChange = !enabled ? { action: "pause" }
      : current.status.services.storage === "failed" ? { action: "retry" }
      : { action: "start", afterRevision: current.status.revision };
    store.setState({ changing, error: null });
    try {
      const status = await bridge.setEnabled(enabled);
      if (ownsRequest()) { acceptStatus(status); void refresh(false); }
    } catch (error) {
      if (ownsRequest()) store.setState({ error: readableError(error) });
    } finally {
      if (ownsRequest()) store.setState({ changing: false });
    }
  }
  function updateMerge(mergeItems: MergeItem[]) {
    store.setState(state => ({
      mergeItems, mergeRevision: state.mergeRevision + 1, mergeError: null, mergeCopyMessage: null,
      mergeMissingClipId: mergeItems.some(item => item.id === state.mergeMissingClipId) ? state.mergeMissingClipId : null,
    }));
  }

  function toggleMergeItem(item: ClipSummary) {
    if (!active) return;
    if (!item.format_names.includes("CF_UNICODETEXT")) {
      store.setState({ mergeError: "只有含纯文本正文的条目可以加入合并。" });
      return;
    }
    const { mergeItems } = store.getState();
    if (mergeItems.some(row => row.id === item.id)) {
      removeMergeItem(item.id);
    } else if (mergeItems.length >= MAX_MERGE_CLIP_IDS) {
      store.setState({ mergeError: `最多选择 ${MAX_MERGE_CLIP_IDS} 条正文，请移出一条后再选。` });
    } else {
      updateMerge([...mergeItems, { id: item.id, preview: Array.from(item.preview).slice(0, MAX_PREVIEW_SCALARS).join("") }]);
    }
  }

  function removeMergeItem(id: string) {
    const { mergeItems } = store.getState();
    if (active && mergeItems.some(item => item.id === id)) updateMerge(mergeItems.filter(item => item.id !== id));
  }

  function moveMergeItem(id: string, delta: -1 | 1) {
    const { mergeItems } = store.getState();
    const from = mergeItems.findIndex(item => item.id === id);
    const to = from + delta;
    if (!active || from < 0 || to < 0 || to >= mergeItems.length) return;
    const items = [...mergeItems];
    [items[from], items[to]] = [items[to], items[from]];
    updateMerge(items);
  }

  function clearMergeItems() {
    if (active && store.getState().mergeItems.length) updateMerge([]);
  }

  function mergeIdentity() {
    const { mergeItems, mergeRevision } = store.getState();
    return [lifetime, "merge", mergeRevision, mergeItems.map(item => item.id).join(",")].join(":");
  }

  function canCopyMergedText() {
    const { mergeItems, status } = store.getState();
    return active && ready && bridge.available && !!bridge.copyMergedText && !copyPending &&
      mergeItems.length >= MIN_MERGE_CLIP_IDS && mergeItems.length <= MAX_MERGE_CLIP_IDS &&
      status?.services.storage === "ready" && status.state !== "stopping";
  }

  async function copyMergedText() {
    if (!bridge.copyMergedText || !canCopyMergedText()) return;
    const token = lifetime, sequence = ++copyRequest;
    const request = { request_id: `copy-${sequence}`, clip_ids: store.getState().mergeItems.map(item => item.id) };
    copyPending = true;
    store.setState({ copyBusy: "merged", mergeCopyMessage: null, mergeMissingClipId: null });
    let message = uncertainMergedCopy;
    let mayHaveWritten = true;
    let missingClipId: string | null = null;
    try {
      const result = parseCopyMergedTextResult(await bridge.copyMergedText(request.request_id, [...request.clip_ids]), request);
      if (result.ok) {
        message = mergedCopyMessage(result.value);
        mayHaveWritten = result.value.outcome !== "not_written";
        missingClipId = result.value.missing_clip_id;
      }
    } catch { /* A transport failure cannot confirm the write. */ }
    finally {
      copyPending = false;
      if (active && token === lifetime && sequence === copyRequest) {
        store.setState(state => ({
          copyBusy: false,
          mergeCopyMessage: message,
          mergeMissingClipId: state.mergeItems.some(item => item.id === missingClipId) ? missingClipId : null,
          ...(mayHaveWritten ? { copyMessage: null } : {}),
        }));
      } else store.setState({ copyBusy: false });
    }
  }

  async function copy(format: "text" | "png"): Promise<boolean> {
    const state = store.getState();
    const selected = state.summaries.find(item => item.id === state.selectedId);
    const write = format === "png" ? bridge.copyPng : bridge.copyText;
    if (!active || !ready || !bridge.available || !write || copyPending || state.loading || !state.status || state.status.state === "stopping" || state.status.services.storage !== "ready") return false;
    if (!selected?.format_names.includes(format === "png" ? "PNG" : "CF_UNICODETEXT")) return false;
    if (format === "png" && state.pngPreview?.phase !== "ready") return false;
    const token = lifetime, sel = selection, gen = generation;
    const request = { request_id: `copy-${++copyRequest}`, clip_id: selected.id };
    copyPending = true;
    store.setState({ copyBusy: format, copyMessage: null });
    let copied = false;
    let mayHaveWritten = true;
    let message = "无法确认复制结果，剪贴板可能已变化。请自行检查；不会自动重试。";
    try {
      const parsed = parseCopyClipTextResult(await write.call(bridge, request.request_id, request.clip_id), request);
      if (parsed.ok) {
        const result = parsed.value;
        if (result.outcome === "copied") {
          copied = true;
          message = format === "png" ? "图片已复制，可手动粘贴。" : "已复制，可手动粘贴。";
        }
        if (result.outcome === "not_written") {
          mayHaveWritten = false;
          const reasons: Partial<Record<NonNullable<typeof result.reason>, string>> = {
            busy: "当前操作繁忙，请稍后手动重试。", not_found: "历史条目已不存在。", invalid_input: format === "png" ? "PNG 已损坏或超出复制限制。" : "此条目没有可复制的纯文本。", timeout: "请求在写入前已到期。", permission_unknown: "宿主未允许复制。", storage_unavailable: "历史存储不可用。",
          };
          message = `未执行复制：${(result.reason && reasons[result.reason]) || "无法开始复制，请稍后手动重试。"}`;
        }
      }
    } catch { /* Once dispatched, transport failure is uncertain; never replay. */ }
    finally {
      copyPending = false;
      store.setState({
        copyBusy: false,
        ...(active && token === lifetime && mayHaveWritten ? { mergeCopyMessage: null } : {}),
        ...(ownsPreview(token, sel, gen) ? { copyMessage: message } : {}),
      });
    }
    return copied;
  }
  function measurementReceipt() { return committedReceipt; }
  function acknowledgeMeasurement(receipt: Receipt, inputFocused: boolean) {
    const state = store.getState();
    if (!measurement || !active || committedReceipt !== receipt || submittedReceipt === receipt || measuredIdentity?.model !== receipt.model || state.loading || state.error || receipt.generation !== String(generation) || receipt.sequence !== String(measuredSequence)) return;
    submittedReceipt = receipt;
    const token = lifetime;
    // No await between the authoritative state check and transport invocation.
    // Submission is at most once; rejection is never an accepted host ACK.
    void measurement.ack(receipt, inputFocused).catch(() => {
      if (active && lifetime === token && committedReceipt === receipt) committedReceipt = null;
    });
  }
  function copyIdentity() { return `${lifetime}:${generation}:${selection}`; }
  return { store, start, refresh, more, toggle, setEnabled, setQuery, setFavoritesOnly, setFavorite, selectClip, retryPreview, retryPng, morePreview, copyText: () => copy("text"), copyPng: () => copy("png"), copyIdentity, toggleMergeItem, removeMergeItem, moveMergeItem, clearMergeItems, canCopyMergedText, copyMergedText, mergeIdentity, measurementReceipt, acknowledgeMeasurement };
}
