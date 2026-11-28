import { HistoryManagement } from "./HistoryManagement";
import { nativeManagement } from "./lib/history-management";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStore } from "zustand";
import { nativeBridge, nativePanelControl, nativeResidencyControl, nativeSettingsControl, nativeStartupCaptureControl, type DesktopBridge, type ManualPanelControl } from "./lib/desktop";
import { ExclusionSettings } from "./ExclusionSettings";
import { StartupCaptureSettings } from "./StartupCaptureSettings";
import type { SettingsControl } from "./lib/settings";
import type { StartupCaptureControl } from "./lib/startup-capture";
import { useResidency, type ResidencyControl } from "./lib/residency";
import { useManualPanel } from "./lib/manual-panel-state";
import { ManualPanelInput, isImeKey, isMergeAction, type PanelAction, type PanelIntent } from "./lib/manual-panel-input";
import { canPauseWhileStarting, createHistoryModel, isDrainingCaptureQueue } from "./lib/history-store";
import { normalizeQuery, type ClipSummary, type PngPreview } from "./lib/ipc";
import { interactiveInput } from "./lib/measurement";

function HistoryItem({ item, selected, disabled, onSelect, merge }: {
  item: ClipSummary; selected: boolean; disabled: boolean; onSelect: (id: string) => void;
  merge?: { index: number; disabled: boolean };
}) {
  const date = new Date(item.captured_at_ms);
  const validTime = Number.isFinite(date.getTime());
  const text = item.format_names.includes("CF_UNICODETEXT");
  const previewButton = <button type="button" aria-pressed={selected} disabled={disabled} onClick={() => onSelect(item.id)}>{item.kind === "png" ? "查看图片" : "查看正文"}</button>;
  return (
    <li data-history-id={item.id}>
      <p className="preview">{item.preview || (item.kind === "png" ? "已保存 PNG 样本" : "已保存非文本内容")}</p>
      <p className="item-meta">
        <time dateTime={validTime ? date.toISOString() : undefined}>{validTime ? date.toLocaleString() : "时间超出可显示范围"}</time>
        <span>{item.format_names.join(" · ")}</span>
        <span>{Math.ceil(item.payload_bytes / 1024)} KiB</span>
        {item.favorited && <span>★ 已收藏</span>}
        <span>{item.source_confidence === "unknown" ? "来源未知" : "来源已核对"}</span>
      </p>
      {merge ? <div className="history-row-actions">
        {previewButton}
        <button type="button" data-merge-action="toggle-merge" data-clip-id={item.id} aria-pressed={merge.index >= 0} disabled={merge.disabled || !text}>
          {!text ? "仅支持文本" : merge.index < 0 ? "加入合并" : `已选 #${merge.index + 1} · 取消`}
        </button>
      </div> : previewButton}
    </li>
  );
}

function PngThumbnail({ image }: { image: PngPreview }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    canvas.current?.getContext("2d")?.putImageData(
      new ImageData(new Uint8ClampedArray(image.rgba), image.width, image.height), 0, 0,
    );
  }, [image]);
  return <figure className="png-preview">
    <canvas ref={canvas} width={image.width} height={image.height} role="img" aria-label="PNG 缩略图" />
    <figcaption>PNG · {image.sourceWidth} × {image.sourceHeight} 像素</figcaption>
  </figure>;
}

