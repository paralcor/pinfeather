import { useEffect, useState } from "react";
import { normalizeWindowsExePath } from "./lib/ipc";
import { useSettings, type SettingsControl } from "./lib/settings";

export function ExclusionSettings({ control, closing, onApplied, onBlocked }: { control: SettingsControl; closing: boolean; onApplied(): void; onBlocked(blocked: boolean): void }) {
  const settings = useSettings(control, true);
  const [paths, setPaths] = useState<string[]>([]), [path, setPath] = useState(""), [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null), [saved, setSaved] = useState(false);
  const disabled = closing || !settings.snapshot || settings.snapshot.blocked || settings.snapshot.saving || settings.pending;
  useEffect(() => { if (settings.snapshot && !dirty) setPaths(settings.snapshot.excluded_apps); }, [settings.snapshot, dirty]);
  useEffect(() => { onBlocked(control.available && (!settings.snapshot || settings.snapshot.blocked || settings.snapshot.saving || settings.pending || settings.error !== null)); }, [control.available, settings.snapshot, settings.pending, settings.error, onBlocked]);
  function add() {
    if (disabled) return;
    const parsed = normalizeWindowsExePath(path);
    if (!parsed.ok || paths.length >= 32) { setError("请填写 Windows 完整 exe 路径；最多 32 条。"); return; }
    setPaths([...paths, parsed.value]); setPath(""); setDirty(true); setSaved(false); setError(null);
  }
  async function apply() {
    if (disabled || !dirty) return;
    setSaved(false); setError(null);
    if (await settings.apply(paths)) { setDirty(false); setSaved(true); onApplied(); }
  }
  return <section className="status-card" aria-labelledby="exclusions-heading" aria-busy={settings.pending || settings.snapshot?.saving}>
    <h2 id="exclusions-heading">应用排除</h2>
    <p className="notice">排除列表保存在当前数据目录，重启后仍生效；每次启动都暂停收录。规则在读取剪贴板快照后、保存历史前过滤，不能保证不读取剪贴板。</p>
    {!control.available && <p role="status">当前环境未提供持久排除设置。</p>}
    {control.available && !settings.snapshot && !settings.error && <p role="status">正在读取排除设置…</p>}
    {settings.snapshot?.blocked && <p role="alert">设置状态需要核对，收录已阻止；本实例不会覆盖设置文件。请退出后修正 settings.json，再重新启动。</p>}
    {settings.snapshot?.error && <p role="alert">{settings.snapshot.error}</p>}
    {settings.error && <p role="alert">{settings.error}</p>}
    <label className="search-label" htmlFor="excluded-app-path">Windows 完整 exe 路径</label>
    <div className="toolbar"><input id="excluded-app-path" className="search-input" type="text" value={path} disabled={disabled} onChange={event => { setPath(event.target.value); setError(null); }} aria-describedby="exclusion-path-help" /><button type="button" disabled={disabled || paths.length >= 32} onClick={add}>添加排除路径</button></div>
    <p id="exclusion-path-help" className="notice">例如 C:\Program Files\Private\vault.exe。最多 32 条；重复路径在应用时按 Windows 大小写规则合并。</p>
    <ul aria-label="待应用排除路径">{paths.map((item, index) => <li key={`${index}-${item}`}><span className="data-path">{item}</span> <button type="button" disabled={disabled} aria-label={`移除 ${item}`} onClick={() => { if (!disabled) { setPaths(paths.filter((_, i) => i !== index)); setDirty(true); setSaved(false); } }}>移除</button></li>)}</ul>
    <p role="status">{settings.snapshot?.blocked ? "排除状态未确认。" : dirty ? "列表有尚未应用的更改。" : settings.snapshot ? `已应用 ${settings.snapshot.excluded_apps.length} 条排除路径。` : "尚未确认已应用的规则。"}</p>
    <p className="notice">应用更改会取消尚未保存的旧收录任务，已保存历史保留。列表非空时，来源路径未知或无效的快照也会跳过。</p>
    <button type="button" disabled={disabled || !dirty} onClick={() => void apply()}>{settings.pending || settings.snapshot?.saving ? "正在保存排除设置…" : "应用排除设置"}</button>
    {error && <p role="alert">{error}</p>}{saved && !settings.snapshot?.blocked && <p role="status">排除设置已保存并生效。</p>}
    {settings.snapshot?.last_skip === "excluded_owner" && <p role="status">最近一次跳过：来源匹配应用排除规则。</p>}
    {settings.snapshot?.last_skip === "unknown_owner" && <p role="status">最近一次跳过：来源无法确认，按排除规则跳过。</p>}
  </section>;
}
