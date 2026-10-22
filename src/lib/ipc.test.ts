import { describe, expect, it } from "vitest";
import primitivesFixture from "../../tests/fixtures/contracts/primitives.json";
import queryLimitsFixture from "../../tests/fixtures/contracts/query_limits.json";
import requestsFixture from "../../tests/fixtures/contracts/requests.json";
import responsesFixture from "../../tests/fixtures/contracts/responses.json";
import errorsWorkerFixture from "../../tests/fixtures/contracts/errors_worker.json";
import eventsResultsFixture from "../../tests/fixtures/contracts/events_results.json";
import fixBoundariesFixture from "../../tests/fixtures/contracts/fix_boundaries.json";
import responseDtosFixture from "../../tests/fixtures/contracts/response_dtos.json";
import {
  contractErrorToJson,
  errorJsonLeaks,
  normalizeQuery,
  parseClipSummary,
  parseClipboardState,
  parseErrorCode,
  parseGetClipTextRequest,
  parseInputState,
  parseListClipsRequest,
  parsePanelReadyRequest,
  parsePasteClipRequest,
  parseDismissPanelRequest,
  parseSetCaptureEnabledRequest,
  parseSetExcludedAppsRequest,
  parseSetHotkeyRequest,
  parseStatusState,
  parsePasteFinishedEvent,
  parsePasteClipResult,
  parseRevisionEvent,
  parseListClipsResult,
  parseGetClipTextResult,
  parseStatusResult,
  normalizeWindowsExePath,
  parseU64String,
  parsePositiveId,
  validateLimit,
  validateRequestId,
  validateTimestampMs,
  type ContractResult,
} from "./ipc";

interface FixtureCase {
  id: string;
  group: string;
  expect: "accept" | "reject";
  reject_code?: string;
  value?: unknown;
  value_fill?: { char: string; count: number; prefix?: string; suffix?: string };
  normalized?: string;
  normalized_fill?: { char: string; count: number; prefix?: string; suffix?: string };
}

type FixtureDoc = { sensitive_sentinel?: string; cases: FixtureCase[] };

const FIXTURES: Record<string, FixtureDoc> = {
  "primitives.json": primitivesFixture as FixtureDoc,
  "query_limits.json": queryLimitsFixture as FixtureDoc,
  "requests.json": requestsFixture as FixtureDoc,
  "responses.json": responsesFixture as FixtureDoc,
  "errors_worker.json": errorsWorkerFixture as FixtureDoc,
  "events_results.json": eventsResultsFixture as FixtureDoc,
};

function loadFixture(name: string): FixtureDoc {
  return FIXTURES[name]!;
}

function expand(fill: { char: string; count: number; prefix?: string; suffix?: string }): string {
  return (fill.prefix ?? "") + fill.char.repeat(fill.count) + (fill.suffix ?? "");
}

function caseValue(c: FixtureCase): unknown {
  if (c.value !== undefined) return c.value;
  if (c.value_fill) return expand(c.value_fill);
  return null;
}

function runCases(
  name: string,
  pick: (c: FixtureCase) => ContractResult<unknown> | null,
): void {
  const doc = loadFixture(name);
  for (const c of doc.cases) {
    const result = pick(c);
    if (result === null) continue;
    if (c.expect === "accept") {
      expect(result.ok, `${c.id} should accept: ${JSON.stringify(result)}`).toBe(true);
    } else {
      expect(result.ok, `${c.id} should reject`).toBe(false);
      if (!result.ok) {
        expect(result.error.code, `${c.id} wrong code`).toBe(c.reject_code ?? "invalid_input");
      }
    }
  }
}

describe("primitive fixtures", () => {
  it("request_id", () => {
    runCases("primitives.json", (c) =>
      c.group === "request_id" ? validateRequestId(caseValue(c)) : null,
    );
  });

  it("positive_id", () => {
    runCases("primitives.json", (c) =>
      c.group === "positive_id" ? parsePositiveId(caseValue(c)) : null,
    );
  });

  it("query / limit / timestamp", () => {
    runCases("query_limits.json", (c) => {
      const v = caseValue(c);
      if (c.group === "query") return normalizeQuery(v);
      if (c.group === "limit") return validateLimit(v);
      if (c.group === "timestamp_ms") return validateTimestampMs(v);
      return null;
    });
  });

  it("query normalization matches fixture", () => {
    const doc = loadFixture("query_limits.json");
    for (const c of doc.cases) {
      if (c.group !== "query" || c.expect !== "accept") continue;
      const normalized = normalizeQuery(caseValue(c));
      expect(normalized.ok).toBe(true);
      if (normalized.ok) {
        const expected = c.normalized ?? expand(c.normalized_fill!);
        expect(normalized.value, c.id).toBe(expected);
      }
    }
  });
});

