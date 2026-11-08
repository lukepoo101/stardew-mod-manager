import type {
  DownloadedPreviewDto,
  ModRequirementDto,
  OperationPreviewDto,
} from "@/shared/api/generated";
import { meetsMinimumVersion } from "@/shared/versions";

/** One archive chosen for a batch install, after inspection. */
export interface BatchItem {
  /** The archive's path, or for a downloaded update a key naming it. */
  path: string;
  /** What to call it; the file name of `path` when not given. */
  name?: string;
  preview?: OperationPreviewDto;
  /** Where a downloaded update came from. */
  origin?: Omit<DownloadedPreviewDto, "preview">;
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
  /** Paths of the archives in the batch this one needs installed first. */
  after: string[];
}

/** Statuses whose archives the batch will try to install. */
export const WILL_INSTALL: ReadonlySet<BatchStatus> = new Set([
  "ready",
  "upgrade",
  "after_others",
]);

export const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

/** What a batch calls an item: its name, else its archive's file name. */
export const itemName = (item: Pick<BatchItem, "path" | "name">) =>
  item.name ?? fileName(item.path);

const ids = (preview: OperationPreviewDto) =>
  preview.detected_components.map((c) => c.unique_id.toLowerCase());

const ALREADY_INSTALLED = /already installed/i;

/** A requirement of one mod in an archive that the profile does not meet. */
interface Unmet {
  modName: string;
  requirement: ModRequirementDto;
}

const unmetRequirements = (preview: OperationPreviewDto): Unmet[] => {
  const own = new Set(ids(preview));
  return preview.detected_components.flatMap((component) =>
    component.requirements
      .filter(
        (r) =>
          r.kind !== "optional" &&
          (r.status === "missing" || r.status === "too_old") &&
          !own.has(r.unique_id.toLowerCase()),
      )
      .map((requirement) => ({ modName: component.name, requirement })),
  );
};

const needText = ({ modName, requirement }: Unmet, name?: string) =>
  `${modName} needs ${name ?? requirement.name ?? requirement.unique_id}${
    requirement.minimum_version
      ? ` ${requirement.minimum_version} or newer`
      : ""
  }`;

/** What an archive provides to the others: each mod and its version. */
interface Provided {
  entry: Draft;
  name: string;
  version: string;
}

interface Draft extends BatchEntry {
  index: number;
  unmet: Unmet[];
  /** For a duplicate: the index of the archive installed instead. */
  sameAs?: number;
}

/**
 * Decides what a batch will do with each archive, before anything changes,
 * and in which order to install them. Entries come back in the order given;
 * `installOrder` puts them in the order they will be installed.
 *
 * Each archive is installed on its own: one that cannot be installed does not
 * stop the others. A mod needed by another archive in the batch is installed
 * first, and only when the batch has a version new enough; archives that need
 * each other, or need one that cannot be installed, are left out with the
 * reason. A downgrade is left out so it can be confirmed on its own.
 */
