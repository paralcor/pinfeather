import { expect, it } from "vitest";
import { parseStartupCaptureSnapshot } from "./startup-capture";
import startupTimeoutSnapshots from "../../tests/fixtures/contracts/startup_timeout_snapshots.json";

it.each(startupTimeoutSnapshots)("parses the host's $outcome timeout snapshot", snapshot => {
  expect(parseStartupCaptureSnapshot(snapshot)).toEqual(snapshot);
  expect(parseStartupCaptureSnapshot({ ...snapshot, error: null })).toBeNull();
});
