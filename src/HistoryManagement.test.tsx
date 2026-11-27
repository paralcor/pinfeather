// @vitest-environment jsdom
import { afterEach, it, expect, vi } from "vitest";
import { cleanup, render, screen, fireEvent } from "@testing-library/react";
import { HistoryManagement } from "./HistoryManagement";
import type { ManagementControl } from "./lib/history-management";
afterEach(cleanup);
function setup(ready=true) { const control:ManagementControl={available:true,run:vi.fn(async()=>({items:1,favorites:1,deleted:1,backup_path:null}))}; const changed=vi.fn(); render(<HistoryManagement control={control} ready={ready} items={2} favorites={1} historyRevision="4" selectedFavorited={false} selectedId="12" onChanged={changed}/>);return {control,changed}; }
it("requires a second explicit confirmation and preserves selected ID",async()=>{const f=setup();fireEvent.click(screen.getByRole("button",{name:"删除选中条目"}));expect(f.control.run).not.toHaveBeenCalled();fireEvent.click(screen.getByRole("button",{name:"确认删除"}));await screen.findByText("已删除 1 条历史，保留 1 条已收藏历史。");expect(f.control.run).toHaveBeenCalledWith({action:"delete",clip_id:"12",expected_items:2});expect(f.changed).toHaveBeenCalledTimes(1);});
it("clear identifies affected and preserved history and supports cancel without dispatch",()=>{const f=setup();fireEvent.click(screen.getByRole("button",{name:"清空未收藏历史"}));expect(screen.getByText(/永久删除 1 条未收藏历史，保留 1 条已收藏历史/)).toBeTruthy();fireEvent.click(screen.getByRole("button",{name:"取消"}));expect(f.control.run).not.toHaveBeenCalled();});
it("requires paused empty queue readiness",()=>{const f=setup(false);fireEvent.click(screen.getByRole("button",{name:"删除选中条目"}));fireEvent.click(screen.getByRole("button",{name:"备份历史"}));expect(f.control.run).not.toHaveBeenCalled();});
it("failures never claim successful deletion",async()=>{const f=setup();vi.mocked(f.control.run).mockRejectedValueOnce(new Error("unknown"));fireEvent.click(screen.getByRole("button",{name:"删除选中条目"}));fireEvent.click(screen.getByRole("button",{name:"确认删除"}));await screen.findByRole("alert");expect(screen.queryByText("已删除 1 条历史，保留 1 条已收藏历史。")).toBeNull();expect(f.control.run).toHaveBeenCalledTimes(1);});

it("selection change invalidates an old confirmation",()=>{const control:ManagementControl={available:true,run:vi.fn()};const props={control,ready:true,items:2,favorites:1,historyRevision:"4",selectedFavorited:false,onChanged:vi.fn()};const v=render(<HistoryManagement {...props} selectedId="12"/>);fireEvent.click(screen.getByRole("button",{name:"删除选中条目"}));v.rerender(<HistoryManagement {...props} selectedId="13"/>);v.rerender(<HistoryManagement {...props} selectedId="12"/>);expect(screen.queryByRole("button",{name:"确认删除"})).toBeNull();expect(control.run).not.toHaveBeenCalled();});
it("duplicate clicks dispatch once while result is pending",()=>{const control:ManagementControl={available:true,run:vi.fn(()=>new Promise<never>(()=>{}))};render(<HistoryManagement control={control} ready items={2} favorites={1} historyRevision="4" selectedFavorited={false} selectedId="12" onChanged={vi.fn()}/>);const button=screen.getByRole("button",{name:"备份历史"});fireEvent.click(button);fireEvent.click(button);expect(control.run).toHaveBeenCalledTimes(1);});

it("protects a selected favorite and shows the same notice for a cross-window race", async () => {
  const control: ManagementControl = { available: true, run: vi.fn() };
  const props = { control, ready: true, items: 2, favorites: 1, historyRevision: "4", selectedId: "12", onChanged: vi.fn() };
  const mounted = render(<HistoryManagement {...props} selectedFavorited />);
  const button = screen.getByRole<HTMLButtonElement>("button", { name: "删除选中条目" });
  expect(button.disabled).toBe(true);
  const message = "已收藏条目受保护，请先取消收藏后再删除。";
  expect(screen.getByText(message)).toBeTruthy();
  mounted.rerender(<HistoryManagement {...props} selectedFavorited={false} />);
  const error = { code: "invalid_input", retryable: false, message: "clip is favorited; unfavorite before deleting" };
  vi.mocked(control.run).mockRejectedValueOnce(error);
  fireEvent.click(button);
  fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
  expect((await screen.findByRole("alert")).textContent).toBe(message);
  expect(props.onChanged).not.toHaveBeenCalled();
});

it("invalidates clear on a same-count revision and sends the renewed confirmation", async () => {
  const control: ManagementControl = { available: true, run: vi.fn(async () => ({ items: 1, favorites: 1, deleted: 1, backup_path: null })) };
  const props = { control, ready: true, items: 2, favorites: 1, selectedId: null, selectedFavorited: false, onChanged: vi.fn() };
  const mounted = render(<HistoryManagement {...props} historyRevision="4" />);
  const clear = screen.getByRole<HTMLButtonElement>("button", { name: "清空未收藏历史" });
  fireEvent.click(clear);
  mounted.rerender(<HistoryManagement {...props} historyRevision="6" />);
  expect(screen.queryByRole("button", { name: "确认删除" })).toBeNull();
  expect(control.run).not.toHaveBeenCalled();
  fireEvent.click(clear);
  fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
  await screen.findByText("已删除 1 条历史，保留 1 条已收藏历史。");
  expect(control.run).toHaveBeenCalledExactlyOnceWith({ action: "clear", expected_items: 2, expected_history_revision: "6" });
  mounted.rerender(<HistoryManagement {...props} items={1} historyRevision="7" />);
  expect(clear.disabled).toBe(true);
});
