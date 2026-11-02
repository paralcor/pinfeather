// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import fixture from "../../tests/fixtures/contracts/settings.json";
import { parseSetExcludedAppsRequest } from "./ipc";
import { acceptSettings, parseSettingsSnapshot, useSettings, type SettingsSnapshot, type SettingsControl } from "./settings";
afterEach(cleanup);
const state = (revision = "1", paths: string[] = []): SettingsSnapshot => ({ revision, schema_version: 1, excluded_apps: paths, saving: false, blocked: false, error: null, last_skip: null });
function control() { return { available: true, snapshot: vi.fn(async () => state()), subscribe: vi.fn<SettingsControl["subscribe"]>(async () => () => {}), apply: vi.fn<SettingsControl["apply"]>(async paths => state("2", paths)) }; }
it("m2 settings shares strict request and snapshot fixtures with Rust", () => {
  expect(parseSettingsSnapshot(fixture.snapshot)).toEqual(fixture.snapshot);
  for (const value of fixture.invalid_snapshots) expect(parseSettingsSnapshot(value)).toBeNull();
  for (const value of [null, [], {}, { ...fixture.snapshot, revision: "18446744073709551616" }, { ...fixture.snapshot, excluded_apps: Array.from({ length: 33 }, () => "C:\\p.exe") }]) expect(parseSettingsSnapshot(value)).toBeNull();
  for (const test of fixture.requests) { const parsed = parseSetExcludedAppsRequest(test.value); if (test.expected === null) expect(parsed.ok).toBe(false); else expect(parsed).toEqual({ ok: true, value: test.expected }); }
});
it("m2 settings rejects older and duplicate revisions", () => {
  const current = state("9", ["C:\\new.exe"]); expect(acceptSettings(current, state("2"))).toBe(current); expect(acceptSettings(current, state("9"))).toBe(current); expect(acceptSettings(current, state("10"))).toEqual(state("10"));
});
it("m2 settings coalesces double apply and keeps the confirmed normalized result", async () => {
  const c = control(); let resolve!: (value: SettingsSnapshot) => void; c.apply.mockImplementation(() => new Promise(r => { resolve = r; }));
  const { result } = renderHook(() => useSettings(c, true)); await waitFor(() => expect(result.current.snapshot).toEqual(state()));
  let first!: Promise<boolean>; act(() => { first = result.current.apply(["c:/Private/old/../vault.exe"]); void result.current.apply(["C:\\second.exe"]); });
  expect(c.apply).toHaveBeenCalledTimes(1); expect(c.apply).toHaveBeenCalledWith(["C:\\Private\\vault.exe"]); expect(result.current.pending).toBe(true);
  await act(async () => { resolve(state("2", ["C:\\Private\\vault.exe"])); expect(await first).toBe(true); }); expect(result.current.pending).toBe(false);
});
it("m2 settings validation and save failure retain the draft and show no success", async () => {
  const c = control(); const { result } = renderHook(() => useSettings(c, true)); await waitFor(() => expect(result.current.snapshot).not.toBeNull());
  await act(async () => expect(await result.current.apply(["vault.exe"])).toBe(false)); expect(c.apply).not.toHaveBeenCalled(); expect(result.current.error).toMatch(/完整 exe/);
  c.apply.mockRejectedValue({ code: "storage_unavailable" }); c.snapshot.mockResolvedValue({ ...state("3"), blocked: true, error: "disk state uncertain" });
  await act(async () => expect(await result.current.apply(["C:\\vault.exe"])).toBe(false)); expect(result.current.snapshot?.blocked).toBe(true); expect(result.current.error).toMatch(/未确认生效/); expect(result.current.pending).toBe(false);
});
it("m2 settings late apply response cannot replace a newer authoritative policy", async () => {
  const c = control(); let notify!: (revision: string) => void; c.subscribe.mockImplementation(async callback => { notify = callback; return () => {}; });
  let resolve!: (value: SettingsSnapshot) => void; c.apply.mockImplementation(() => new Promise(r => { resolve = r; }));
  const { result } = renderHook(() => useSettings(c, true)); await waitFor(() => expect(result.current.snapshot?.revision).toBe("1"));
  let applying!: Promise<boolean>; act(() => { applying = result.current.apply(["C:\\old.exe"]); }); c.snapshot.mockResolvedValue(state("9", ["C:\\new.exe"]));
  await act(async () => notify("9")); await waitFor(() => expect(result.current.snapshot?.revision).toBe("9"));
  await act(async () => { resolve(state("2", ["C:\\old.exe"])); expect(await applying).toBe(false); }); expect(result.current.snapshot?.excluded_apps).toEqual(["C:\\new.exe"]); expect(result.current.error).toMatch(/状态已更新/);
});
it("m2 settings old host reports unavailable and unmount disposes a late subscription", async () => {
  const c = control(); c.snapshot.mockRejectedValue(new Error("unknown command")); const { result, unmount } = renderHook(() => useSettings(c, true)); await waitFor(() => expect(result.current.error).toMatch(/宿主不支持/)); expect(await result.current.apply([])).toBe(false); expect(c.apply).not.toHaveBeenCalled(); unmount();
  const off = vi.fn(); let resolve!: (value: () => void) => void; c.subscribe.mockImplementation(() => new Promise(r => { resolve = r; })); const late = renderHook(() => useSettings(c, true)); late.unmount(); await act(async () => resolve(off)); expect(off).toHaveBeenCalledTimes(1);
});
