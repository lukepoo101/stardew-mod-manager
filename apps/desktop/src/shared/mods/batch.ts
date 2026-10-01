import type { OperationPreviewDto } from "@/shared/api/generated";

/** One archive chosen for a batch install, after inspection. */
export interface BatchItem {
  path: string;
  preview?: OperationPreviewDto;
  /** Why it could not be inspected. */
  error?: string;
}

export type BatchStatus =
  | "ready"
  | "upgrade"
  | "downgrade"
  | "after_others"
  | "blocked"
  | "duplicate"
  | "failed";

export interface BatchEntry {
  item: BatchItem;
  status: BatchStatus;
  /** Plain-language reason, shown beside the archive. */
  reason: string;
}

/** Statuses whose archives the batch will try to install. */
export const WILL_INSTALL: ReadonlySet<BatchStatus> = new Set([
  "ready",
  "upgrade",
  "after_others",
]);

export const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

const ids = (preview: OperationPreviewDto) =>
  preview.detected_components.map((c) => c.unique_id.toLowerCase());

/** UniqueIDs quoted in a blocker, such as "Required dependency 'X' is missing". */
const quoted = (text: string) =>
  [...text.matchAll(/'([^']+)'/g)].map((match) => match[1].toLowerCase());

/**
 * Decides what a batch will do with each archive, before anything changes.
 * Each archive is installed on its own: one that cannot be installed does not
 * stop the others. A mod needed by another archive in the batch is installed
 * first; a downgrade is left out so it can be confirmed on its own.
 */
export function classifyBatch(items: readonly BatchItem[]): BatchEntry[] {
  const provided = new Set<string>();
  for (const item of items) {
    if (item.preview) for (const id of ids(item.preview)) provided.add(id);
  }
  const seen = new Map<string, string>();
  return items.map((item) => {
    const preview = item.preview;
    if (!preview) {
      return {
        item,
        status: "failed",
        reason: item.error ?? "It could not be read.",
      };
    }
    const repeat = ids(preview).find((id) => seen.has(id));
    if (repeat) {
      return {
        item,
        status: "duplicate",
        reason: `It has the same mod as ${seen.get(repeat)}, which is installed instead.`,
      };
    }
    for (const id of ids(preview)) seen.set(id, fileName(item.path));

    const replacing = preview.replaces.length > 0;
    const blockers = replacing
      ? preview.blockers.filter((b) => !/already installed/i.test(b))
      : preview.blockers;
    if (replacing && blockers.length === 0) {
      const downgrade = preview.replaces.some(
        (r) => r.direction === "downgrade",
      );
      const change = preview.replaces
        .map((r) => `${r.name} ${r.installed_version} → ${r.incoming_version}`)
        .join(", ");
      return downgrade
        ? {
            item,
            status: "downgrade",
            reason: `Older than what is installed (${change}). Install it on its own to confirm a downgrade.`,
          }
        : {
            item,
            status: "upgrade",
            reason: `Replaces ${change}. Settings are kept and a restore point is saved first.`,
          };
    }
    if (blockers.length === 0) {
      return { item, status: "ready", reason: "Ready to install." };
    }
    const own = new Set(ids(preview));
    const satisfiedHere = blockers.every((blocker) => {
      const needed = quoted(blocker).filter((id) => !own.has(id));
      return needed.length > 0 && needed.every((id) => provided.has(id));
    });
    return satisfiedHere
      ? {
          item,
          status: "after_others",
          reason: "Needs a mod from another archive here; installed after it.",
        }
      : { item, status: "blocked", reason: blockers.join(" ") };
  });
}