export default function App({ bridge = nativeBridge, panel, panelControl = nativePanelControl, residencyControl = nativeResidencyControl, settingsControl = nativeSettingsControl, startupCaptureControl = nativeStartupCaptureControl }: { bridge?: DesktopBridge; panel?: { hide(afterCopy?: true): void | Promise<void> }; panelControl?: ManualPanelControl; residencyControl?: ResidencyControl; settingsControl?: SettingsControl; startupCaptureControl?: StartupCaptureControl }) {
  const model = useMemo(() => createHistoryModel(bridge), [bridge]);
  const view = useStore(model.store);
  useEffect(() => model.start(), [model]);
  const panelState = useManualPanel(panelControl, !panel);
  const residency = useResidency(residencyControl, !panel);
  const [settingsBlocked, setSettingsBlocked] = useState(settingsControl.available);
  const closing = residency.pending || (!!residency.snapshot && residency.snapshot.phase !== "running");
  const input = useMemo(() => new ManualPanelInput(), []);
  const focusVersion = useRef(0);
  const hidePending = useRef(false);
  const [mode, setMode] = useState<"search" | "browse">("search");
  const titleRef = useRef<HTMLHeadingElement>(null);
  const [showBackToTop, setShowBackToTop] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLOListElement>(null);
  const diagnosticsRef = useRef<HTMLDetailsElement>(null);
  const previousDiagnosticReasons = useRef(0);
  const viewportRef = useRef<HTMLDivElement>(null);
  const browsing = !!panel && mode === "browse";
  const actionsAllowed = !closing;
  useEffect(() => {
    const syncPageScroll = () => setShowBackToTop(window.scrollY > 200);
    syncPageScroll();
    window.addEventListener("scroll", syncPageScroll, { passive: true });
    return () => window.removeEventListener("scroll", syncPageScroll);
  }, []);
  function backToTop() {
    if (!actionsAllowed || view.status?.state === "stopping") return;
    window.scrollTo({ top: 0, behavior: "instant" });
    titleRef.current?.focus({ preventScroll: true });
  }
  useLayoutEffect(() => {
    if (viewportRef.current) viewportRef.current.scrollTop = 0;
  }, [view.query, view.listRevision]);
  function revealSelection(id: string | null) {
    const viewport = viewportRef.current;
    const row = listRef.current?.querySelector<HTMLElement>(`[data-history-id="${id}"]`);
    if (!viewport || !row || viewport.clientHeight === 0) return;
    const bounds = viewport.getBoundingClientRect(), item = row.getBoundingClientRect();
    const top = bounds.top + viewport.clientTop, bottom = top + viewport.clientHeight;
    if (item.height > viewport.clientHeight || item.top < top) viewport.scrollTop += item.top - top;
    else if (item.bottom > bottom) viewport.scrollTop += item.bottom - bottom;
  }
  useLayoutEffect(() => { revealSelection(view.selectedId); }, [view.selectedId]);
  useEffect(() => { if (panel) searchRef.current?.focus({ preventScroll: true }); }, [panel !== undefined]);
  useEffect(() => {
    if (!panel) return;
    const cancel = () => { input.invalidate(); focusVersion.current++; };
    window.addEventListener("blur", cancel);
    return () => { window.removeEventListener("blur", cancel); cancel(); };
  }, [input, panel !== undefined]);
  useEffect(() => {
    const receipt = model.measurementReceipt();
    if (!receipt) return;
    const check = () => {
      if (interactiveInput(searchRef.current, listRef.current, !!panel, receipt.content, actionsAllowed)) model.acknowledgeMeasurement(receipt, document.activeElement === searchRef.current);
    };
    check();
    window.addEventListener("focus", check); document.addEventListener("focusin", check); document.addEventListener("visibilitychange", check);
    return () => { window.removeEventListener("focus", check); document.removeEventListener("focusin", check); document.removeEventListener("visibilitychange", check); };
  }, [model, view, panel, actionsAllowed]);
  function enterBrowse(id?: string) {
    if (closing) return false;
    const state = model.store.getState();
    if (state.loading || state.error || state.summaries.length === 0) return false;
    const selected = id ?? state.selectedId ?? state.summaries[0].id;
    if (!state.summaries.some(item => item.id === selected)) return false;
    input.invalidate();
    model.selectClip(selected);
    setMode("browse");
    listRef.current?.focus({ preventScroll: true });
    return true;
  }
  function returnToSearch() {
    input.invalidate();
    setMode("search");
    model.selectClip(null);
    searchRef.current?.focus({ preventScroll: true });
  }
  function updateQuery(value: string) {
    if (closing || value === model.store.getState().query) return;
    input.invalidate();
    if (panel) setMode("search");
    model.setQuery(value);
  }
  function newInput() {
    input.invalidate(); setMode("search");
    if (model.store.getState().selectedId !== null) model.selectClip(null);
  }
  function compositionQuery(target: EventTarget) {
    const editor = searchRef.current;
    if (!editor || target !== editor) return undefined;
    return { value: editor.value, start: editor.selectionStart ?? editor.value.length, end: editor.selectionEnd ?? editor.value.length };
  }
  function intent(action: PanelAction | null, target: HTMLElement, clipId?: string): PanelIntent | null {
    if (!action || closing || hidePending.current) return null;
    const state = model.store.getState();
    if (isMergeAction(action)) {
      if (state.status?.state === "stopping" || (target instanceof HTMLButtonElement && target.disabled)) return null;
      if (action === "copy-merged" && !model.canCopyMergedText()) return null;
      if (action === "toggle-merge") {
        clipId ??= state.selectedId ?? undefined;
        if (state.loading || state.error || !state.summaries.some(item => item.id === clipId)) return null;
      }
      return { action, target, clipId, context: model.mergeIdentity() };
    }
    if (action === "copy" || action === "copy-png") {
      if (!browsing || state.loading || state.error || state.copyBusy || !state.status || state.status.state === "stopping" || state.status.services.storage !== "ready") return null;
      if (action === "copy-png" && (!bridge.copyPng || state.pngPreview?.phase !== "ready")) return null;
      const format = action === "copy" ? "CF_UNICODETEXT" : "PNG";
      if (!state.summaries.some(item => item.id === state.selectedId && item.format_names.includes(format))) return null;
    }
    if (action === "favorite") {
      const item = state.summaries.find(row => row.id === state.selectedId);
      if (!browsing || !item || !bridge.setFavorite || state.loading || state.error || state.favoriteBusy ||
          state.status?.services.storage !== "ready" || state.status.state === "stopping") return null;
      return { action, target, context: `${model.copyIdentity()}:${item.id}:${!item.favorited}` };
    }
    if (action === "hide" && input.imeSeen && !browsing) return null;
    return { action, target, context: model.copyIdentity() };
  }
  function control(target: EventTarget): { action: PanelAction; target: HTMLButtonElement; clipId?: string } | null {
    const element = (target as HTMLElement).closest<HTMLButtonElement>("button");
    if (element?.dataset.copyAction === "true") return { action: "copy" as const, target: element };
    if (element?.dataset.copyAction === "png") return { action: "copy-png" as const, target: element };
    if (element?.dataset.favoriteAction === "true") return { action: "favorite" as const, target: element };
    if (element?.dataset.hideAction === "true") return { action: "hide" as const, target: element };
    if (element && isMergeAction(element.dataset.mergeAction)) {
      return { action: element.dataset.mergeAction, target: element, clipId: element.dataset.clipId };
    }
    return null;
  }
  function listAction(target: HTMLElement, key: string): PanelAction | null {
    if (!browsing) return null;
    if (key === "Enter" && target === listRef.current) return "copy";
    if (key === " " && target === listRef.current) return "toggle-merge";
    if (key === "Escape" && listRef.current?.contains(target)) return "hide";
    return null;
  }
  async function hidePanel(afterCopy?: true) {
    if (!panel || hidePending.current) return;
    hidePending.current = true;
    focusVersion.current++;
    try {
      await (afterCopy ? panel.hide(true) : panel.hide());
    } finally {
      hidePending.current = false;
    }
  }
  async function copyFromList(target: HTMLElement) {
    const focus = focusVersion.current;
    const context = model.copyIdentity();
    const copied = await model.copyText();
    if (
      copied && focusVersion.current === focus && target.ownerDocument.activeElement === target &&
      model.copyIdentity() === context && model.store.getState().status?.state !== "stopping"
    ) {
      await hidePanel(true);
    }
  }
  function perform(action: PanelAction, clipId?: string) {
    if (action === "copy") void model.copyText();
    else if (action === "copy-png") void model.copyPng();
    else if (action === "favorite") void model.setFavorite();
    else if (action === "hide") void hidePanel();
    else if (action === "copy-merged") void model.copyMergedText();
    else if (action === "merge-clear") model.clearMergeItems();
    else if (clipId) {
      if (action === "merge-remove") model.removeMergeItem(clipId);
      else if (action === "merge-up") model.moveMergeItem(clipId, -1);
      else if (action === "merge-down") model.moveMergeItem(clipId, 1);
      else if (action === "toggle-merge") {
        const item = model.store.getState().summaries.find(row => row.id === clipId);
        if (item) model.toggleMergeItem(item);
      }
    }
  }
  function copyText() { if (actionsAllowed && !panel) void model.copyText(); }
  function copyPng() { if (actionsAllowed && !panel) void model.copyPng(); }
  const status = view.status;
  const normalizedQuery = normalizeQuery(view.query);
  const searching = normalizedQuery.ok && normalizedQuery.value.length > 0;
  const label = !status ? "正在读取状态" : ({ listening: "正在收录", paused: "已暂停", overloaded: "已停止新增：队列或历史容量已满", failed: "存储或抓取异常", stopping: "正在退出", startup: "正在启动" })[status.state];
  const retryCapture = status?.services.storage === "failed" && status.queue.items > 0;
  const pauseWhileStarting = canPauseWhileStarting(view);
  const captureChanging = (!!view.changing && !pauseWhileStarting) || isDrainingCaptureQueue(status);
  const exitFailed = residency.snapshot?.phase === "failed";
  const queuedWithoutCapture = !!status && status.queue.items > 0 && !status.capture.admission_open;
  const diagnosticReasons = (exitFailed ? 1 : 0) | (status?.state === "overloaded" ? 2 : 0)
    | (status?.state === "failed" ? 4 : 0) | (queuedWithoutCapture ? 8 : 0);
  useLayoutEffect(() => {
    const details = diagnosticsRef.current;
    const previous = previousDiagnosticReasons.current;
    previousDiagnosticReasons.current = details ? diagnosticReasons : 0;
    if (!details) return;
    if (diagnosticReasons & ~previous) details.open = true;
    else if (previous && !diagnosticReasons) details.open = false;
  }, [diagnosticReasons, view.hostMissing, panel !== undefined]);
  const historyCount = status && <div><dt>已存历史</dt><dd>{status.history.items} 条</dd></div>;
  const diagnosticCounts = status && (
    <dl className="counts">
      {panel && historyCount}
      <div><dt>未保存队列</dt><dd>{status.queue.items} 条 · {Math.ceil(status.queue.bytes / 1024)} KiB</dd></div>
      <div><dt>已忽略 / 已覆盖</dt><dd>{status.counters.ignored} / {status.counters.superseded}</dd></div>
      <div><dt>已取消</dt><dd>{status.counters.cancelled}</dd></div>
    </dl>
  );
  const selectedItem = view.summaries.find(item => item.id === view.selectedId);
  const selectedFormats = selectedItem?.format_names;
  const copyHasText = selectedFormats?.includes("CF_UNICODETEXT") ?? false;
  const hasPng = selectedFormats?.includes("PNG") ?? false;
  const imageOnly = hasPng && !copyHasText;
  const copyDisabled = !actionsAllowed || (!!panel && !browsing) || !!view.copyBusy || view.loading || !status || status.state === "stopping" || status.services.storage !== "ready";
  const mergeEditingDisabled = !actionsAllowed || status?.state === "stopping";
  const favoriteDisabled = !actionsAllowed || (!!panel && !browsing) || view.favoriteBusy ||
    view.loading || !!view.error || status?.state === "stopping" || status?.services.storage !== "ready";
  return (
    <main className="shell history-shell" onCompositionStartCapture={event => { if (panel) { input.compositionStart(compositionQuery(event.target)); newInput(); } }} onCompositionUpdateCapture={event => { if (panel) { input.compositionUpdate(compositionQuery(event.target)); newInput(); } }} onCompositionEndCapture={event => { if (panel) { const editor = searchRef.current; const query = input.compositionEnd(event.target === editor ? event.data : "", event.target === editor ? editor?.value : undefined); if (query !== null) updateQuery(query); } }} onKeyUpCapture={event => {
      if (!panel) return;
      const target = event.target as HTMLElement;
      const fromList = listAction(target, event.key);
      const ctrl = fromList === "hide" ? null : control(target);
      const action = ctrl?.action ?? fromList;
      const candidate = intent(action, ctrl?.target ?? target, ctrl?.clipId);
      const accepted = input.keyUp(event.nativeEvent, candidate);
      if (action || (input.composingSearch && target.closest("button") && (event.key === "Enter" || event.key === " "))) {
        event.preventDefault(); event.stopPropagation();
      }
      if (accepted === "copy" && !ctrl) void copyFromList(target);
      else if (accepted) perform(accepted, candidate?.clipId);
    }} onKeyDownCapture={event => {
      if (!panel) return;
      const key = event.key, ime = isImeKey(event.nativeEvent);
      const target = event.target as HTMLElement;
      if (ime) newInput();
      const fromList = listAction(target, key);
      const ctrl = fromList === "hide" ? null : control(target);
      const action = ctrl?.action ?? fromList;
      const candidate = intent(action, ctrl?.target ?? target, ctrl?.clipId);
      const independentArrowDown = input.keyDown(
        event.nativeEvent, candidate, compositionQuery(event.target), browsing && ctrl?.action === "copy",
      );
      if ((action && (key === "Enter" || key === " " || key === "Escape")) ||
          (input.composingSearch && target.closest("button") && (key === "Enter" || key === " "))) {
        event.preventDefault(); event.stopPropagation(); return;
      }
      if (target.id === "history-search") {
        if (independentArrowDown) {
          const first = model.store.getState().summaries[0];
          if (first && enterBrowse(first.id)) {
            event.preventDefault();
            event.stopPropagation();
          }
        } else if (key === "Enter" && !ime) {
          event.preventDefault();
        }
        return;
      }
      if (ime || event.repeat || closing) { if (target.closest("button")) event.preventDefault(); event.stopPropagation(); return; }
      if (browsing && (key === "ArrowUp" || key === "ArrowDown") && target.closest(".history-list") !== null) {
        input.invalidate();
        event.preventDefault(); const index = view.summaries.findIndex(item => item.id === view.selectedId);
        const next = index < 0 ? (key === "ArrowDown" ? 0 : view.summaries.length - 1) : Math.max(0, Math.min(view.summaries.length - 1, index + (key === "ArrowDown" ? 1 : -1)));
        if (view.summaries[next]) {
          const id = view.summaries[next].id;
          model.selectClip(id);
          if (id === view.selectedId) revealSelection(id);
        }
      }
    }} onPointerDownCapture={event => {
      if (!panel) return;
      const ctrl = control(event.target);
      input.pointerDown(event.nativeEvent, ctrl ? intent(ctrl.action, ctrl.target, ctrl.clipId) : null);
    }} onPointerUpCapture={event => {
      if (!panel) return;
      const ctrl = control(event.target);
      input.pointerUp(event.nativeEvent, ctrl ? intent(ctrl.action, ctrl.target, ctrl.clipId) : null);
    }} onBlurCapture={event => { input.focusLeft(event.target); focusVersion.current++; }} onPointerCancelCapture={() => input.invalidate()} onClickCapture={event => {
      if (!panel) return;
      const ctrl = control(event.target);
      if (!ctrl) return;
      event.preventDefault(); event.stopPropagation();
      const candidate = intent(ctrl.action, ctrl.target, ctrl.clipId);
      if (input.takeClick(event.detail, candidate)) perform(ctrl.action, candidate?.clipId);
    }}>
      <header><h1 ref={titleRef} tabIndex={-1}>{panel ? "片羽 · 快捷面板" : "片羽"}</h1><p className="tagline">{panel ? browsing ? "上下键选择，空格加入或取消合并，Enter 复制当前条目并收起。Esc 收起。" : "输入搜索词，按 ↓ 浏览结果" : "本地剪贴板历史"}</p>
        {panel ? <button type="button" data-hide-action="true" disabled={!actionsAllowed || (input.imeSeen && !browsing)}>隐藏快捷面板</button> : <>
          <button type="button" disabled={closing || !panelControl.available || panelState.snapshot?.phase === "stopping" || panelState.snapshot?.phase === "unavailable"} onClick={() => void panelState.open()}>打开快捷面板</button>
          <p role="status">{panelState.snapshot?.hotkey === "ready" ? "快捷键：Ctrl+Alt+V" : "可使用主窗口入口打开快捷面板。"}</p>
          {panelState.snapshot?.error && <p role="alert">{panelState.snapshot.error}</p>}
          {panelState.error && <p role="alert">{panelState.error}</p>}
          <button type="button" disabled={!residency.snapshot || residency.pending || residency.snapshot.phase === "stopping" || residency.snapshot.phase === "exited"} onClick={() => void residency.exit()}>{exitFailed ? "重试退出片羽" : "退出片羽"}</button>
          {residency.snapshot?.phase === "stopping" && <p role="status">正在退出片羽，等待保存和资源回收。</p>}
          {(!exitFailed || view.hostMissing) && residency.snapshot?.error && <p role="alert">{residency.snapshot.error}</p>}
          {residency.error && <p role="alert">{residency.error}</p>}
          {!residencyControl.available && <p role="status">当前环境未提供驻留与退出控制。</p>}
        </>}
      </header>
      {view.hostMissing ? <p className="notice" role="status">需要桌面宿主。请从片羽桌面程序打开此界面。</p> : <>
        <section className="status-card" aria-label="收录状态">
          <div className="toolbar"><p role="status" aria-live="polite">{label}</p>
            {!panel && status && <dl className="counts">{historyCount}</dl>}
            {!panel && <button disabled={closing || !status || captureChanging || (!status.capture.user_enabled && settingsBlocked) || (status.state === "stopping" && !retryCapture)} onClick={() => void (retryCapture ? model.toggle() : model.setEnabled(!status?.capture.user_enabled))}>{captureChanging ? "正在更改…" : retryCapture ? "重试保存" : status?.capture.user_enabled ? "暂停收录" : "开始收录"}</button>}
          </div>
          {panel ? <>
            {diagnosticCounts}
            {status && <details><summary>数据目录</summary><p className="data-path">{status.data_dir}</p></details>}
            {status?.state === "failed" && <p className="error" role="alert">{status.queue.items > 0 ? "仍有未保存内容。可重试保存，退出未完成时窗口会保留。" : "服务暂不可用，请检查隔离目录或重启程序。"}</p>}
          </> : <details ref={diagnosticsRef} className="capture-diagnostics">
            <summary>收录诊断</summary>
            <div className="capture-diagnostics-body">
              {status?.state === "overloaded" && <p className="notice">队列处理完后，可暂停再开始收录。历史容量用完时，可暂停收录后在历史管理中释放空间。</p>}
              {status?.state === "failed" && <p className="error" role="alert">{retryCapture ? "仍有未保存内容，可重试保存。" : "服务暂不可用，请检查数据目录或重启程序。"}</p>}
              {queuedWithoutCapture && status.state !== "failed" && !exitFailed && <p className="notice">还有 {status.queue.items} 条内容待处理，待队列归零后可管理历史。</p>}
              {exitFailed && <p className="error" role="alert">退出未完成，仍有 {status?.queue.items ?? "待确认"} 条未保存内容。收录和复制保持关闭。</p>}
              {exitFailed && residency.snapshot?.error && <p className="error" role="alert">{residency.snapshot.error}</p>}
              {exitFailed && retryCapture && <div className="toolbar"><button disabled={residency.pending} onClick={() => void residency.retrySaving().then(() => model.refresh(false))}>仅重试保存</button></div>}
              {diagnosticCounts}
              {status && <dl className="counts"><div><dt>数据目录</dt><dd>{status.data_dir}</dd></div></dl>}
              <p className="notice">暂停确认后会取消尚未保存的旧收录任务，已保存的历史仍可浏览和复制。重试保存用于留在队列中的失败任务。</p>
            </div>
          </details>}
        </section>
        {!panel && startupCaptureControl.available && <StartupCaptureSettings control={startupCaptureControl} closing={closing || status?.state === "stopping"} />}
        {!panel && <ExclusionSettings control={settingsControl} closing={closing} onApplied={() => void model.refresh(false)} onBlocked={setSettingsBlocked} />}
        {!panel && <HistoryManagement control={nativeManagement} ready={!closing && !!status && !status.capture.user_enabled && status.queue.items === 0 && status.services.storage === "ready" && !view.copyBusy && !view.loading} items={status?.history.items ?? 0} favorites={status?.history.favorites ?? 0} historyRevision={status?.history.revision ?? "0"} selectedId={view.selectedId} selectedFavorited={selectedItem?.favorited ?? false} onChanged={() => { model.selectClip(null); void model.refresh(); }} />}
        {view.error && <p className="error" role="alert">{view.error}</p>}
        <section aria-labelledby="history-heading" aria-busy={view.loading}>
          <div className="toolbar"><h2 id="history-heading">最近历史</h2><button disabled={!actionsAllowed || view.loading} onClick={() => { if (actionsAllowed) void model.refresh(); }}>刷新</button></div>
          <div className="search-bar">
            <label className="search-label" htmlFor="history-search">搜索历史正文</label>
            <input ref={searchRef} id="history-search" className="search-input" type="search" value={view.query} onChange={event => updateQuery(event.target.value)} autoComplete="off" />
            <button type="button" disabled={!actionsAllowed} onClick={() => { updateQuery(""); if (panel) searchRef.current?.focus({ preventScroll: true }); }}>清空搜索</button>
            <button type="button" className="favorites-filter" aria-pressed={view.favoritesOnly} disabled={!actionsAllowed} onClick={() => {
              if (!actionsAllowed) return;
              if (panel) input.invalidate();
              model.setFavoritesOnly(!view.favoritesOnly);
            }}>只看收藏</button>
            {panel && (browsing ? <button key="return-search" type="button" onClick={returnToSearch}>返回搜索</button> : <button key="enter-browse" type="button" disabled={!actionsAllowed || view.loading || !!view.error || view.summaries.length === 0} onClick={() => enterBrowse()}>浏览结果</button>)}
          </div>
          {panel && <p className="notice input-status" role="status">{view.loading ? "正在搜索，结果更新后可查看。" : view.error ? "暂时无法浏览，请刷新后重试。" : view.summaries.length === 0 ? "当前没有可浏览的结果。" : `结果对应搜索词：${view.query || "全部历史"}。${browsing ? "" : "按 ↓ 预览首条。"}`}</p>}
          {view.loading && <p role="status">正在读取历史…</p>}
          {!view.loading && !view.error && view.summaries.length === 0 && (view.favoritesOnly
            ? <p className="notice" role="status">{searching ? "没有匹配的收藏。" : "暂无收藏。"}</p>
            : searching
            ? <p className="notice" role="status">没有匹配的历史正文。</p>
            : status?.services.storage === "ready" && <p className="notice" role="status">暂无历史。开始收录后，在其他应用中复制内容。</p>)}
          <div ref={viewportRef} className="history-viewport" role="region" aria-label="历史列表滚动区域">
          <ol ref={listRef} className="history-list" tabIndex={panel ? browsing ? 0 : -1 : undefined} aria-label="历史条目">{view.summaries.map(item => <HistoryItem key={item.id} item={item} selected={view.selectedId === item.id} disabled={!actionsAllowed || view.loading} onSelect={id => panel ? enterBrowse(id) : model.selectClip(id)} merge={panel ? {
            index: view.mergeItems.findIndex(row => row.id === item.id), disabled: mergeEditingDisabled || view.loading || !!view.error,
          } : undefined} />)}</ol>
          </div>
          {view.cursor && <button disabled={!actionsAllowed || view.loading} onClick={() => { if (actionsAllowed) void model.more(); }}>加载更多</button>}
        </section>
        {panel && view.mergeItems.length > 0 && <section className="merge-card" aria-labelledby="merge-heading">
          <div className="toolbar">
            <h2 id="merge-heading">已选合并</h2>
            <div className="merge-actions">
              <button type="button" data-merge-action="merge-clear" disabled={mergeEditingDisabled}>清空已选</button>
              <button type="button" data-merge-action="copy-merged" disabled={!actionsAllowed || !model.canCopyMergedText()}>
                {view.copyBusy === "merged" ? "正在合并复制…" : `合并复制（${view.mergeItems.length} 条）`}
              </button>
            </div>
          </div>
          <ol className="merge-items">
            {view.mergeItems.map(({ id, preview }, index) => <li key={id}>
              <div className="merge-item">
                <div className="merge-item-content">
                  <div className="merge-item-label">
                    <span>#{id}</span>
                    {view.mergeMissingClipId === id && <strong>已不存在</strong>}
                  </div>
                  <div className="merge-item-preview">{preview || "（空文本）"}</div>
                </div>
                <div className="merge-actions">
                  <button type="button" data-merge-action="merge-up" data-clip-id={id} aria-label={`上移第 ${index + 1} 条`} disabled={mergeEditingDisabled || index === 0}>上移</button>
                  <button type="button" data-merge-action="merge-down" data-clip-id={id} aria-label={`下移第 ${index + 1} 条`} disabled={mergeEditingDisabled || index === view.mergeItems.length - 1}>下移</button>
                  <button type="button" data-merge-action="merge-remove" data-clip-id={id} aria-label={`移出第 ${index + 1} 条`} disabled={mergeEditingDisabled}>移出</button>
                </div>
              </div>
            </li>)}
          </ol>
          <p className="notice">每条最多预览 200 个字符，合并复制时将按 ID 读取完整正文</p>
          <pre className="preview-body merge-preview" aria-label="合并预览">{view.mergeItems.map(item => item.preview).join("\n\n")}</pre>
        </section>}
        {panel && view.mergeError && <p className="error" role="alert">{view.mergeError}</p>}
        {panel && view.mergeCopyMessage && <p className="notice" role="status" aria-live="polite">{view.mergeCopyMessage}</p>}
        <section className="preview-card" aria-labelledby="preview-heading">
          <div className="toolbar">
            <h2 id="preview-heading">{hasPng ? "内容预览" : "正文预览"}</h2>
            {!imageOnly && <button type="button" data-copy-action="true" disabled={copyDisabled || !view.copyAvailable || !copyHasText} onClick={copyText}>{view.copyBusy === "text" ? "复制中…" : "复制纯文本"}</button>}
            {hasPng && bridge.copyPng && <button type="button" data-copy-action="png" disabled={copyDisabled || view.pngPreview?.phase !== "ready"} onClick={copyPng}>{view.copyBusy === "png" ? "复制中…" : "复制图片"}</button>}
            {selectedItem && bridge.setFavorite && <button type="button" data-favorite-action="true" disabled={favoriteDisabled} onClick={() => {
              if (!panel && !favoriteDisabled) void model.setFavorite();
            }}>{selectedItem.favorited ? "取消收藏" : "收藏"}</button>}
            {view.selectedId && <button type="button" disabled={!actionsAllowed} onClick={() => { if (!actionsAllowed) return; model.selectClip(null); if (browsing) listRef.current?.focus({ preventScroll: true }); }}>{hasPng ? "关闭预览" : "关闭正文预览"}</button>}
          </div>
          {!imageOnly && !view.copyAvailable && <p className="notice">当前宿主不支持复制纯文本。</p>}
          {view.favoriteMessage && <p role="alert">{view.favoriteMessage}</p>}
          {view.copyMessage && <p role="status" aria-live="polite">{view.copyMessage}</p>}
          {!view.selectedId ? <p className="notice">从列表中选择一条历史以预览正文。</p> : <>
            {hasPng && <>
              {!bridge.getPngPreview && <p className="notice">当前宿主无法预览 PNG。</p>}
              {view.pngPreview?.phase === "loading" && <p role="status">正在读取图片…</p>}
              {view.pngPreview?.phase === "ready" && <>
                <PngThumbnail image={view.pngPreview.image} />
                {imageOnly && panel && bridge.copyPng && <p className="notice">使用「复制图片」将此图片写入剪贴板。</p>}
              </>}
              {view.pngPreview?.phase === "failed" && <>
                <p className="error" role="alert">{view.pngPreview.message}</p>
                <button type="button" disabled={!actionsAllowed || view.loading} onClick={model.retryPng}>重试读取图片</button>
              </>}
            </>}
            {view.previewLoading && <p role="status">正在读取正文…</p>}
            {view.previewError && <p className="error" role="alert">{view.previewError}</p>}
            {view.previewText !== "" && <pre className="preview-body">{view.previewText}</pre>}
            {view.previewError
              ? <button type="button" onClick={() => (view.previewNextOffset === null ? model.retryPreview() : model.morePreview())}>{view.previewNextOffset === null ? "重试读取正文" : "重试继续读取正文"}</button>
              : view.previewNextOffset !== null
                ? <button type="button" disabled={view.previewLoading} onClick={() => model.morePreview()}>继续读取正文</button>
                : !imageOnly && !view.previewLoading && (view.previewText === ""
                  ? <p className="notice">无可预览正文。</p>
                  : <p className="notice" role="status">读取完毕。</p>)}
          </>}
        </section>
        <p className="notice">列表只显示已提交的历史；未保存队列单独计数。隐私标记、内部写回及应用排除会跳过收录。{panel ? "面板收起后，在目标应用按 Ctrl+V 粘贴。" : "复制后请自行切回目标应用粘贴。"}</p>
      </>}
      {showBackToTop && <button type="button" className="back-to-top" disabled={!actionsAllowed || status?.state === "stopping"} onClick={backToTop}>返回顶部</button>}
    </main>
  );
}
