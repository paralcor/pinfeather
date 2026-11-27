// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "./App";
import { parseStartupCaptureSnapshot, type StartupCaptureControl, type StartupCaptureSnapshot } from "./lib/startup-capture";
import { createHistoryModel } from "./lib/history-store";
import startupTimeoutSnapshots from "../tests/fixtures/contracts/startup_timeout_snapshots.json";
import type { DesktopBridge } from "./lib/desktop";
import { parseStatusResult } from "./lib/ipc";
import statusFixture from "../tests/fixtures/contracts/response_dtos.json";

afterEach(cleanup);

const initial: StartupCaptureSnapshot = {
  revision: "1", enabled: false, blocked: false, outcome: "default_paused", reason: null, error: null,
};
const fixture = (snapshot = initial) => {
  let revision = 1;
  return {
    available: true,
    snapshot: vi.fn(async () => snapshot),
    subscribe: vi.fn(async (_callback: () => void) => () => {}),
    setEnabled: vi.fn(async (enabled: boolean) => ({ ...snapshot, revision: String(++revision), enabled })),
  } satisfies StartupCaptureControl;
};
const bridge: DesktopBridge = {
  available: true,
  async status() {
    const status = parseStatusResult(statusFixture.status_cases.find(entry => entry.id === "st_ok")!.value);
    if (!status.ok) throw new Error("invalid fixture");
    return status.value;
  },
  async list(request_id, _cursor, normalized_query = "") { return { request_id, normalized_query, summaries: [], next_cursor: null }; },
  getText: vi.fn(), setEnabled: vi.fn(),
  async subscribe() { return () => {}; },
};

it.each([true, false])("lets Pause supersede an unsettled Start (Start resolves first: %s)", async startFirst => {
  const sample = await bridge.status();
  const paused = { ...sample, state: "paused" as const, capture: { ...sample.capture, user_enabled: false, admission_open: false } };
  const listening = { ...sample, state: "listening" as const, capture: { ...sample.capture, user_enabled: true, admission_open: true } };
  let status: typeof sample = { ...paused, revision: "1" };
  let statusChanged!: () => void;
  let finishStart!: (value: typeof sample) => void;
  let finishPause!: (value: typeof sample) => void;
  const start = new Promise<typeof sample>(resolve => { finishStart = resolve; });
  const pause = new Promise<typeof sample>(resolve => { finishPause = resolve; });
  const setEnabled = vi.fn((enabled: boolean) => enabled ? start : pause);
  render(<App bridge={{
    ...bridge,
    status: async () => status,
    setEnabled,
    async subscribe(name, callback) {
      if (name === "status_changed") statusChanged = callback;
      return () => {};
    },
  }} />);
  fireEvent.click(await screen.findByRole("button", { name: "开始收录" }));
  expect(setEnabled).toHaveBeenNthCalledWith(1, true);
  expect((screen.getByRole("button", { name: "正在更改…" }) as HTMLButtonElement).disabled).toBe(true);
  status = { ...listening, revision: "2" };
  await act(async () => statusChanged());
  const pauseButton = screen.getByRole("button", { name: "暂停收录" }) as HTMLButtonElement;
  expect(pauseButton.disabled).toBe(false);
  fireEvent.click(pauseButton);
  expect(setEnabled).toHaveBeenNthCalledWith(2, false);
  status = { ...paused, revision: "3" };
  await act(async () => statusChanged());
  if (startFirst) {
    await act(async () => finishStart({ ...listening, revision: "4" }));
    expect((screen.getByRole("button", { name: "正在更改…" }) as HTMLButtonElement).disabled).toBe(true);
  }
  await act(async () => finishPause(status));
  if (!startFirst) await act(async () => finishStart({ ...listening, revision: "4" }));
  expect((screen.getByRole("button", { name: "开始收录" }) as HTMLButtonElement).disabled).toBe(false);
});

