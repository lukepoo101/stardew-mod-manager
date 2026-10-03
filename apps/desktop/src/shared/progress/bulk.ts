import { useSyncExternalStore } from "react";

/**
 * The bulk job in progress, if any, for showing outside the dialog that
 * started it, so it stays visible while the user goes elsewhere. One at a
 * time; presentation only, nothing here changes what the job does.
 */
export type ItemState =
  | "queued"
  | "running"
  | "done"
  | "failed"
  | "skipped"
  | "cancelled";

export interface BulkItem {
  name: string;
  state: ItemState;
  message?: string;
}

export interface BulkJob {
  title: string;
  items: BulkItem[];
  finished: boolean;
}

let job: BulkJob | null = null;
const listeners = new Set<() => void>();
const emit = () => {
  for (const listener of listeners) listener();
};

export const bulkProgress = {
  start(title: string, names: string[]) {
    job = {
      title,
      items: names.map((name) => ({ name, state: "queued" })),
      finished: false,
    };
    emit();
  },
  set(index: number, state: ItemState, message?: string) {
    if (!job) return;
    job = {
      ...job,
      items: job.items.map((item, i) =>
        i === index ? { ...item, state, message } : item,
      ),
    };
    emit();
  },
  finish() {
    if (!job) return;
    job = { ...job, finished: true };
    emit();
  },
  clear() {
    job = null;
    emit();
  },
  get: () => job,
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};

export function useBulkProgress(): BulkJob | null {
  return useSyncExternalStore(bulkProgress.subscribe, bulkProgress.get);
}

/** Counts by state, and a one-line summary that tells partial from full. */
export function summarise(items: readonly BulkItem[]): {
  counts: Record<ItemState, number>;
  line: string;
} {
  const counts: Record<ItemState, number> = {
    queued: 0,
    running: 0,
    done: 0,
    failed: 0,
    skipped: 0,
    cancelled: 0,
  };
  for (const item of items) counts[item.state] += 1;
  const total = items.length;
  const line =
    counts.queued + counts.running > 0
      ? `${counts.done + counts.failed + counts.skipped + counts.cancelled} of ${total} handled`
      : counts.done === total
        ? `All ${total} done`
        : `${counts.done} of ${total} done; ${[
            counts.failed && `${counts.failed} failed`,
            counts.skipped && `${counts.skipped} skipped`,
            counts.cancelled && `${counts.cancelled} not started`,
          ]
            .filter(Boolean)
            .join(", ")}`;
  return { counts, line };
}
