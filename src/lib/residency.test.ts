import { expect, it } from "vitest";
import fixture from "../../tests/fixtures/contracts/residency.json";
import { acceptResidency, parseResidencySnapshot } from "./residency";
it("m2 strictly accepts the shared residency contract and rejects malformed capabilities", () => {
  for (const value of fixture.valid) expect(parseResidencySnapshot(value)).not.toBeNull();
  for (const value of fixture.invalid) expect(parseResidencySnapshot(value)).toBeNull();
});
it("m2 rejects stale revisions and never reopens an exit from a late running response", () => {
  const stopping = parseResidencySnapshot(fixture.valid[1])!;
  expect(acceptResidency(stopping, parseResidencySnapshot(fixture.valid[0])!)).toBe(stopping);
  expect(acceptResidency(stopping, { ...stopping, revision: "3", phase: "running" })).toBe(stopping);
  expect(acceptResidency(stopping, { ...stopping, revision: "3", phase: "failed" }).phase).toBe("failed");
});
