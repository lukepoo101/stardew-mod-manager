import { afterEach, describe, expect, it } from "vitest";
import { markSeen, readSeenAt, unseen } from "@/shared/activity/seen";
import type { OperationDto } from "@/shared/api/generated";

afterEach(() => localStorage.clear());

const op = (id: string, state: string, at: string, over = {}) =>
  ({
    id,
    kind: "mod_install",
    state,
    updated_at: at,
    completed_at: at,
    rolled_back: false,
    ...over,
  }) as unknown as OperationDto;

describe("activity read state", () => {
  it("counts finished entries since the last visit, and failures apart", () => {
    const ops = [
      op("a", "succeeded", "2026-10-02T10:00:00Z"),
      op("b", "failed", "2026-10-02T11:00:00Z"),
      op("c", "running", "2026-10-02T12:00:00Z"),
      op("d", "failed", "2026-10-02T12:30:00Z", { rolled_back: true }),
      op("e", "succeeded", "2026-10-01T09:00:00Z"),
    ];
    const fresh = unseen(ops, "2026-10-02T00:00:00Z");
    expect([...fresh.ids].sort()).toEqual(["a", "b", "d"]);
    expect(fresh.failed).toBe(1);
  });

  it("treats everything as new until visited, and remembers the visit", () => {
    expect(readSeenAt()).toBeNull();
    expect(
      unseen([op("a", "succeeded", "2026-10-02T10:00:00Z")], null).count,
    ).toBe(1);
    markSeen("2026-10-03T00:00:00Z");
    expect(readSeenAt()).toBe("2026-10-03T00:00:00Z");
  });
});
