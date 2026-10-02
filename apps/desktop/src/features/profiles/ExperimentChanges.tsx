import React, { useState } from "react";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useProfileMods } from "@/shared/api/hooks";
import type { ModListItemDto } from "@/shared/api/generated";

export type ExperimentChange =
  | { kind: "added"; mod: ModListItemDto }
  | { kind: "removed"; source: ModListItemDto }
  | { kind: "version"; mod: ModListItemDto; source: ModListItemDto }
  | { kind: "enabled"; mod: ModListItemDto; source: ModListItemDto };

/** What the experiment changed compared with the profile it was copied from. */
export function experimentChanges(
  source: readonly ModListItemDto[],
  experiment: readonly ModListItemDto[],
): ExperimentChange[] {
  const key = (m: ModListItemDto) => m.unique_id.toLowerCase();
  const before = new Map(source.map((m) => [key(m), m]));
  const after = new Map(experiment.map((m) => [key(m), m]));
  const changes: ExperimentChange[] = [];
  for (const [id, mod] of after) {
    const was = before.get(id);
    if (!was) changes.push({ kind: "added", mod });
    else if (was.artifact_hash !== mod.artifact_hash)
      changes.push({ kind: "version", mod, source: was });
    else if (was.enabled !== mod.enabled)
      changes.push({ kind: "enabled", mod, source: was });
  }
  for (const [id, was] of before) {
    if (!after.has(id)) changes.push({ kind: "removed", source: was });
  }
  return changes;
}

/**
 * Lists the experiment's changes and applies a chosen one to the original
 * profile through the normal reviewed workflows. Nothing is applied in bulk
 * or without its own confirmation.
 */
export const ExperimentChanges: React.FC<{
  sourceId: string;
  sourceName: string;
  experimentId: string;
}> = ({ sourceId, sourceName, experimentId }) => {
  const { data: source, refetch } = useProfileMods(sourceId);
  const { data: experiment } = useProfileMods(experimentId);
  const [status, setStatus] = useState<string | null>(null);
  if (!source || !experiment) return null;
  const changes = experimentChanges(source, experiment);

  const apply = async (change: ExperimentChange) => {
    const [question, action, done] =
      change.kind === "added"
        ? [
            `Install ${change.mod.name} ${change.mod.version} in "${sourceName}" too? It goes through the normal install checks.`,
            () => api.installStoredPackage(sourceId, change.mod.artifact_hash),
            `Installed ${change.mod.name} in "${sourceName}".`,
          ]
        : change.kind === "version"
          ? [
              `Change ${change.source.name} in "${sourceName}" from ${change.source.version} to ${change.mod.version}? It is replaced through the reviewed replace, keeping its settings.`,
              () => api.replaceModVersion(sourceId, change.mod.artifact_hash),
              `${change.mod.name} in "${sourceName}" is now ${change.mod.version}.`,
            ]
          : change.kind === "enabled"
            ? [
                `Turn ${change.mod.name} ${change.mod.enabled ? "on" : "off"} in "${sourceName}" too?`,
                () =>
                  api.setModsEnabled(
                    [change.source.profile_component_id],
                    change.mod.enabled,
                  ),
                `${change.mod.name} is now ${change.mod.enabled ? "on" : "off"} in "${sourceName}".`,
              ]
            : [
                `Turn ${change.source.name} off in "${sourceName}"? It is only turned off, not removed, so you can undo it.`,
                () =>
                  api.setModsEnabled(
                    [change.source.profile_component_id],
                    false,
                  ),
                `${change.source.name} is now off in "${sourceName}".`,
              ];
    if (!window.confirm(question)) return;
    setStatus(null);
    try {
      await action();
      setStatus(done);
      await refetch();
    } catch (error) {
      setStatus(errorSummary(error, "Nothing was changed"));
    }
  };

  const describe = (change: ExperimentChange) => {
    switch (change.kind) {
      case "added":
        return `${change.mod.name} ${change.mod.version} was installed`;
      case "removed":
        return `${change.source.name} was removed`;
      case "version":
        return `${change.mod.name} is ${change.mod.version}, was ${change.source.version}`;
      case "enabled":
        return `${change.mod.name} was turned ${change.mod.enabled ? "on" : "off"}`;
    }
  };

  return (
    <div className="space-y-1">
      <p className="font-semibold">Changes compared with "{sourceName}"</p>
      {changes.length === 0 ? (
        <p className="text-[var(--fg-muted)]">No mod changes yet.</p>
      ) : (
        <ul className="space-y-1">
          {changes.map((change) => (
            <li
              key={`${change.kind}:${"mod" in change ? change.mod.unique_id : change.source.unique_id}`}
              className="flex flex-wrap items-center gap-2"
            >
              <span>{describe(change)}</span>
              <button
                type="button"
                className="underline cursor-pointer"
                onClick={() => void apply(change)}
              >
                Apply to "{sourceName}"
              </button>
            </li>
          ))}
        </ul>
      )}
      {status && <p role="status">{status}</p>}
    </div>
  );
};
