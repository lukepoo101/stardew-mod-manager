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

/**
 * Failed operations stay "to follow up" after being seen, until the user
 * says they are done with one or snoozes it. Presentation only, kept on this
 * computer, and bounded: only the newest entries are remembered.
 */
const FOLLOW_KEY = "smm-activity-follow-up";
const MAX_REMEMBERED = 200;
const WINDOW_MS = 7 * 24 * 60 * 60 * 1000;

interface FollowState {
  /** Operation id → when the user said they were done with it. */
  done: Record<string, string>;
  /** Operation id → until when it is snoozed. */
  snoozed: Record<string, string>;
}

function readFollow(): FollowState {
  try {
    const parsed = JSON.parse(localStorage.getItem(FOLLOW_KEY) ?? "{}");
    return {
      done: parsed.done && typeof parsed.done === "object" ? parsed.done : {},
      snoozed:
        parsed.snoozed && typeof parsed.snoozed === "object"
          ? parsed.snoozed
          : {},
    };
  } catch {
    return { done: {}, snoozed: {} };
  }
}

function bound(record: Record<string, string>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(record)
      .sort((a, b) => b[1].localeCompare(a[1]))
      .slice(0, MAX_REMEMBERED),
  );
}

function writeFollow(state: FollowState) {
  try {
    localStorage.setItem(
      FOLLOW_KEY,
      JSON.stringify({
        done: bound(state.done),
        snoozed: bound(state.snoozed),
      }),
    );
  } catch {
    // Without storage, follow-ups simply stay listed.
  }
}

export function markDone(id: string, at = new Date()): void {
  const state = readFollow();
  state.done[id] = at.toISOString();
  delete state.snoozed[id];
  writeFollow(state);
}

export function snooze(id: string, until: Date): void {
  const state = readFollow();
  state.snoozed[id] = until.toISOString();
  writeFollow(state);
}

/** Failed operations of the last week still to follow up. */
export function followUps(
  operations: readonly OperationDto[] | undefined,
  now = new Date(),
): OperationDto[] {
  const state = readFollow();
  return (operations ?? []).filter(
    (op) =>
      !op.rolled_back &&
      (op.state === "failed" || op.state === "recovery_required") &&
      now.getTime() - Date.parse(op.completed_at ?? op.updated_at) <
        WINDOW_MS &&
      !state.done[op.id] &&
      !(
        state.snoozed[op.id] && Date.parse(state.snoozed[op.id]) > now.getTime()
      ),
  );
}
