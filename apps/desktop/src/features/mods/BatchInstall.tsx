import React, { useRef, useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  type BatchEntry,
  type BatchItem,
  type BatchStatus,
  WILL_INSTALL,
  classifyBatch,
  fileName,
} from "@/shared/mods/batch";

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

/**
 * Inspects several archives, shows what each will do, and installs the ones
 * that can be installed, each on its own. Nothing is installed until the user
 * confirms; removing an archive from the list just leaves it out.
 */
export const BatchInstall: React.FC<{
  profileId: string;
  paths: string[];
  onClose: () => void;
}> = ({ profileId, paths, onClose }) => {
  const [items, setItems] = useState<BatchItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [outcomes, setOutcomes] = useState<Outcome[] | null>(null);
  // Where a long batch has got to, announced as it changes.
  const [progress, setProgress] = useState<string | null>(null);
  // A stop request is honoured between archives: one that is installing
  // finishes (or is rolled back) first, never left half done.
  const stopRequested = useRef(false);
  const [stopping, setStopping] = useState(false);

  // Inspect each archive once, one after another. A profile holds one
  // prepared change at a time, so each draft is discarded once read and the
  // archive is prepared again when it is installed.
  React.useEffect(() => {
    let current = true;
    (async () => {
      const inspected: BatchItem[] = [];
      for (const [index, path] of paths.entries()) {
        if (current)
          setProgress(
            `Checking ${index + 1} of ${paths.length}: ${fileName(path)}`,
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
  }, [paths, profileId]);

  const entries = items ? classifyBatch(items) : [];
  const installable = entries.filter((e) => WILL_INSTALL.has(e.status));

  const remove = (entry: BatchEntry) => {
    setItems((list) => list?.filter((item) => item !== entry.item) ?? null);
  };

  const close = () => onClose();

  /** Prepares an archive again and commits it, or says why not. */
  const installOne = async (path: string): Promise<Outcome> => {
    const name = fileName(path);
    try {
      const fresh = await api.inspectPackageForInstall(path, profileId);
      if (fresh.blockers.length > 0) {
        await api.cancelActiveOperation(fresh.operation_id);
        return { name, done: false, message: fresh.blockers.join(" ") };
      }
      await api.executeOperation(fresh.operation_id);
      return { name, done: true, message: "Installed." };
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
    // What other archives need goes first.
    const order = [
      ...entries.filter((e) => e.status !== "after_others"),
      ...entries.filter((e) => e.status === "after_others"),
    ];
    const total = order.filter((e) => WILL_INSTALL.has(e.status)).length;
    let started = 0;
    for (const entry of order) {
      const name = fileName(entry.item.path);
      const preview = entry.item.preview;
      if (stopRequested.current && WILL_INSTALL.has(entry.status)) {
        results.push({
          name,
          done: false,
          message: "Not started: you stopped the batch.",
        });
        continue;
      }
      if (WILL_INSTALL.has(entry.status) && preview) {
        started += 1;
        setProgress(`Installing ${started} of ${total}: ${name}`);
      }
      if (!WILL_INSTALL.has(entry.status) || !preview) {
        results.push({ name, done: false, message: entry.reason });
      } else if (entry.status === "upgrade") {
        try {
          await api.replaceModVersion(profileId, preview.artifact_hash);
          results.push({ name, done: true, message: "Replaced." });
        } catch (error) {
          results.push({
            name,
            done: false,
            message: errorSummary(error, "It was not replaced."),
          });
        }
      } else {
        results.push(await installOne(entry.item.path));
      }
    }
    setOutcomes(results);
    setProgress(null);
    setBusy(false);
  };

  return (
    <Modal
      labelledBy="batch-install-title"
      onClose={busy ? undefined : close}
      className="space-y-3 text-sm"
    >
      <h2 id="batch-install-title" className="text-lg font-bold">
        Install {paths.length} archives
      </h2>
      {!items ? (
        <p role="status">{progress ?? "Checking each archive…"}</p>
      ) : outcomes ? (
        <>
          <p role="status">
            {outcomes.filter((o) => o.done).length} of {outcomes.length}{" "}
            installed.
          </p>
          <ul className="text-xs space-y-1">
            {outcomes.map((outcome) => (
              <li key={outcome.name}>
                <span className="font-medium">{outcome.name}</span>:{" "}
                {outcome.done ? "installed" : outcome.message}
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
                      {fileName(entry.item.path)}
                    </span>
                    <StatusBadge variant={TONE[entry.status]}>
                      {LABEL[entry.status]}
                    </StatusBadge>
                  </span>
                  {entry.item.preview && (
                    <span className="block text-[var(--fg-muted)]">
                      {entry.item.preview.detected_components
                        .map((c) => `${c.name} ${c.version}`)
                        .join(", ")}
                    </span>
                  )}
                  <span className="block">{entry.reason}</span>
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => remove(entry)}
                  aria-label={`Remove ${fileName(entry.item.path)} from the batch`}
                >
                  Remove
                </Button>
              </li>
            ))}
          </ul>
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
            Each archive is installed on its own, through the same checks as a
            single install. If one fails, the others still install, and the
            result lists what happened to each.
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="secondary" disabled={busy} onClick={close}>
              Cancel
            </Button>
            <Button
              disabled={busy || installable.length === 0}
              isLoading={busy}
              onClick={() => void install()}
            >
              Install {installable.length}
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
};
