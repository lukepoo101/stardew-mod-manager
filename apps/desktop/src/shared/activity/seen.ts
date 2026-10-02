import type { OperationDto } from "@/shared/api/generated";

/**
 * Which Activity entries the user has already looked at. This is
 * presentation only: marking something read never changes the operation,
 * and unresolved problems stay in health whatever is read here.
 */
const KEY = "smm-activity-seen-at";

const FINISHED = new Set([
  "succeeded",
  "failed",
  "cancelled",
  "recovery_required",
]);

export function readSeenAt(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function markSeen(at: string = new Date().toISOString()): void {
  try {
    localStorage.setItem(KEY, at);
  } catch {
    // Without storage everything simply stays "new".
  }
}

/** Finished entries newer than the last visit, and how many failed. */
export function unseen(
  operations: readonly OperationDto[] | undefined,
  seenAt: string | null,
): { count: number; failed: number; ids: Set<string> } {
  const since = seenAt ? Date.parse(seenAt) : Number.NEGATIVE_INFINITY;
  const fresh = (operations ?? []).filter(
    (op) =>
      FINISHED.has(op.state) &&
      Date.parse(op.completed_at ?? op.updated_at) > since,
  );
  return {
    count: fresh.length,
    failed: fresh.filter(
      (op) =>
        !op.rolled_back &&
        (op.state === "failed" || op.state === "recovery_required"),
    ).length,
    ids: new Set(fresh.map((op) => op.id)),
  };
}