export function classifyBatch(items: readonly BatchItem[]): BatchEntry[] {
  const seen = new Map<string, number>();
  const drafts: Draft[] = items.map((item, index) => {
    const draft = (
      status: BatchStatus,
      reason: string,
      unmet: Unmet[] = [],
    ): Draft => ({ item, status, reason, after: [], index, unmet });
    const preview = item.preview;
    if (!preview) {
      return draft("failed", item.error ?? "It could not be read.");
    }
    const repeat = ids(preview).find((id) => seen.has(id));
    if (repeat) {
      const sameAs = seen.get(repeat) ?? 0;
      return {
        ...draft(
          "duplicate",
          `It has the same mod as ${itemName(items[sameAs])}, which is installed instead.`,
        ),
        sameAs,
      };
    }
    for (const id of ids(preview)) seen.set(id, index);

    const replacing = preview.replaces.length > 0;
    const drop = (list: string[]) =>
      replacing ? list.filter((b) => !ALREADY_INSTALLED.test(b)) : list;
    const blockers = drop(preview.blockers);
    const others = drop(preview.other_blockers);
    const unmet = unmetRequirements(preview);
    if (others.length > 0) return draft("blocked", others.join(" "));
    // Something blocks it that is neither a requirement nor named above.
    if (blockers.length > 0 && unmet.length === 0) {
      return draft("blocked", blockers.join(" "));
    }
    if (replacing) {
      const downgrade = preview.replaces.some(
        (r) => r.direction === "downgrade",
      );
      const change = preview.replaces
        .map((r) => `${r.name} ${r.installed_version} → ${r.incoming_version}`)
        .join(", ");
      return downgrade
        ? draft(
            "downgrade",
            `Older than what is installed (${change}). Install it on its own to confirm a downgrade.`,
          )
        : draft(
            "upgrade",
            `Replaces ${change}. Settings are kept and a restore point is saved first.`,
            unmet,
          );
    }
    return unmet.length === 0
      ? draft("ready", "Ready to install.")
      : draft("after_others", "", unmet);
  });

  // Which mods each archive brings, with versions. One that will not be
  // installed still counts here, so its dependents say why they wait on it.
  const provided = new Map<string, Provided[]>();
  for (const entry of drafts) {
    if (entry.status === "duplicate" || !entry.item.preview) continue;
    for (const component of entry.item.preview.detected_components) {
      const id = component.unique_id.toLowerCase();
      provided.set(id, [
        ...(provided.get(id) ?? []),
        { entry, name: component.name, version: component.version },
      ]);
    }
  }

  // Match each unmet requirement to an archive whose version meets it.
  const needs = new Map<Draft, Draft[]>();
  for (const entry of drafts) {
    if (!WILL_INSTALL.has(entry.status) || entry.unmet.length === 0) continue;
    const providers: Draft[] = [];
    const reasons: string[] = [];
    let unsatisfied: string | null = null;
    for (const unmet of entry.unmet) {
      const { requirement } = unmet;
      const candidates = (
        provided.get(requirement.unique_id.toLowerCase()) ?? []
      )
        .filter((p) => p.entry !== entry)
        // Prefer an archive that is being installed.
        .sort(
          (a, b) =>
            Number(WILL_INSTALL.has(b.entry.status)) -
            Number(WILL_INSTALL.has(a.entry.status)),
        );
      const fits = candidates.find(
        (p) =>
          !requirement.minimum_version ||
          meetsMinimumVersion(p.version, requirement.minimum_version),
      );
      if (fits) {
        if (!providers.includes(fits.entry)) providers.push(fits.entry);
        reasons.push(
          `${needText(unmet, fits.name)}: ${fits.name} ${fits.version} from ${itemName(fits.entry.item)} is installed first.`,
        );
      } else if (candidates.length > 0) {
        const have = candidates
          .map((p) => `${p.version} (${itemName(p.entry.item)})`)
          .join(", ");
        unsatisfied = `Cannot install: ${needText(unmet, candidates[0].name)}; the batch has ${have}.`;
        break;
      } else {
        unsatisfied =
          requirement.status === "too_old"
            ? `Cannot install: ${needText(unmet)}; ${requirement.installed_version} is installed and the batch has no newer one.`
            : `Cannot install: ${needText(unmet)}, which is not installed or in this batch.`;
        break;
      }
    }
    if (unsatisfied) {
      entry.status = "blocked";
      entry.reason = unsatisfied;
      continue;
    }
    needs.set(entry, providers);
    entry.after = providers.map((p) => p.item.path);
    const lead = entry.status === "upgrade" ? `${entry.reason} ` : "";
    entry.reason = `${lead}${reasons.join(" ")}`;
  }

  // An archive that needs one which cannot be installed cannot be either;
  // archives that need each other cannot go first. Repeat until settled.
  const block = (entry: Draft, reason: string) => {
    entry.status = "blocked";
    entry.reason = reason;
    needs.delete(entry);
  };
  for (;;) {
    let changed = false;
    for (const [entry, providers] of needs) {
      const gone = providers.find((p) => !WILL_INSTALL.has(p.status));
      if (gone) {
        block(
          entry,
          `Cannot install: it needs ${itemName(gone.item)}, which ${
            gone.status === "downgrade"
              ? "is left out of this batch"
              : "cannot be installed"
          }.`,
        );
        changed = true;
      }
    }
    if (changed) continue;
    const cycle = findCycle(needs);
    if (!cycle) break;
    const names = [...cycle, cycle[0]].map((e) => itemName(e.item)).join(" → ");
    for (const entry of cycle) {
      block(
        entry,
        `Cannot install: these archives need each other, so none can be installed first (${names}).`,
      );
    }
  }

  // The archive a duplicate deferred to may itself be left out.
  for (const entry of drafts) {
    if (entry.sameAs === undefined) continue;
    const kept = drafts[entry.sameAs];
    if (!WILL_INSTALL.has(kept.status)) {
      entry.reason = `It has the same mod as ${itemName(kept.item)}, which is not being installed either; its reason is listed with it.`;
    }
  }

  return drafts.map(({ item, status, reason, after }) => ({
    item,
    status,
    reason,
    after: WILL_INSTALL.has(status) ? after : [],
  }));
}

/** One cycle among archives waiting for each other, if there is any. */
function findCycle(needs: Map<Draft, Draft[]>): Draft[] | null {
  const state = new Map<Draft, "visiting" | "done">();
  const path: Draft[] = [];
  const visit = (entry: Draft): Draft[] | null => {
    if (state.get(entry) === "done") return null;
    if (state.get(entry) === "visiting") {
      return path.slice(path.indexOf(entry));
    }
    state.set(entry, "visiting");
    path.push(entry);
    for (const next of needs.get(entry) ?? []) {
      const found = visit(next);
      if (found) return found;
    }
    path.pop();
    state.set(entry, "done");
    return null;
  };
  for (const entry of [...needs.keys()].sort((a, b) => a.index - b.index)) {
    const found = visit(entry);
    if (found) return found;
  }
  return null;
}

/**
 * The entries in the order a batch installs them: every archive after the
 * ones it needs, otherwise in the order chosen; the ones left out last.
 */
export function installOrder(entries: readonly BatchEntry[]): BatchEntry[] {
  const willInstall = entries.filter((e) => WILL_INSTALL.has(e.status));
  const placed = new Set<string>();
  const ordered: BatchEntry[] = [];
  while (ordered.length < willInstall.length) {
    const next = willInstall.find(
      (e) =>
        !placed.has(e.item.path) && e.after.every((path) => placed.has(path)),
    );
    // classifyBatch leaves no cycles; never loop if a caller passes one.
    if (!next) break;
    placed.add(next.item.path);
    ordered.push(next);
  }
  return [...ordered, ...entries.filter((e) => !ordered.includes(e))];
}