describe("request DTO fixtures", () => {
  it("all request groups", () => {
    runCases("requests.json", (c) => {
      const v = caseValue(c) as Record<string, unknown>;
      switch (c.group) {
        case "list_clips_request":
          return parseListClipsRequest(v);
        case "get_clip_text_request":
          return parseGetClipTextRequest(v);
        case "set_capture_enabled_request":
          return parseSetCaptureEnabledRequest(v);
        case "set_excluded_apps_request": {
          if (v && typeof v === "object" && "paths_count" in v) {
            const n = v.paths_count as number;
            const paths = Array.from({ length: n }, (_x, i) => `C:\\App\\p${i}.exe`);
            return parseSetExcludedAppsRequest({ paths });
          }
          return parseSetExcludedAppsRequest(v);
        }
        case "set_hotkey_request":
          return parseSetHotkeyRequest(v);
        case "panel_ready_request":
          return parsePanelReadyRequest(v);
        case "paste_clip_request":
          return parsePasteClipRequest(v);
        case "dismiss_panel_request":
          return parseDismissPanelRequest(v);
        default:
          throw new Error(`unknown group ${c.group}`);
      }
    });
  });
});

describe("response shape and enum fixtures", () => {
  it("accepted summaries preserve their numeric fields on a JSON round trip", () => {
    for (const c of loadFixture("responses.json").cases) {
      if (c.group !== "clip_summary" || c.expect !== "accept") continue;
      const parsed = parseClipSummary(caseValue(c));
      expect(parsed.ok, c.id).toBe(true);
      if (!parsed.ok) continue;
      const wire = JSON.parse(JSON.stringify(parsed.value));
      expect(wire, c.id).toEqual(caseValue(c));
      expect(parseClipSummary(wire), c.id).toEqual(parsed);
    }
  });

  it("all response groups", () => {
    runCases("responses.json", (c) => {
      const v = caseValue(c);
      switch (c.group) {
        case "clip_summary":
          return parseClipSummary(v);
        case "error_code":
          return Array.isArray(v)
            ? (v as unknown[]).reduce<ContractResult<unknown>>(
                (acc, item) => (acc.ok ? parseErrorCode(item) : acc),
                { ok: true, value: null },
              )
            : parseErrorCode(v);
        case "status_state":
          return Array.isArray(v)
            ? (v as unknown[]).reduce<ContractResult<unknown>>(
                (acc, item) => (acc.ok ? parseStatusState(item) : acc),
                { ok: true, value: null },
              )
            : parseStatusState(v);
        case "clipboard_state":
          return Array.isArray(v)
            ? (v as unknown[]).reduce<ContractResult<unknown>>(
                (acc, item) => (acc.ok ? parseClipboardState(item) : acc),
                { ok: true, value: null },
              )
            : parseClipboardState(v);
        case "input_state":
          return Array.isArray(v)
            ? (v as unknown[]).reduce<ContractResult<unknown>>(
                (acc, item) => (acc.ok ? parseInputState(item) : acc),
                { ok: true, value: null },
              )
            : parseInputState(v);
        default:
          throw new Error(`unknown group ${c.group}`);
      }
    });
  });
});

describe("error serialization", () => {
  it("error json does not leak sentinel", () => {
    const doc = loadFixture("errors_worker.json");
    const sentinel = doc.sensitive_sentinel!;
    for (const c of doc.cases) {
      if (c.group !== "error_no_leak") continue;
      const value = caseValue(c) as { group: string; input?: string; input_fill?: { char: string; count: number; prefix?: string } };
      const input = value.input ?? expand(value.input_fill!);
      let result: ContractResult<unknown>;
      if (value.group === "query") result = normalizeQuery(input);
      else if (value.group === "request_id") result = validateRequestId(input);
      else result = parsePositiveId(input);
      expect(result.ok, `${c.id} must reject`).toBe(false);
      if (!result.ok) {
        const serialized = contractErrorToJson(result.error);
        expect(errorJsonLeaks(serialized, sentinel), `${c.id} leaked`).toBe(false);
      }
    }
  });
});

describe("constants mirror Rust authority", () => {
  it("boundary constants", () => {
    expect(normalizeQuery("x".repeat(256)).ok).toBe(true);
    expect(normalizeQuery("x".repeat(257)).ok).toBe(false);
    expect(validateLimit(1).ok).toBe(true);
    expect(validateLimit(100).ok).toBe(true);
    expect(validateLimit(0).ok).toBe(false);
    expect(validateLimit(101).ok).toBe(false);
    expect(validateRequestId("a".repeat(64)).ok).toBe(true);
    expect(validateRequestId("a".repeat(65)).ok).toBe(false);
  });
});

