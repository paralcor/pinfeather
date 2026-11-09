// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import fixture from "../../tests/fixtures/contracts/measurement.json";
import { interactiveInput, measurementEnabled, parseIdentity, parseReceipt } from "./measurement";
afterEach(() => { document.body.replaceChildren(); vi.restoreAllMocks(); delete (globalThis as Record<string, unknown>).__PINFEATHER_MEASUREMENT__; });
it("shares strict schema1 identities and receipts with Rust", () => {
  expect(parseIdentity(fixture.identity)).toEqual(fixture.identity);
  expect(parseReceipt(fixture.receipt)).toEqual(fixture.receipt);
  for (const change of fixture.invalid_overrides) expect(() => parseReceipt({ ...fixture.receipt, ...change })).toThrow();
  for (const key of Object.keys(fixture.receipt)) { const value: Record<string, unknown> = { ...fixture.receipt }; delete value[key]; expect(() => parseReceipt(value)).toThrow(); }
});
it("is disabled unless the exact readonly host flag shape is present", () => {
  expect(measurementEnabled()).toBe(false);
  for (const value of [{ schema: 2, enabled: true }, { schema: 1, enabled: true, data_dir: "private" }, { schema: 1, enabled: "true" }]) { (globalThis as Record<string, unknown>).__PINFEATHER_MEASUREMENT__ = value; expect(measurementEnabled()).toBe(false); }
});
it("distinguishes main input availability from panel actual editor focus without focusing main", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const editor = document.createElement("input"), list = document.createElement("ol"), button = document.createElement("button");
  list.append(button); document.body.append(editor, list);
  vi.spyOn(editor, "getBoundingClientRect").mockReturnValue({ width: 100, height: 20 } as DOMRect);
  expect(document.activeElement).not.toBe(editor);
  expect(interactiveInput(editor, list, false, "nonempty", true)).toBe(true);
  expect(document.activeElement).not.toBe(editor);
  expect(interactiveInput(editor, list, true, "nonempty", true)).toBe(false);
  editor.focus(); expect(interactiveInput(editor, list, true, "nonempty", true)).toBe(true);
  button.disabled = true; expect(interactiveInput(editor, list, true, "nonempty", true)).toBe(false);
  expect(interactiveInput(editor, list, true, "empty", true)).toBe(true);
  editor.readOnly = true; expect(interactiveInput(editor, list, true, "empty", true)).toBe(false);
  editor.readOnly = false; vi.spyOn(document, "hasFocus").mockReturnValue(false); expect(interactiveInput(editor, list, false, "empty", true)).toBe(false);
});