it("keeps Retry Save disabled until the command settles, even if storage has recovered", async () => {
  const sample = await bridge.status();
  const failed = {
    ...sample, revision: "1", state: "failed" as const,
    capture: { ...sample.capture, user_enabled: true, admission_open: false },
    services: { ...sample.services, storage: "failed" as const },
    queue: { items: 1, bytes: 4 },
  };
  const recovered = { ...sample, revision: "2" };
  let status: typeof sample = failed;
  let statusChanged!: () => void;
  let finish!: (value: typeof sample) => void;
  const setEnabled = vi.fn((_enabled: boolean) => new Promise<typeof sample>(resolve => { finish = resolve; }));
  render(<App bridge={{
    ...bridge, status: async () => status, setEnabled,
    async subscribe(name, callback) {
      if (name === "status_changed") statusChanged = callback;
      return () => {};
    },
  }} />);

  fireEvent.click(await screen.findByRole("button", { name: "重试保存" }));
  for (const next of [failed, recovered]) {
    status = next;
    await act(async () => statusChanged());
    expect(screen.queryByRole("button", { name: "暂停收录" })).toBeNull();
    const changing = screen.getByRole("button", { name: "正在更改…" }) as HTMLButtonElement;
    expect(changing.disabled).toBe(true);
    fireEvent.click(changing);
    expect(setEnabled.mock.calls).toEqual([[true]]);
  }
  await act(async () => finish(recovered));
  expect((screen.getByRole("button", { name: "暂停收录" }) as HTMLButtonElement).disabled).toBe(false);
});

async function recoveryFixture() {
  const sample = await bridge.status();
  const failed = {
    ...sample, revision: "1", state: "failed" as const,
    capture: { ...sample.capture, user_enabled: true, admission_open: false },
    services: { ...sample.services, storage: "failed" as const, worker: "failed" as const },
    queue: { items: 2, bytes: 8 },
  };
  const saving = { ...failed, revision: "2", services: { ...failed.services, storage: "ready" as const }, queue: { items: 1, bytes: 4 } };
  let status: typeof sample = failed;
  let statusChanged!: () => void;
  const setEnabled = vi.fn(async (enabled: boolean) => {
    if (enabled) status = saving;
    return status;
  });
  return {
    saving, setEnabled,
    bridge: {
      ...bridge, status: async () => status, setEnabled,
      async subscribe(name, callback) {
        if (name === "status_changed") statusChanged = callback;
        return () => {};
      },
    } satisfies DesktopBridge,
    publish(next: typeof sample) { status = next; statusChanged(); },
  };
}

function afterRecovery(saving: Awaited<ReturnType<typeof recoveryFixture>>["saving"], admissionOpen: boolean) {
  return {
    ...saving, revision: "3",
    state: admissionOpen ? "listening" as const : saving.state,
    capture: { ...saving.capture, admission_open: admissionOpen },
    services: { ...saving.services, worker: admissionOpen ? "ready" as const : saving.services.worker },
    queue: admissionOpen ? saving.queue : { items: 0, bytes: 0 },
  };
}

it.each([false, true])("blocks model pause and toggle while retry drains the queue (admission reopens: %s)", async admissionOpen => {
  const recovery = await recoveryFixture();
  const model = createHistoryModel(recovery.bridge);
  const stop = model.start();
  try {
    await waitFor(() => expect(model.store.getState().status?.revision).toBe("1"));
    await model.toggle();
    expect(model.store.getState()).toMatchObject({ status: recovery.saving, changing: false });
    await model.setEnabled(false);
    await model.toggle();
    expect(recovery.setEnabled.mock.calls).toEqual([[true]]);

    recovery.publish(afterRecovery(recovery.saving, admissionOpen));
    await waitFor(() => expect(model.store.getState().status?.revision).toBe("3"));
    await model.setEnabled(false);
    expect(recovery.setEnabled.mock.calls).toEqual([[true], [false]]);
  } finally { stop(); }
});

it.each([false, true])("keeps the capture button disabled after Retry Save returns (admission reopens: %s)", async admissionOpen => {
  const recovery = await recoveryFixture();
  render(<App bridge={recovery.bridge} />);
  const retry = await screen.findByRole("button", { name: "重试保存" });
  await act(async () => { fireEvent.click(retry); });

  expect(screen.queryByRole("button", { name: "暂停收录" })).toBeNull();
  const saving = screen.getByRole("button", { name: "正在更改…" }) as HTMLButtonElement;
  expect(saving.disabled).toBe(true);
  fireEvent.click(saving);
  expect(recovery.setEnabled.mock.calls).toEqual([[true]]);

  await act(async () => recovery.publish(afterRecovery(recovery.saving, admissionOpen)));
  const pause = screen.getByRole("button", { name: "暂停收录" }) as HTMLButtonElement;
  expect(pause.disabled).toBe(false);
  fireEvent.click(pause);
  expect(recovery.setEnabled.mock.calls).toEqual([[true], [false]]);
});