describe("event and result fixtures", () => {
  it("all event/result groups", () => {
    runCases("events_results.json", (c) => {
      const v = caseValue(c);
      switch (c.group) {
        case "history_changed":
        case "status_changed":
          return parseRevisionEvent(v, "revision");
        case "panel_opened":
          return parseRevisionEvent(v, "session_id");
        case "paste_finished":
          return parsePasteFinishedEvent(v);
        case "list_clips_result":
          return parseListClipsResult(v);
        default:
          throw new Error(`unknown group ${c.group}`);
      }
    });
  });

  it("paste result reason enum", () => {
    expect(
      parsePasteClipResult({
        operation_id: "op1",
        clipboard_state: "written",
        input_state: "sent",
        reason: null,
      }).ok,
    ).toBe(true);
    expect(
      parsePasteClipResult({
        operation_id: "op1",
        clipboard_state: "written",
        input_state: "sent",
        reason: "nope",
      }).ok,
    ).toBe(false);
  });
});

function unitsToString(units: number[]): string | null {
  let out = "";
  for (const u of units) {
    if (u >= 0xd800 && u <= 0xdfff) return null;
    out += String.fromCodePoint(u);
  }
  return out;
}

describe("F1-F4 fix boundaries", () => {
  it("F1 trim whitespace set matches spec", () => {
    const cases = (fixBoundariesFixture as { trim_cases: Array<Record<string, unknown>> }).trim_cases;
    for (const c of cases) {
      const id = c.id as string;
      const units = c.value_units as number[];
      const built = unitsToString(units);
      if (c.expect === "reject") {
        if (built === null) continue;
        expect(normalizeQuery(built).ok, `${id} should reject`).toBe(false);
        continue;
      }
      expect(built, `${id} units decode`).not.toBeNull();
      const normalized = normalizeQuery(built!);
      expect(normalized.ok, `${id} should accept`).toBe(true);
      if (normalized.ok) {
        const expected = unitsToString(c.normalized_units as number[])!;
        expect(normalized.value, id).toBe(expected);
      }
    }
  });

  it("F2 excluded apps path lexical", () => {
    const cases = (fixBoundariesFixture as { path_cases: Array<Record<string, unknown>> }).path_cases;
    for (const c of cases) {
      const id = c.id as string;
      const input = c.input as string;
      const result = normalizeWindowsExePath(input);
      if (c.expect === "reject") {
        expect(result.ok, `${id} should reject: ${JSON.stringify(result)}`).toBe(false);
        continue;
      }
      expect(result.ok, `${id} should accept: ${JSON.stringify(result)}`).toBe(true);
      if (result.ok) expect(result.value, id).toBe(c.normalized);
    }
  });

  it("F3 id range string canonical", () => {
    const cases = (fixBoundariesFixture as { id_cases: Array<Record<string, unknown>> }).id_cases;
    for (const c of cases) {
      const id = c.id as string;
      const result = parsePositiveId(c.value);
      if (c.expect === "accept") expect(result.ok, `${id} should accept`).toBe(true);
      else expect(result.ok, `${id} should reject`).toBe(false);
    }
    const u64 = (fixBoundariesFixture as { u64_cases: Array<Record<string, unknown>> }).u64_cases;
    for (const c of u64) {
      const id = c.id as string;
      const result = parseU64String(c.value);
      if (c.expect === "accept") expect(result.ok, `${id} should accept`).toBe(true);
      else expect(result.ok, `${id} should reject`).toBe(false);
    }
  });

  it("F4 list optional defaults", () => {
    const cases = (fixBoundariesFixture as { list_optional_cases: Array<Record<string, unknown>> })
      .list_optional_cases;
    for (const c of cases) {
      const id = c.id as string;
      const result = parseListClipsRequest(c.value);
      if (c.expect === "reject") {
        expect(result.ok, `${id} should reject: ${JSON.stringify(result)}`).toBe(false);
        continue;
      }
      expect(result.ok, `${id} should accept: ${JSON.stringify(result)}`).toBe(true);
      if (result.ok) {
        expect(result.value.limit, `${id} limit`).toBe(c.expect_limit);
        if (c.expect_cursor === null) expect(result.value.cursor, `${id} cursor`).toBeNull();
        else expect(result.value.cursor, `${id} cursor`).not.toBeNull();
      }
    }
  });
});

