import { afterEach, describe, expect, it } from "vitest";
import type { OperationDto } from "@/shared/api/generated";
import { followUps, markDone, snooze } from "@/shared/activity/seen";

afterEach(() => localStorage.clear());

const op = (id: string, state: string, at: string, rolled_back = false) =>
  ({
    id,
    state,
    completed_at: at,
    updated_at: at,
    rolled_back,
  }) as OperationDto;

describe("follow-ups", () => {
  const now = new Date("2026-10-03T12:00:00Z");
  const ops = [
    op("f1", "failed", "2026-10-03T10:00:00Z"),
    op("f2", "failed", "2026-10-02T10:00:00Z"),
    op("ok", "succeeded", "2026-10-03T10:00:00Z"),
    op("rb", "failed", "2026-10-03T10:00:00Z", true),
    op("old", "failed", "2026-09-01T10:00:00Z"),
  ];

  it("lists recent failures until done or snoozed", () => {
    expect(followUps(ops, now).map((o) => o.id)).toEqual(["f1", "f2"]);
    markDone("f1");
    snooze("f2", new Date("2026-10-04T12:00:00Z"));
    expect(followUps(ops, now)).toEqual([]);
    // A snooze ends; being done does not.
    expect(
      followUps(ops, new Date("2026-10-04T13:00:00Z")).map((o) => o.id),
    ).toEqual(["f2"]);
  });

  it("keeps only a bounded number of remembered entries", () => {
    for (let i = 0; i < 250; i += 1)
      markDone(`op${i}`, new Date(Date.UTC(2026, 0, 1, 0, 0, i)));
    const stored = JSON.parse(
      localStorage.getItem("smm-activity-follow-up") ?? "{}",
    );
    expect(Object.keys(stored.done)).toHaveLength(200);
    expect(stored.done.op249).toBeDefined();
    expect(stored.done.op0).toBeUndefined();
  });
});
