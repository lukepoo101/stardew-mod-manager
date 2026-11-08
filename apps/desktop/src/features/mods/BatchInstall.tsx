import React, { useRef, useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { ModUpdateDto } from "@/shared/api/generated";
import {
  type BatchEntry,
  type BatchItem,
  type BatchStatus,
  WILL_INSTALL,
  classifyBatch,
  itemName,
  installOrder,
} from "@/shared/mods/batch";
import {
  bulkProgress,
  summarise,
  useBulkProgress,
} from "@/shared/progress/bulk";

const LABEL: Record<BatchStatus, string> = {
  ready: "Install",
  upgrade: "Replace",
  downgrade: "Left out",
  after_others: "Install after others",
  blocked: "Cannot install",
  duplicate: "Left out",
  failed: "Cannot read",
};

const TONE: Record<BatchStatus, "success" | "info" | "warning" | "danger"> = {
  ready: "success",
  upgrade: "info",
  after_others: "info",
  downgrade: "warning",
  duplicate: "warning",
  blocked: "danger",
  failed: "danger",
};

interface Outcome {
  name: string;
  done: boolean;
  message: string;
}

/** The key a downloaded update is listed under in place of a path. */
const updateKey = (mod: ModUpdateDto) => `update:${mod.profile_component_id}`;

/** Where a downloaded update came from, in one line. */
function originLine(item: BatchItem): string | null {
  if (!item.origin) return null;
  const { source, checksum_verified } = item.origin;
  return `${source.asset_name} from GitHub release ${source.tag} of ${source.repo} · ${
    checksum_verified
      ? "matches the checksum GitHub publishes"
      : "GitHub publishes no checksum for it"
  }. Who published that release is not verified.`;
}

/**
 * Inspects several archives, shows what each will do, and installs the ones
 * that can be installed, each on its own. Nothing is installed until the user
 * confirms; removing an archive from the list just leaves it out.
 *
 * Given `updates` instead of `paths`, it downloads each mod's update from
 * GitHub first, one after another (a profile downloads one at a time), and
 * lists any that could not be downloaded with why; the rest are reviewed
 * and replaced the same way.
 */
export const BatchInstall: React.FC<{
  profileId: string;
  paths?: string[];
  updates?: ModUpdateDto[];
  onClose: () => void;
  /** Where focus goes when it closes; see `Modal`. */
  returnFocusTo?: () => HTMLElement | null;
}> = ({ profileId, paths, updates, onClose, returnFocusTo }) => {
  const forUpdates = updates !== undefined;
  const count = updates?.length ?? paths?.length ?? 0;
  const [items, setItems] = useState<BatchItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [outcomes, setOutcomes] = useState<Outcome[] | null>(null);
  // Where a long batch has got to, announced as it changes.
  const [progress, setProgress] = useState<string | null>(null);
  // A stop request is honoured between archives: one that is installing
  // finishes (or is rolled back) first, never left half done.
  const stopRequested = useRef(false);
  const job = useBulkProgress();
  const [stopping, setStopping] = useState(false);

  // Inspect each archive once, one after another. A profile holds one
  // prepared change at a time, so each draft is discarded once read and the
  // archive is prepared again when it is installed.
  React.useEffect(() => {
    let current = true;
    (async () => {
      const inspected: BatchItem[] = [];
      // Each update is downloaded, checked and prepared as a single
      // download is, then its draft discarded so the next can be prepared.
      // A failure (a file that is not this mod, the wrong size, no
      // matching release file) is listed with that mod; the rest go on.
      for (const [index, mod] of (updates ?? []).entries()) {
        if (!current) return;
        const name = `${mod.name} ${mod.update_version ?? ""}`.trim();
        setProgress(`Downloading ${index + 1} of ${updates?.length}: ${name}`);
        try {
          const { preview, ...origin } = await api.prepareModUpdate(
            profileId,
            mod.profile_component_id,
          );
          await api.cancelActiveOperation(preview.operation_id);
          inspected.push({ path: updateKey(mod), name, preview, origin });
        } catch (error) {
          inspected.push({
            path: updateKey(mod),
            name,
            error: errorSummary(error, "It could not be downloaded."),
          });
        }
      }
      for (const [index, path] of (paths ?? []).entries()) {
        // Closed before checking finished: the rest are not looked at.
        if (!current) return;
        setProgress(
          `Checking ${index + 1} of ${paths?.length}: ${itemName({ path })}`,
        );
        try {
          const preview = await api.inspectPackageForInstall(path, profileId);
          await api.cancelActiveOperation(preview.operation_id);
          inspected.push({ path, preview });
        } catch (error) {
          inspected.push({
            path,
            error: errorSummary(error, "It could not be read."),
          });
        }
      }
      if (current) setItems(inspected);
    })();
    return () => {
      current = false;
    };
  }, [paths, updates, profileId]);

  const entries = items ? classifyBatch(items) : [];
  const installable = entries.filter((e) => WILL_INSTALL.has(e.status));

  const remove = (entry: BatchEntry) => {
    setItems((list) => list?.filter((item) => item !== entry.item) ?? null);
  };

  const close = () => onClose();

  /** Prepares an archive again and commits it, or says why not. */
  const installOne = async (item: BatchItem): Promise<Outcome> => {
    const { path } = item;
    const name = itemName(item);
    // A download is not kept as a file to prepare again; one that no
    // longer replaces an installed mod is left to its own review.
    if (item.origin) {
      return {
        name,
        done: false,
        message:
          "It no longer replaces an installed mod, so it was not installed. Download it on its own to review it.",
      };
    }
    try {
      const fresh = await api.inspectPackageForInstall(path, profileId);
      if (fresh.blockers.length > 0) {
        await api.cancelActiveOperation(fresh.operation_id);
        return { name, done: false, message: fresh.blockers.join(" ") };
      }
      await api.executeOperation(fresh.operation_id);
      // What still needs attention (a disabled requirement) stays with the
      // result, as it does after a single install.
      return {
        name,
        done: true,
        message:
          fresh.warnings.length > 0
            ? `installed, but check: ${fresh.warnings.join(" ")}`
            : "installed",
      };
    } catch (error) {
      return {
        name,
        done: false,
        message: errorSummary(error, "It was not installed."),
      };
    }
  };

  const install = async () => {
    if (!items) return;
    setBusy(true);
    stopRequested.current = false;
    setStopping(false);
    const results: Outcome[] = [];
    // Every archive after the ones it needs.
    const order = installOrder(entries);
    const total = order.filter((e) => WILL_INSTALL.has(e.status)).length;
    const title = forUpdates
      ? `Updating ${total} mod(s)`
      : `Installing ${total} archive(s)`;
    // Shown in the sidebar too, so it stays visible on other pages, and
    // journaled as one change so each item's result stays in Activity.
    bulkProgress.start(
      title,
      order.map((e) => itemName(e.item)),
    );
    const parts = order
      .filter((e) => WILL_INSTALL.has(e.status) && e.item.preview)
      .map((e) => itemName(e.item));
    const journal =
      parts.length > 0
        ? await api
            .beginChangeSet(profileId, title, parts, "batch")
            .catch(() => null)
        : null;
    let started = 0;
    let part = 0;
    const failed: string[] = [];
    const notInstalled = new Set<string>();
    for (const [index, entry] of order.entries()) {
      const name = itemName(entry.item);
      const preview = entry.item.preview;
      const willInstall = WILL_INSTALL.has(entry.status) && preview;
      if (stopRequested.current && willInstall) {
        const message = "Not started: you stopped the batch.";
        results.push({ name, done: false, message });
        bulkProgress.set(index, "cancelled", message);
        if (journal) await api.changeSetPartDone(journal, part, message);
        part += 1;
        continue;
      }
      if (!willInstall) {
        results.push({ name, done: false, message: entry.reason });
        bulkProgress.set(index, "skipped", entry.reason);
        continue;
      }
      // What it needs did not install, so it would only fail the same way.
      const missing = entry.after.find((path) => notInstalled.has(path));
      if (missing) {
        const needed = entries.find((e) => e.item.path === missing)?.item;
        const message = `Not installed: it needs ${itemName(needed ?? { path: missing })}, which was not installed.`;
        results.push({ name, done: false, message });
        bulkProgress.set(index, "skipped", message);
        notInstalled.add(entry.item.path);
        failed.push(`${name}: ${message}`);
        if (journal) await api.changeSetPartDone(journal, part, message);
        part += 1;
        continue;
      }
      started += 1;
      setProgress(`Installing ${started} of ${total}: ${name}`);
      bulkProgress.set(index, "running");
      let outcome: Outcome;
      if (entry.status === "upgrade") {
        try {
          await api.replaceModVersion(profileId, preview.artifact_hash);
          outcome = { name, done: true, message: "replaced" };
        } catch (error) {
          outcome = {
            name,
            done: false,
            message: errorSummary(error, "It was not replaced."),
          };
        }
      } else {
        outcome = await installOne(entry.item);
      }
      results.push(outcome);
      bulkProgress.set(
        index,
        outcome.done ? "done" : "failed",
        outcome.done ? undefined : outcome.message,
      );
      if (!outcome.done) {
        failed.push(`${name}: ${outcome.message}`);
        notInstalled.add(entry.item.path);
      }
      if (journal)
        await api.changeSetPartDone(
          journal,
          part,
          outcome.done ? null : outcome.message,
        );
      part += 1;
    }
    if (journal) await api.finishChangeSet(journal, failed);
    bulkProgress.finish();
    setOutcomes(results);
    setProgress(null);
    setBusy(false);
  };

  return (
    <Modal
      labelledBy="batch-install-title"
      onClose={close}
      returnFocusTo={returnFocusTo}
      className="space-y-3 text-sm"
    >
      <h2 id="batch-install-title" className="text-lg font-bold">
        {forUpdates ? `Update ${count} mods` : `Install ${count} archives`}
      </h2>
      {!items ? (
        <>
          <p role="status">
            {progress ??
              (forUpdates
                ? "Downloading each update…"
                : "Checking each archive…")}
          </p>
          {forUpdates && (
            <p className="text-xs text-[var(--fg-muted)]">
              Nothing is installed until you confirm the list that follows.
              Closing stops the downloads still to come.
            </p>
          )}
        </>
      ) : outcomes ? (
        <>
          <p role="status">
            {outcomes.every((o) => o.done)
              ? `All ${outcomes.length} ${forUpdates ? "updated" : "installed"}.`
              : `${outcomes.filter((o) => o.done).length} of ${outcomes.length} ${forUpdates ? "updated" : "installed"}; the rest are listed with why.`}
          </p>
          <ul className="text-xs space-y-1">
            {outcomes.map((outcome) => (
              <li key={outcome.name}>
                <span className="font-medium">{outcome.name}</span>:{" "}
                {outcome.message}
              </li>
            ))}
          </ul>
          <div className="flex justify-end">
            <Button onClick={onClose}>Done</Button>
          </div>
        </>
      ) : (
        <>
          <ul className="text-xs divide-y divide-[var(--border)]">
            {entries.map((entry) => (
              <li
                key={entry.item.path}
                className="py-2 flex items-start justify-between gap-2"
              >
                <span className="space-y-0.5">
                  <span className="flex items-center gap-2">
                    <span className="font-medium break-all">
                      {itemName(entry.item)}
                    </span>
                    <StatusBadge variant={TONE[entry.status]}>
                      {forUpdates && entry.status === "failed"
                        ? "Cannot download"
                        : forUpdates && entry.status === "upgrade"
                          ? "Update"
                          : LABEL[entry.status]}
                    </StatusBadge>
                  </span>
                  {entry.item.preview && (
                    <span className="block text-[var(--fg-muted)]">
                      {entry.item.preview.detected_components
                        .map((c) => `${c.name} ${c.version}`)
                        .join(", ")}
                    </span>
                  )}
                  {originLine(entry.item) && (
                    <span className="block text-[var(--fg-muted)]">
                      {originLine(entry.item)}
                    </span>
                  )}
                  <span className="block">{entry.reason}</span>
                  {WILL_INSTALL.has(entry.status) &&
                    entry.item.preview?.warnings.map((warning) => (
                      <span
                        key={warning}
                        className="block text-[var(--warning)]"
                      >
                        {warning}
                      </span>
                    ))}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => remove(entry)}
                  aria-label={`Remove ${itemName(entry.item)} from the batch`}
                >
                  Remove
                </Button>
              </li>
            ))}
          </ul>
          {busy && job && (
            <ul className="text-xs space-y-0.5">
              {job.items.map((item, index) => (
                <li key={index}>
                  {item.name}: {item.state}
                </li>
              ))}
              <li className="font-semibold">{summarise(job.items).line}</li>
            </ul>
          )}
          {busy && progress && (
            <div className="flex items-center justify-between gap-2 text-xs">
              <p role="status">{progress}</p>
              <Button
                size="sm"
                variant="secondary"
                disabled={stopping}
                onClick={() => {
                  stopRequested.current = true;
                  setStopping(true);
                }}
              >
                {stopping ? "Stopping after this one…" : "Stop after this one"}
              </Button>
            </div>
          )}
          {busy && (
            <p className="text-xs text-[var(--fg-muted)]">
              An archive that has started installing always finishes or is
              rolled back; stopping skips the ones not yet started.
            </p>
          )}
          <p className="text-xs text-[var(--fg-muted)]">
            {forUpdates
              ? "Each update replaces its mod on its own, as a single update does: settings are kept and a restore point is saved first. If one fails, the others still go ahead, and the result lists what happened to each."
              : "Each archive is installed on its own, through the same checks as a single install. If one fails, the others still install, and the result lists what happened to each."}
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={close}>
              {busy ? "Hide (keeps going)" : "Cancel"}
            </Button>
            <Button
              disabled={busy || installable.length === 0}
              isLoading={busy}
              onClick={() => void install()}
            >
              {forUpdates ? "Update" : "Install"} {installable.length}
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
};
