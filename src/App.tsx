import { HistoryManagement } from "./HistoryManagement";
import { nativeManagement } from "./lib/history-management";
import { useEffect, useMemo, useRef, useState } from "react";
import { useStore } from "zustand";
import { nativeBridge, nativePanelControl, nativeResidencyControl, nativeSettingsControl, type DesktopBridge, type ManualPanelControl } from "./lib/desktop";
import { ExclusionSettings } from "./ExclusionSettings";
import type { SettingsControl } from "./lib/settings";
import { useResidency, type ResidencyControl } from "./lib/residency";
import { useManualPanel } from "./lib/manual-panel-state";
import { ManualPanelInput, isImeKey, type PanelAction, type PanelIntent } from "./lib/manual-panel-input";
import { createHistoryModel } from "./lib/history-store";
import { normalizeQuery, type ClipSummary } from "./lib/ipc";
import { interactiveInput } from "./lib/measurement";

function HistoryItem({ item, selected, disabled, onSelect }: { item: ClipSummary; selected: boolean; disabled: boolean; onSelect: (id: string) => void }) {
  const date = new Date(item.captured_at_ms);
  const validTime = Number.isFinite(date.getTime());
  return (
    <li>
      <p className="preview">{item.preview || (item.kind === "png" ? "已保存 PNG 样本" : "已保存非文本内容")}</p>
      <p className="item-meta">
        <time dateTime={validTime ? date.toISOString() : undefined}>{validTime ? date.toLocaleString() : "时间超出可显示范围"}</time>
        <span>{item.format_names.join(" · ")}</span>
        <span>{Math.ceil(item.payload_bytes / 1024)} KiB</span>
        <span>{item.source_confidence === "unknown" ? "来源未知" : "来源已核对"}</span>
      </p>
      <button type="button" aria-pressed={selected} disabled={disabled} onClick={() => onSelect(item.id)}>查看正文</button>
    </li>
  );
}