describe("7.2 response DTOs", () => {
  it("status result fixtures", () => {
    const cases = (responseDtosFixture as { status_cases: Array<Record<string, unknown>> }).status_cases;
    for (const c of cases) {
      const id = c.id as string;
      const result = parseStatusResult(c.value);
      if (c.expect === "reject") {
        expect(result.ok, `${id} should reject: ${JSON.stringify(result)}`).toBe(false);
        continue;
      }
      expect(result.ok, `${id} should accept: ${JSON.stringify(result)}`).toBe(true);
      if (result.ok) {
        const wire = JSON.parse(JSON.stringify(result.value));
        expect(typeof wire.revision, `${id} revision wire`).toBe("string");
        expect(typeof wire.counters.observed, `${id} counter wire`).toBe("string");
        const reparsed = parseStatusResult(wire);
        expect(reparsed.ok, `${id} reparse`).toBe(true);
      }
    }
  });

  it("get clip text result fixtures", () => {
    const cases = (responseDtosFixture as { text_cases: Array<Record<string, unknown>> }).text_cases;
    for (const c of cases) {
      const id = c.id as string;
      const value = { ...(c.value as Record<string, unknown>) };
      if (value.text_fill) {
        const fill = value.text_fill as { char: string; count: number };
        value.text = fill.char.repeat(fill.count);
        delete value.text_fill;
      }
      const request = parseGetClipTextRequest(c.request);
      expect(request.ok, `${id} request`).toBe(true);
      if (!request.ok) continue;
      const result = parseGetClipTextResult(value, request.value);
      if (c.expect === "reject") {
        expect(result.ok, `${id} should reject: ${JSON.stringify(result)}`).toBe(false);
        continue;
      }
      expect(result.ok, `${id} should accept: ${JSON.stringify(result)}`).toBe(true);
    }
  });

  it("list clips result fixtures", () => {
    const cases = (responseDtosFixture as { list_cases: Array<Record<string, unknown>> }).list_cases;
    for (const c of cases) {
      const id = c.id as string;
      const value = { ...(c.value as Record<string, unknown>) };
      if (value.summaries_count !== undefined) {
        const n = value.summaries_count as number;
        const summary = {
          id: "1",
          captured_at_ms: 1000,
          kind: "text",
          preview: "hi",
          format_names: ["CF_UNICODETEXT"],
          omitted_format_count: 0,
          payload_bytes: 4,
          source_process: null,
          source_confidence: "unknown",
        };
        value.summaries = Array.from({ length: n }, () => ({ ...summary }));
        delete value.summaries_count;
      }
      const result = parseListClipsResult(value);
      if (c.expect === "reject") {
        expect(result.ok, `${id} should reject: ${JSON.stringify(result)}`).toBe(false);
        continue;
      }
      expect(result.ok, `${id} should accept: ${JSON.stringify(result)}`).toBe(true);
    }
  });

  it("list result budget exceed rejected", () => {
    const longSource = "s".repeat(12000);
    const summaries = Array.from({ length: 100 }, (_x, i) => ({
      id: String(i + 1),
      captured_at_ms: 1000,
      kind: "text",
      preview: "hi",
      format_names: ["CF_UNICODETEXT"],
      omitted_format_count: 0,
      payload_bytes: 4,
      source_process: longSource,
      source_confidence: "unknown",
    }));
    const result = parseListClipsResult({
      request_id: "r1",
      normalized_query: "",
      summaries,
      next_cursor: null,
    });
    expect(result.ok).toBe(false);
  });
});

describe("F6-F7 event revision and normalized_query", () => {
  it("F6 event revision u64", () => {
    const cases = (eventsResultsFixture as unknown as {
      revision_cases: Array<{ id: string; group: string; expect: string; value: unknown }>;
    }).revision_cases;
    for (const c of cases) {
      const result =
        c.group === "panel_opened"
          ? parseRevisionEvent(c.value, "session_id")
          : parseRevisionEvent(c.value, "revision");
      if (c.expect === "accept") {
        expect(result.ok, `${c.id} should accept: ${JSON.stringify(result)}`).toBe(true);
      } else {
        expect(result.ok, `${c.id} should reject`).toBe(false);
      }
    }
  });

  it("F7 list normalized_query and required fields", () => {
    const cases = (responseDtosFixture as unknown as {
      list_norm_cases: Array<{ id: string; expect: string; value: unknown }>;
    }).list_norm_cases;
    for (const c of cases) {
      const result = parseListClipsResult(c.value);
      if (c.expect === "accept") {
        expect(result.ok, `${c.id} should accept: ${JSON.stringify(result)}`).toBe(true);
      } else {
        expect(result.ok, `${c.id} should reject`).toBe(false);
      }
    }
  });
});
