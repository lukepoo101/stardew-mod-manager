import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useKnownGood,
  useRestorePoints,
} from "@/shared/api/hooks";
import type { RestorePlanDto } from "@/shared/api/generated";
import { Bookmark } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { UnfinishedChanges } from "@/components/ui/UnfinishedChanges";

const section = (title: string, items: string[]) =>
  items.length > 0 && (
    <div>
      <p className="font-semibold">{title}</p>
      <ul className="list-disc pl-4">
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </div>
  );

/**
 * Named restore points: save the profile's mods as they are, and later put
 * them back exactly. Restoring shows its plan first, refuses when a needed
 * package is gone, and saves the current state first so it can be undone.
 */
export const RestorePointsCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: points } = useRestorePoints(profileId);
  const { data: knownGood } = useKnownGood(profileId);
  const [label, setLabel] = useState("");
  const [plan, setPlan] = useState<{
    label: string;
    plan: RestorePlanDto;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  // An optional save backup to put back after the mods, as its own step.
  const [saveChoice, setSaveChoice] = useState<string>("");
  const { data: saves } = useQuery({
    queryKey: ["saves"],
    queryFn: () => api.listSaves(),
    enabled: Boolean(plan),
  });
  const saveOptions = (saves?.saves ?? [])
    .slice()
    .sort(
      (a, b) =>
        Number(b.profile_id === profileId) - Number(a.profile_id === profileId),
    )
    .flatMap((save) =>
      save.backups.map((backup) => ({
        id: backup.id,
        label: `${save.farm_name ?? save.id} (${save.farmer_name ?? "?"}), backed up ${new Date(backup.created_at).toLocaleString()}${backup.note ? ` for ${backup.note}` : ""}`,
      })),
    );
  const chosenSave = saveOptions.find((o) => o.id === saveChoice);

  if (!profileId) return null;

  const run = async (action: () => Promise<string | void>) => {
    setBusy(true);
    setStatus(null);
    try {
      const message = await action();
      if (message) setStatus(message);
    } catch (error) {
      setStatus(errorSummary(error, "That did not work"));
    } finally {
      setBusy(false);
    }
  };

  /** Builds a new profile from a point, after saying what it can and cannot. */
  const recreate = (
    pointId: string,
    label: string,
    mods: readonly { artifact_hash: string }[],
  ) =>
    run(async () => {
      const name = window.prompt(
        `Name for the new profile made from "${label}":`,
        `${overview?.profile.name ?? "Profile"} (${label})`.slice(0, 60),
      );
      if (!name?.trim()) return;
      const stored = new Set(
        (
          await api.storedPackages([
            ...new Set(mods.map((m) => m.artifact_hash)),
          ])
        ).map((h) => h.toLowerCase()),
      );
      const missing = mods.filter(
        (m) => !stored.has(m.artifact_hash.toLowerCase()),
      ).length;
      if (
        !window.confirm(
          `Create "${name.trim()}" with the ${mods.length} mod(s) "${label}" recorded, at the same versions and enabled state?${
            missing > 0
              ? ` ${missing} of them cannot be recreated because their exact archive is no longer stored; nothing else is put in their place.`
              : ""
          } This profile is not changed.`,
        )
      )
        return;
      const result = await api.recreateProfileFromPoint(
        profileId,
        pointId,
        name.trim(),
      );
      return result.failures.length === 0
        ? `Created "${result.profile_name}" with ${result.installed.length} mod(s).`
        : `Created "${result.profile_name}"; not recreated: ${result.failures
            .map((f) => f.name)
            .join(", ")}.`;
    });

  const nothingToDo =
    plan &&
    plan.plan.available &&
    plan.plan.remove.length +
      plan.plan.install.length +
      plan.plan.change_version.length +
      plan.plan.enable.length +
      plan.plan.disable.length +
      plan.plan.settings.length ===
      0;

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Bookmark className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Restore points</h3>
      </div>
      <form
        className="flex gap-2 text-xs"
        onSubmit={(event) => {
          event.preventDefault();
          void run(async () => {
            await api.createRestorePoint(profileId, label);
            setLabel("");
            return "Restore point saved.";
          });
        }}
      >
        <input
          type="text"
          value={label}
          maxLength={80}
          onChange={(event) => setLabel(event.target.value)}
          placeholder="Name, e.g. Before trying new farm mods"
          aria-label="Restore point name"
          className="flex-1 px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)]"
        />
        <Button size="sm" type="submit" disabled={busy || !label.trim()}>
          Save restore point
        </Button>
      </form>
      <UnfinishedChanges
        profileId={profileId}
        kind="restore_point"
        actions={(change) => (
          <>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() =>
                run(async () =>
                  setPlan({
                    label:
                      points?.find((p) => p.id === change.resume_target)
                        ?.label ?? "the same point",
                    plan: await api.planRestore(
                      profileId,
                      change.resume_target,
                    ),
                  }),
                )
              }
            >
              Review finishing it
            </Button>
            {change.undo_point_id && (
              <Button
                size="sm"
                variant="secondary"
                disabled={busy}
                onClick={() =>
                  run(async () =>
                    setPlan({
                      label: "how it was before",
                      plan: await api.planRestore(
                        profileId,
                        change.undo_point_id as string,
                      ),
                    }),
                  )
                }
              >
                Review undoing it
              </Button>
            )}
          </>
        )}
      />
      {knownGood && (
        <div className="text-xs flex items-center justify-between gap-2 py-2 border-b border-[var(--border)]">
          <span>
            <span className="font-medium">The last working setup</span>{" "}
            <span className="text-[var(--fg-muted)]">
              {new Date(knownGood.recorded_at).toLocaleString()} ·{" "}
              {knownGood.mods.length} mod(s) · versions included
            </span>
          </span>
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() =>
              run(async () =>
                setPlan({
                  label: "the last working setup",
                  plan: await api.planRestore(profileId, "known-good"),
                }),
              )
            }
          >
            Review restore
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() =>
              recreate("known-good", "the last working setup", knownGood.mods)
            }
          >
            Recreate as new profile
          </Button>
        </div>
      )}
      {points && points.length > 0 ? (
        <ul className="text-xs divide-y divide-[var(--border)]">
          {points.map((point) => (
            <li
              key={point.id}
              className="py-2 flex items-center justify-between gap-2"
            >
              <span>
                <span className="font-medium">{point.label}</span>{" "}
                <span className="text-[var(--fg-muted)]">
                  {new Date(point.created_at).toLocaleString()} ·{" "}
                  {point.mods.length} mod(s)
                  {point.settings.length > 0
                    ? ` · settings of ${point.settings.length}`
                    : ""}
                  {point.operations.length > 0
                    ? ` · saved before ${point.operations.length} change(s) shown in Activity`
                    : ""}
                </span>
              </span>
              <span className="flex gap-1">
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={busy}
                  onClick={() =>
                    run(async () =>
                      setPlan({
                        label: point.label,
                        plan: await api.planRestore(profileId, point.id),
                      }),
                    )
                  }
                >
                  Review restore
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => recreate(point.id, point.label, point.mods)}
                >
                  Recreate as new profile
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() =>
                    run(() => api.deleteRestorePoint(profileId, point.id))
                  }
                  aria-label={`Delete restore point ${point.label}`}
                >
                  Delete
                </Button>
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-xs text-[var(--fg-muted)]">
          No restore points yet. Save one before trying something risky.
        </p>
      )}
      {status && (
        <p role="status" className="text-xs">
          {status}
        </p>
      )}
      {plan && (
        <Modal
          labelledBy="restore-plan-title"
          onClose={busy ? undefined : () => setPlan(null)}
          className="space-y-3 text-xs"
        >
          <h2 id="restore-plan-title" className="text-lg font-bold">
            Restore "{plan.label}"?
          </h2>
          {!plan.plan.available ? (
            <div role="alert" className="space-y-1">
              <p className="text-[var(--danger)]">
                This restore point cannot be restored, so nothing will change:
              </p>
              <ul className="list-disc pl-4">
                {plan.plan.unavailable.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </div>
          ) : nothingToDo ? (
            <p>The profile already matches this restore point.</p>
          ) : (
            <>
              {section("Remove", plan.plan.remove)}
              {section("Install", plan.plan.install)}
              {section("Change version", plan.plan.change_version)}
              {section("Enable", plan.plan.enable)}
              {section("Disable", plan.plan.disable)}
              {section("Put back saved settings", plan.plan.settings)}
              {plan.plan.settings_unavailable.length > 0 && (
                <p>
                  The saved settings of{" "}
                  {plan.plan.settings_unavailable.join(", ")} are no longer kept
                  (cleaned up), so their current settings stay.
                </p>
              )}
              <p className="text-[var(--fg-muted)]">
                The current state, with its settings, is saved as a new restore
                point first, so this can be undone. Restore points you make keep
                mods' settings; automatic ones keep the mods only.
              </p>
            </>
          )}
          {saveOptions.length > 0 && (
            <div className="space-y-1 border-t border-[var(--border)] pt-2">
              <label className="block space-y-1">
                <span className="font-semibold">
                  Also put a save back (optional, a separate step)
                </span>
                <select
                  className="w-full px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
                  value={saveChoice}
                  onChange={(event) => setSaveChoice(event.target.value)}
                >
                  <option value="">No, leave saves alone</option>
                  {saveOptions.map((option) => (
                    <option key={option.id} value={option.id}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
              {chosenSave && (
                <ol className="list-decimal pl-4">
                  <li>
                    {plan.plan.available && !nothingToDo
                      ? "The mods are restored as listed above. This does not touch any save."
                      : "The mods are left as they are."}
                  </li>
                  <li>
                    Then {chosenSave.label} is put back. The live save is backed
                    up first, and this step does not change any mods. If the
                    mods are not fully restored, the save is left alone.
                  </li>
                </ol>
              )}
            </div>
          )}
          <div className="flex justify-end gap-2">
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => setPlan(null)}
            >
              Cancel
            </Button>
            {((plan.plan.available && !nothingToDo) || chosenSave) && (
              <Button
                isLoading={busy}
                onClick={() =>
                  run(async () => {
                    let message = "";
                    if (plan.plan.available && !nothingToDo) {
                      const result = await api.restoreToPoint(
                        profileId,
                        plan.plan.point_id,
                      );
                      if (result.failed.length > 0) {
                        setPlan(null);
                        return `Restored with problems: ${result.failed.join("; ")}${
                          chosenSave
                            ? " The save was not put back, because the mods were not fully restored."
                            : ""
                        }`;
                      }
                      message = `Restored "${plan.label}".`;
                    }
                    if (chosenSave) {
                      await api.restoreSaveBackup(chosenSave.id);
                      message += ` Put back ${chosenSave.label}; the live save was backed up first.`;
                    }
                    setPlan(null);
                    setSaveChoice("");
                    return message.trim();
                  })
                }
              >
                {chosenSave
                  ? plan.plan.available && !nothingToDo
                    ? "Restore mods, then the save"
                    : "Put the save back"
                  : "Restore"}
              </Button>
            )}
          </div>
        </Modal>
      )}
    </Card>
  );
};