export default function App({ bridge = nativeBridge, panel, panelControl = nativePanelControl, residencyControl = nativeResidencyControl, settingsControl = nativeSettingsControl }: { bridge?: DesktopBridge; panel?: { hide(): void }; panelControl?: ManualPanelControl; residencyControl?: ResidencyControl; settingsControl?: SettingsControl }) {
  const model = useMemo(() => createHistoryModel(bridge), [bridge]);
  const view = useStore(model.store);
  useEffect(() => model.start(), [model]);
  const panelState = useManualPanel(panelControl, !panel);
  const residency = useResidency(residencyControl, !panel);
  const [settingsBlocked, setSettingsBlocked] = useState(settingsControl.available);
  const closing = residency.pending || (!!residency.snapshot && residency.snapshot.phase !== "running");
  const input = useMemo(() => new ManualPanelInput(), []);
  const [mode, setMode] = useState<"search" | "browse">("search");
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLOListElement>(null);
  const browsing = !!panel && mode === "browse";
  const actionsAllowed = !closing;
  useEffect(() => { if (panel) searchRef.current?.focus(); }, [panel !== undefined]);
  useEffect(() => {
    if (!panel) return;
    const cancel = () => input.invalidate();
    window.addEventListener("blur", cancel);
    return () => { window.removeEventListener("blur", cancel); input.invalidate(); };
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
    if (closing) return;
    const state = model.store.getState();
    if (state.loading || state.error || state.summaries.length === 0) return;
    const selected = id ?? state.selectedId ?? state.summaries[0].id;
    if (!state.summaries.some(item => item.id === selected)) return;
    input.invalidate();
    model.selectClip(selected);
    setMode("browse");
    listRef.current?.focus();
  }
  function returnToSearch() {
    input.invalidate();
    setMode("search");
    model.selectClip(null);
    searchRef.current?.focus();
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
  function intent(action: PanelAction | null, target: EventTarget): PanelIntent | null {
    if (!action || closing) return null;
    const state = model.store.getState();
    if (action === "copy" && (!browsing || state.loading || state.error || state.copyBusy || !state.summaries.some(item => item.id === state.selectedId && item.format_names.includes("CF_UNICODETEXT")) || !state.status || state.status.state === "stopping" || state.status.services.storage !== "ready")) return null;
    if (action === "hide" && input.imeSeen && !browsing) return null;
    return { action, target, context: model.copyIdentity() };
  }
  function control(target: EventTarget) {
    const element = (target as HTMLElement).closest<HTMLElement>("button");
    return element?.dataset.copyAction === "true" ? { action: "copy" as const, target: element } : element?.dataset.hideAction === "true" ? { action: "hide" as const, target: element } : null;
  }
  function perform(action: PanelAction) { if (action === "copy") void model.copyText(); else panel?.hide(); }
  function copyText() { if (actionsAllowed && !panel) void model.copyText(); }
  const status = view.status;
  const normalizedQuery = normalizeQuery(view.query);
  const searching = normalizedQuery.ok && normalizedQuery.value.length > 0;
  const label = !status ? "正在读取状态" : ({ listening: "正在收录", paused: "已暂停", overloaded: "已停止新增：队列或历史容量已满", failed: "存储或抓取异常", stopping: "正在退出", startup: "正在启动" })[status.state];
  const retry = status?.services.storage === "failed" && status.queue.items > 0;
  const copyHasText = view.summaries.find(item => item.id === view.selectedId)?.format_names.includes("CF_UNICODETEXT") ?? false;
  return (
    <main className="shell history-shell" onCompositionStartCapture={event => { if (panel) { input.compositionStart(compositionQuery(event.target)); newInput(); } }} onCompositionUpdateCapture={event => { if (panel) { input.compositionUpdate(compositionQuery(event.target)); newInput(); } }} onCompositionEndCapture={event => { if (panel) { const editor = searchRef.current; const query = input.compositionEnd(event.target === editor ? event.data : "", event.target === editor ? editor?.value : undefined); if (query !== null) updateQuery(query); } }} onKeyUpCapture={event => {
      if (!panel) return;
      const target = event.target as HTMLElement, ctrl = control(target);
      const action = ctrl?.action ?? (browsing && target.closest(".history-list") ? event.key === "Enter" ? "copy" : event.key === "Escape" ? "hide" : null : null);
      const accepted = input.keyUp(event.nativeEvent, intent(action, ctrl?.target ?? target));
      if (action) { event.preventDefault(); event.stopPropagation(); }
      if (accepted) perform(accepted);
    }} onKeyDownCapture={event => {
      if (!panel) return;
      const key = event.key, ime = isImeKey(event.nativeEvent);
      const target = event.target as HTMLElement;
      if (ime) newInput();
      const ctrl = control(target);
      const action = ctrl?.action ?? (browsing && target.closest(".history-list") ? key === "Enter" ? "copy" : key === "Escape" ? "hide" : null : null);
      input.keyDown(event.nativeEvent, intent(action, ctrl?.target ?? target), compositionQuery(event.target));
      if (action && (key === "Enter" || key === " " || key === "Escape")) {
        event.preventDefault(); event.stopPropagation(); return;
      }
      if (target.id === "history-search") {
        if (key === "Enter" && !ime) event.preventDefault();
        return;
      }
      if (ime || event.repeat || closing) { if (target.closest("button")) event.preventDefault(); event.stopPropagation(); return; }
      if (browsing && (key === "ArrowUp" || key === "ArrowDown") && target.closest(".history-list") !== null) {
        input.invalidate();
        event.preventDefault(); const index = view.summaries.findIndex(item => item.id === view.selectedId);
        const next = index < 0 ? (key === "ArrowDown" ? 0 : view.summaries.length - 1) : Math.max(0, Math.min(view.summaries.length - 1, index + (key === "ArrowDown" ? 1 : -1)));
        if (view.summaries[next]) model.selectClip(view.summaries[next].id);
      }
    }} onPointerDownCapture={event => {
      if (!panel) return;
      const ctrl = control(event.target);
      input.pointerDown(event.nativeEvent, ctrl ? intent(ctrl.action, ctrl.target) : null);
    }} onPointerUpCapture={event => {
      if (!panel) return;
      const ctrl = control(event.target);
      input.pointerUp(event.nativeEvent, ctrl ? intent(ctrl.action, ctrl.target) : null);
    }} onBlurCapture={event => input.focusLeft(event.target)} onPointerCancelCapture={() => input.invalidate()} onClickCapture={event => {
      if (!panel) return;
      const ctrl = control(event.target);
      if (!ctrl) return;
      event.preventDefault(); event.stopPropagation();
      if (input.takeClick(event.detail, intent(ctrl.action, ctrl.target))) perform(ctrl.action);
    }}>
      <header><h1>{panel ? "片羽 · 快捷面板" : "片羽"}</h1><p className="tagline">{panel ? browsing ? "上下键选择，Enter 复制，Esc 隐藏；复制后手动粘贴" : "输入搜索词，选择条目查看正文；明确复制后手动粘贴" : "本地剪贴板历史"}</p>
        {panel ? <button type="button" data-hide-action="true" disabled={!actionsAllowed || (input.imeSeen && !browsing)}>隐藏快捷面板</button> : <><button type="button" disabled={closing || !panelControl.available || panelState.snapshot?.phase === "stopping" || panelState.snapshot?.phase === "unavailable"} onClick={() => void panelState.open()}>打开快捷面板</button><p role="status">{panelState.snapshot?.hotkey === "ready" ? "快捷键：Ctrl+Alt+V" : "可使用主窗口入口打开快捷面板。"}</p>{panelState.snapshot?.error && <p role="alert">{panelState.snapshot.error}</p>}{panelState.error && <p role="alert">{panelState.error}</p>}<button type="button" disabled={!residency.snapshot || residency.pending || residency.snapshot.phase === "stopping" || residency.snapshot.phase === "exited"} onClick={() => void residency.exit()}>{residency.snapshot?.phase === "failed" ? "重试退出片羽" : "退出片羽"}</button>{residency.snapshot?.phase === "stopping" && <p role="status">正在退出片羽，等待保存和资源回收。</p>}{residency.snapshot?.phase === "failed" && <p role="alert">退出未完成，仍有 {view.status?.queue.items ?? "待确认"} 条未保存内容；收录和复制保持关闭。</p>}{residency.snapshot?.error && <p role="alert">{residency.snapshot.error}</p>}{residency.error && <p role="alert">{residency.error}</p>}{!residencyControl.available && <p role="status">当前环境未提供驻留与退出控制。</p>}</>}
      </header>
      {view.hostMissing ? <p className="notice" role="status">需要桌面宿主。请从片羽桌面程序打开此界面。</p> : <>
        <section className="status-card" aria-label="收录状态">
          <div className="toolbar"><p role="status" aria-live="polite">{label}</p>
            {!panel && <button disabled={closing || !status || view.changing || (!status.capture.user_enabled && settingsBlocked) || (status.state === "stopping" && !retry)} onClick={() => void (retry ? model.toggle() : model.setEnabled(!status?.capture.user_enabled))}>{view.changing ? "正在更改…" : retry ? "重试保存" : status?.capture.user_enabled ? "暂停收录" : "开始收录"}</button>}
            {!panel && residency.snapshot?.phase === "failed" && retry && <button disabled={residency.pending} onClick={() => void residency.retrySaving().then(() => model.refresh(false))}>仅重试保存</button>}
          </div>
          {status && <><dl className="counts"><div><dt>已存历史</dt><dd>{status.history.items} 条</dd></div><div><dt>未保存队列</dt><dd>{status.queue.items} 条 · {Math.ceil(status.queue.bytes / 1024)} KiB</dd></div><div><dt>已忽略 / 已覆盖</dt><dd>{status.counters.ignored} / {status.counters.superseded}</dd></div><div><dt>已取消</dt><dd>{status.counters.cancelled}</dd></div></dl><details><summary>数据目录</summary><p className="data-path">{status.data_dir}</p></details></>}
          {status?.state === "failed" && <p className="error" role="alert">{status.queue.items > 0 ? "仍有未保存内容。可重试保存，退出未完成时窗口会保留。" : "服务暂不可用，请检查隔离目录或重启程序。"}</p>}
          {!panel && <p className="notice">暂停确认后，尚未保存的旧收录任务会取消；已保存历史仍可浏览和复制。重试保存用于保留的失败队列，与开始收录不同。</p>}
        </section>
        {!panel && <ExclusionSettings control={settingsControl} closing={closing} onApplied={() => void model.refresh(false)} onBlocked={setSettingsBlocked} />}
        {!panel && <HistoryManagement control={nativeManagement} ready={!closing && !!status && !status.capture.user_enabled && status.queue.items === 0 && status.services.storage === "ready" && !view.copyBusy && !view.loading} items={status?.history.items ?? 0} selectedId={view.selectedId} onChanged={() => { model.selectClip(null); void model.refresh(); }} />}
        {view.error && <p className="error" role="alert">{view.error}</p>}
        <section aria-labelledby="history-heading" aria-busy={view.loading}>
          <div className="toolbar"><h2 id="history-heading">最近历史</h2><button disabled={!actionsAllowed || view.loading} onClick={() => { if (actionsAllowed) void model.refresh(); }}>刷新</button></div>
          <div className="search-bar">
            <label className="search-label" htmlFor="history-search">搜索历史正文</label>
            <input ref={searchRef} id="history-search" className="search-input" type="search" value={view.query} onChange={event => updateQuery(event.target.value)} autoComplete="off" />
            <button type="button" disabled={!actionsAllowed} onClick={() => { updateQuery(""); if (panel) searchRef.current?.focus(); }}>清空搜索</button>
            {panel && (browsing ? <button key="return-search" type="button" onClick={returnToSearch}>返回搜索</button> : <button key="enter-browse" type="button" disabled={!actionsAllowed || view.loading || !!view.error || view.summaries.length === 0} onClick={() => enterBrowse()}>浏览结果</button>)}
          </div>
          {panel && <p className="notice input-status" role="status">{view.loading ? "正在搜索，结果更新后可查看。" : view.error ? "暂时无法浏览，请刷新后重试。" : view.summaries.length === 0 ? "当前没有可浏览的结果。" : `结果对应搜索词：${view.query || "全部历史"}。选择条目可查看正文，复制需要另一次明确操作。`}</p>}
          {view.loading && <p role="status">正在读取历史…</p>}
          {!view.loading && !view.error && view.summaries.length === 0 && (searching
            ? <p className="notice" role="status">没有匹配的历史正文。</p>
            : status?.services.storage === "ready" && <p className="notice" role="status">暂无历史。开始收录后，在其他应用中复制内容。</p>)}
          <ol ref={listRef} className="history-list" tabIndex={panel ? browsing ? 0 : -1 : undefined} aria-label="历史条目">{view.summaries.map(item => <HistoryItem key={item.id} item={item} selected={view.selectedId === item.id} disabled={!actionsAllowed || view.loading} onSelect={id => panel ? enterBrowse(id) : model.selectClip(id)} />)}</ol>
          {view.cursor && <button disabled={!actionsAllowed || view.loading} onClick={() => { if (actionsAllowed) void model.more(); }}>加载更多</button>}
        </section>
        <section className="preview-card" aria-labelledby="preview-heading">
          <div className="toolbar">
            <h2 id="preview-heading">正文预览</h2>
            <button type="button" data-copy-action="true" disabled={!actionsAllowed || (!!panel && !browsing) || !view.copyAvailable || view.copyBusy || view.loading || !copyHasText || !status || status.state === "stopping" || status.services.storage !== "ready"} onClick={copyText}>{view.copyBusy ? "复制中…" : "复制纯文本"}</button>
            {view.selectedId && <button type="button" disabled={!actionsAllowed} onClick={() => { if (!actionsAllowed) return; model.selectClip(null); if (browsing) listRef.current?.focus(); }}>关闭正文预览</button>}
          </div>
          {!view.copyAvailable && <p className="notice">当前宿主不支持复制纯文本。</p>}
          {view.copyMessage && <p role="status" aria-live="polite">{view.copyMessage}</p>}
          {!view.selectedId ? <p className="notice">从列表中选择一条历史以预览正文。</p> : <>
            {view.previewLoading && <p role="status">正在读取正文…</p>}
            {view.previewError && <p className="error" role="alert">{view.previewError}</p>}
            {view.previewText !== "" && <pre className="preview-body">{view.previewText}</pre>}
            {view.previewError
              ? <button type="button" onClick={() => (view.previewNextOffset === null ? model.retryPreview() : model.morePreview())}>{view.previewNextOffset === null ? "重试读取正文" : "重试继续读取正文"}</button>
              : view.previewNextOffset !== null
                ? <button type="button" disabled={view.previewLoading} onClick={() => model.morePreview()}>继续读取正文</button>
                : !view.previewLoading && (view.previewText === ""
                  ? <p className="notice">无可预览正文。</p>
                  : <p className="notice" role="status">读取完毕。</p>)}
          </>}
        </section>
        <p className="notice">列表只显示已提交的历史；未保存队列单独计数。隐私标记、内部写回及应用排除会跳过收录。复制后请自行切回目标应用粘贴。</p>
      </>}
    </main>
  );
}