it.each(startupTimeoutSnapshots)("shows the host timeout for $outcome once", async snapshot => {
  const control = fixture();
  control.snapshot.mockImplementation(async () => {
    const parsed = parseStartupCaptureSnapshot(snapshot);
    if (!parsed) throw new Error("invalid startup capture snapshot");
    return parsed;
  });
  render(<App bridge={bridge} startupCaptureControl={control} />);
  const hostMessage = "启动时核对历史存储或启动恢复配置超时。片羽保持暂停，请重新启动。";
  expect(await screen.findByText(hostMessage)).toBeTruthy();
  expect(screen.getAllByText(hostMessage)).toHaveLength(1);
  expect(screen.queryByText("无法读取启动恢复设置。")).toBeNull();
  const box = screen.getByRole("checkbox", { name: "启动时恢复上次手动收录状态" }) as HTMLInputElement;
  expect(box.disabled).toBe(true);
});

it("explicitly opts the current data directory in and clears the preference on opt-out", async () => {
  const control = fixture();
  render(<App bridge={bridge} startupCaptureControl={control} />);
  const box = await screen.findByRole("checkbox", { name: "启动时恢复上次手动收录状态" }) as HTMLInputElement;
  expect(box.checked).toBe(false);
  fireEvent.click(box);
  await screen.findByText("已开启启动恢复。");
  expect(box.checked).toBe(true);
  fireEvent.click(box);
  await screen.findByText("当前目录默认在启动时暂停收录。");
  expect(control.setEnabled.mock.calls).toEqual([[true], [false]]);
});

it("shows startup outcomes and disables changing a damaged preference", async () => {
  const control = fixture({
    ...initial, blocked: true, outcome: "blocked", reason: "启动恢复配置不可用。", error: "原文件已保留。",
  });
  render(<App bridge={bridge} startupCaptureControl={control} />);
  const box = await screen.findByRole("checkbox", { name: "启动时恢复上次手动收录状态" }) as HTMLInputElement;
  expect(box.disabled).toBe(true);
  expect(screen.getByText("启动恢复配置不可用。")).toBeTruthy();
  expect(screen.getByText("原文件已保留。")).toBeTruthy();
});

it("clears the read error when a startup change prompts a successful refresh", async () => {
  const control = fixture({ ...initial, enabled: true, revision: "2" });
  control.snapshot.mockRejectedValueOnce(new Error("snapshot temporarily unavailable"));
  render(<App bridge={bridge} startupCaptureControl={control} />);
  await screen.findByText("无法读取启动恢复设置。");

  await act(async () => { control.subscribe.mock.calls[0][0](); });

  expect(screen.getByText("已开启启动恢复。")).toBeTruthy();
  expect(screen.queryByText("无法读取启动恢复设置。")).toBeNull();
});

it("can retry the first preference read without a change event", async () => {
  const control = fixture();
  control.snapshot.mockRejectedValueOnce(new Error("snapshot temporarily unavailable"));
  let complete!: (snapshot: StartupCaptureSnapshot) => void;
  control.snapshot.mockImplementationOnce(() => new Promise(resolve => { complete = resolve; }));
  render(<App bridge={bridge} startupCaptureControl={control} />);
  await screen.findByText("无法读取启动恢复设置。");
  const box = screen.getByRole("checkbox", { name: "启动时恢复上次手动收录状态" }) as HTMLInputElement;
  expect(box.disabled).toBe(true);
  expect(screen.queryByText("正在读取启动恢复设置…")).toBeNull();
  const retry = screen.getByRole("button", { name: "重试读取" }) as HTMLButtonElement;
  fireEvent.click(retry);
  expect(retry.disabled).toBe(true);
  fireEvent.click(retry);
  expect(control.snapshot).toHaveBeenCalledTimes(2);
  await act(async () => complete(initial));
  expect(box.disabled).toBe(false);
  expect(screen.queryByText("无法读取启动恢复设置。")).toBeNull();
  expect(screen.queryByRole("button", { name: "重试读取" })).toBeNull();
});

it.each([
  ["forced_paused", "本次启动使用了 --start-paused，已强制暂停。"],
  ["restored", "本次启动已恢复收录。"],
] as const)("renders %s independently of the saved opt-in", async (outcome, message) => {
  render(<App bridge={bridge} startupCaptureControl={fixture({ ...initial, enabled: true, outcome })} />);
  expect(await screen.findByText(message)).toBeTruthy();
});

it("keeps the startup preference out of the quick panel", async () => {
  const control = fixture();
  render(<App bridge={bridge} panel={{ hide() {} }} startupCaptureControl={control} />);
  await screen.findByText("正在收录");
  expect(screen.queryByRole("checkbox", { name: "启动时恢复上次手动收录状态" })).toBeNull();
  expect(control.snapshot).not.toHaveBeenCalled();
});
