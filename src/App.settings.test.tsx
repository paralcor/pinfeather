// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import statusFixture from "../tests/fixtures/contracts/response_dtos.json";
import { parseStatusResult } from "./lib/ipc";
import type { DesktopBridge } from "./lib/desktop";
import type { SettingsControl, SettingsSnapshot } from "./lib/settings";
afterEach(cleanup);
function fixture() {
  const parsed = parseStatusResult(statusFixture.status_cases.find(item => item.id === "st_ok")!.value); if (!parsed.ok) throw new Error("fixture");
  const status = { ...parsed.value, state: "paused" as const, capture: { user_enabled: false, admission_open: false, policy_epoch: 0 } };
  const bridge: DesktopBridge = { available: true, status: vi.fn(async () => status), list: vi.fn(async request_id => ({ request_id, normalized_query: "", summaries: [], next_cursor: null })), getText: vi.fn(), copyText: vi.fn(), setEnabled: vi.fn(async () => status), subscribe: vi.fn(async () => () => {}) };
  const snapshot: SettingsSnapshot = { revision: "1", schema_version: 1, excluded_apps: [], saving: false, blocked: false, error: null, last_skip: null };
  let revision = 1;
  const control: SettingsControl = { available: true, snapshot: vi.fn(async () => snapshot), subscribe: vi.fn(async () => () => {}), apply: vi.fn(async paths => ({ ...snapshot, revision: String(++revision), excluded_apps: paths })) };
  return { bridge, control, snapshot };
}
it("m2 settings main can add normalize remove and explicitly apply persistent rules", async () => {
  const f = fixture(); render(<App bridge={f.bridge} settingsControl={f.control} />);
  const input = await screen.findByLabelText("Windows 完整 exe 路径"); await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
  fireEvent.change(input, { target: { value: "c:/Private/old/../vault.exe" } }); fireEvent.click(screen.getByRole("button", { name: "添加排除路径" }));
  expect(screen.getByText("C:\\Private\\vault.exe")).toBeTruthy(); expect(f.control.apply).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "应用排除设置" })); await screen.findByText("排除设置已保存并生效。"); expect(f.control.apply).toHaveBeenCalledWith(["C:\\Private\\vault.exe"]);
  fireEvent.click(screen.getByRole("button", { name: "移除 C:\\Private\\vault.exe" })); fireEvent.click(screen.getByRole("button", { name: "应用排除设置" })); await screen.findByText("已应用 0 条排除路径。"); expect(f.control.apply).toHaveBeenLastCalledWith([]);
  expect(screen.getByText(/快照后、保存历史前过滤/)).toBeTruthy(); expect(screen.getByText(/暂停确认后/)).toBeTruthy();
});
it("m2 settings invalid path, failed save and double click never display success", async () => {
  const f = fixture(); let reject!: (error: unknown) => void; f.control.apply = vi.fn(() => new Promise<SettingsSnapshot>((_resolve, fail) => { reject = fail; })); render(<App bridge={f.bridge} settingsControl={f.control} />);
  const input = await screen.findByLabelText("Windows 完整 exe 路径"); await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
  fireEvent.change(input, { target: { value: "vault.exe" } }); fireEvent.click(screen.getByRole("button", { name: "添加排除路径" })); await screen.findByText(/请填写 Windows 完整 exe 路径/); expect(f.control.apply).not.toHaveBeenCalled();
  fireEvent.change(input, { target: { value: "C:\\vault.exe" } }); fireEvent.click(screen.getByRole("button", { name: "添加排除路径" })); const apply = screen.getByRole("button", { name: "应用排除设置" }); fireEvent.click(apply); fireEvent.click(apply); expect(f.control.apply).toHaveBeenCalledTimes(1);
  await act(async () => reject({ code: "storage_unavailable" })); await screen.findByText(/设置未确认生效/); expect(screen.queryByText("排除设置已保存并生效。")).toBeNull(); expect(screen.getByText("C:\\vault.exe")).toBeTruthy(); expect((screen.getByRole("button", { name: "开始收录" }) as HTMLButtonElement).disabled).toBe(true);
});
it("m2 settings broken startup blocks capture, and quick panel has no management controls", async () => {
  const f = fixture(); f.control.snapshot = vi.fn(async () => ({ ...f.snapshot, blocked: true, error: "invalid settings" })); const rendered = render(<App bridge={f.bridge} settingsControl={f.control} />);
  await screen.findByText("invalid settings"); expect((screen.getByRole("button", { name: "开始收录" }) as HTMLButtonElement).disabled).toBe(true); expect(f.bridge.setEnabled).not.toHaveBeenCalled(); rendered.unmount();
  render(<App bridge={f.bridge} settingsControl={f.control} panel={{ hide: vi.fn() }} />); await screen.findByRole("heading", { name: "片羽 · 快捷面板" }); expect(screen.queryByLabelText("Windows 完整 exe 路径")).toBeNull(); expect(screen.queryByRole("button", { name: "应用排除设置" })).toBeNull();
});
it("m2 settings does not offer capture before the persistent policy has been confirmed", async () => {
  const f = fixture(); let resolve!: (value: SettingsSnapshot) => void; f.control.snapshot = vi.fn(() => new Promise<SettingsSnapshot>(r => { resolve = r; })); render(<App bridge={f.bridge} settingsControl={f.control} />);
  await waitFor(() => expect(f.control.snapshot).toHaveBeenCalledTimes(1)); const start = screen.getByRole("button", { name: "开始收录" }) as HTMLButtonElement;
  expect(start.disabled).toBe(true); fireEvent.click(start); expect(f.bridge.setEnabled).not.toHaveBeenCalled();
  await act(async () => resolve(f.snapshot)); await waitFor(() => expect(start.disabled).toBe(false));
});
