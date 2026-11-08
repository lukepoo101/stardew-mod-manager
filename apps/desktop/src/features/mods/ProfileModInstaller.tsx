import { Modal } from "@/components/ui/Modal";
import React, { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { ModDropZone } from "./ModDropZone";
import { InstallResult } from "./InstallResult";
import { BatchInstall } from "./BatchInstall";
import { StoredDependencyInstall } from "./StoredDependencyInstall";
import { ModUpdates, updatesFocusTarget } from "./ModUpdates";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/api/client";
import {
  useExecuteOperation,
  useProfileFreeze,
  useProfiles,
} from "@/shared/api/hooks";
import { formatBytes } from "@/features/settings/StorageCleanupCard";
import type {
  DownloadedPreviewDto,
  ModRequirementDto,
  ModUpdateDto,
  OperationPreviewDto,
} from "@/shared/api/generated";
import { MOD_TRUST_SUMMARY } from "@/shared/security/trust";
import {
  errorCode,
  errorRecoverability,
  errorSummary,
} from "@/shared/api/errors";

const ALREADY_INSTALLED = /already installed/i;

/** A requirement no mod in the profile or the archive meets, and who needs it. */
interface Unmet {
  requirement: ModRequirementDto;
  neededBy: string[];
}

/** Unmet requirements of an archive's mods, one per UniqueID. */
export function unmetRequirements(preview: OperationPreviewDto): Unmet[] {
  const byId = new Map<string, Unmet>();
  for (const component of preview.detected_components) {
    for (const requirement of component.requirements) {
      if (requirement.kind === "optional") continue;
      if (requirement.status !== "missing" && requirement.status !== "too_old")
        continue;
      const key = requirement.unique_id.toLowerCase();
      const existing = byId.get(key);
      if (existing) {
        if (!existing.neededBy.includes(component.name))
          existing.neededBy.push(component.name);
      } else {
        byId.set(key, { requirement, neededBy: [component.name] });
      }
    }
  }
  return [...byId.values()];
}

const listNames = (names: string[]) =>
  names.length <= 1
    ? (names[0] ?? "")
    : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;

/** "Two A and Two B need Content Patcher 2.0 or newer, which is not installed." */
function unmetText({ requirement, neededBy }: Unmet): string {
  const what = `${requirement.name ?? `the mod with UniqueID ${requirement.unique_id}`}${
    requirement.minimum_version
      ? ` ${requirement.minimum_version} or newer`
      : ""
  }`;
  const verb = neededBy.length === 1 ? "needs" : "need";
  if (requirement.status === "too_old") {
    return `${listNames(neededBy)} ${verb} ${what}; ${requirement.installed_version ?? "an older version"} is installed.`;
  }
  return requirement.kind === "content_pack_for"
    ? `${listNames(neededBy)} ${neededBy.length === 1 ? "is a content pack" : "are content packs"} for ${what}, which is not installed.`
    : `${listNames(neededBy)} ${verb} ${what}, which is not installed.`;
}

export const ProfileModInstaller: React.FC<{ profileId: string }> = ({
  profileId,
}) => {
  const [batch, setBatch] = useState<string[] | null>(null);
  // Updates downloaded together for one review, from "Update all".
  const [updateBatch, setUpdateBatch] = useState<ModUpdateDto[] | null>(null);
  // The update row a review was opened from, so closing it returns focus
  // there; null for an archive chosen on this computer.
  const [fromRow, setFromRow] = useState<string | null>(null);
  const queryClient = useQueryClient();
  /**
   * Reads the update check again now, so an installed update has left the
   * list before focus moves (the backend's change event refreshes it too,
   * but later).
   */
  const refreshUpdates = () =>
    queryClient.refetchQueries({ queryKey: ["mod-updates", profileId] });
  const { data: profiles } = useProfiles();
  const { data: freeze } = useProfileFreeze(profileId);
  const profileName = profiles?.find((p) => p.id === profileId)?.name;
  const [preview, setPreviewState] = useState<OperationPreviewDto | null>(null);
  const [installed, setInstalled] = useState<OperationPreviewDto | null>(null);
  const [archivePath, setArchivePath] = useState<string | null>(null);
  // Where the archive under review was downloaded from; null for a file
  // chosen on this computer.
  const [downloaded, setDownloaded] = useState<Omit<
    DownloadedPreviewDto,
    "preview"
  > | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [cancelling, setCancelling] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [downgradeConfirmed, setDowngradeConfirmed] = useState(false);
  const [replaced, setReplaced] = useState<string | null>(null);
  const execute = useExecuteOperation();
  const busy = execute.isPending || cancelling || refreshing || replacing;

  // The prepared install this review holds and has not yet committed or
  // discarded. A prepared install keeps the profile for itself, so leaving
  // the review any way (Cancel, Escape, Ctrl+K, the sidebar, a route change)
  // discards it the same way. Taking the id out first means it is discarded
  // at most once, and never after it was handed to a commit.
  const draft = useRef<string | null>(null);
  // Whether the page is still there, and whether the user still has the
  // review open. A preview that arrives after either went away (a stored
  // requirement finishing, a slow first inspection, a refused commit) is
  // discarded at once instead of holding the profile or reopening by itself.
  const mounted = useRef(true);
  const reviewOpen = useRef(false);
  const setPreview = (next: OperationPreviewDto | null) => {
    draft.current = next?.operation_id ?? null;
    reviewOpen.current = next !== null;
    setPreviewState(next);
  };
  const discardLater = (id: string) =>
    void api.cancelActiveOperation(id).catch(() => undefined);
  /** Shows a freshly prepared install, or discards it if nobody can see it. */
  const show = (next: OperationPreviewDto, wanted = true) => {
    if (!mounted.current || !wanted) {
      discardLater(next.operation_id);
      return;
    }
    setPreview(next);
  };
  const discardDraft = async () => {
    const id = draft.current;
    draft.current = null;
    if (id) await api.cancelActiveOperation(id);
  };
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      reviewOpen.current = false;
      const id = draft.current;
      draft.current = null;
      if (id) discardLater(id);
    };
  }, []);

  // The "already installed" blocker is what a replacement resolves; any other
  // blocker still stops it.
  const replaces = preview?.replaces ?? [];
  const otherBlockers =
    preview?.blockers.filter((b) => !ALREADY_INSTALLED.test(b)) ?? [];
  const isDowngrade = replaces.some((r) => r.direction === "downgrade");
  const unmet = preview ? unmetRequirements(preview) : [];
  // Blockers that are not a requirement: SMAPI too old, a clash inside the
  // archive. With a replacement, "already installed" is not one.
  const packageBlockers = (preview?.other_blockers ?? []).filter(
    (b) => replaces.length === 0 || !ALREADY_INSTALLED.test(b),
  );
  const shownBlockers = replaces.length > 0 ? otherBlockers : preview?.blockers;
  // A blocker the two lists above do not explain is still shown as given.
  const unexplained =
    unmet.length === 0 && packageBlockers.length === 0
      ? (shownBlockers ?? [])
      : [];
  const frozen =
    Boolean(freeze) ||
    (error !== null && errorCode(error) === "PROFILE_FROZEN");
  const stopsLoading = replaces.flatMap((r) => r.stops_loading);
  const alsoRemoved = replaces.flatMap((r) => r.also_removed ?? []);

  const replace = async () => {
    if (!preview) return;
    setReplacing(true);
    setError(null);
    try {
      // The preview's own draft must not hold the profile during the swap.
      await discardDraft();
      const result = await api.replaceModVersion(
        profileId,
        preview.artifact_hash,
      );
      setReplaced(
        `Replaced ${result.replaced
          .map(
            (r) =>
              `${r.name} ${r.installed_version} with ${r.incoming_version}`,
          )
          .join(", ")}.${
          result.kept_settings.length > 0 ? " Your settings were kept." : ""
        }${
          result.left_disabled
            ? " It was turned off before, so it is still off."
            : ""
        }${
          result.restore_point
            ? " A restore point was saved first; you can go back from Profiles."
            : ""
        }`,
      );
      // The row it was opened from goes before the review closes, so focus
      // goes to the panel heading rather than a row about to vanish.
      if (fromRow !== null) await refreshUpdates();
      setPreview(null);
      setDowngradeConfirmed(false);
    } catch (replaceError) {
      setError(replaceError);
      setPreview(null);
    } finally {
      setReplacing(false);
    }
  };

  // A stale-plan conflict cannot be recovered by retrying the same commit; the
  // user has to regenerate the preview first. That decision comes from the
  // structured recoverability field, never from the error text.
  const canRefreshPreview =
    archivePath !== null &&
    errorRecoverability(error) === "retry_with_fresh_plan";

  /**
   * Prepares the archive again. Asked for by the user it always shows the
   * result; after a stored requirement installs it only does so while the
   * review is still open.
   */
  const refreshPreview = async (explicit = true) => {
    if (!archivePath || !mounted.current) return;
    if (!explicit && !reviewOpen.current) return;
    setRefreshing(true);
    setError(null);
    try {
      await discardDraft().catch(() => undefined);
      const next = await api.inspectPackageForInstall(archivePath, profileId);
      show(next, explicit || reviewOpen.current);
    } catch (refreshError) {
      if (!mounted.current) return;
      setPreview(null);
      setError(refreshError);
    } finally {
      if (mounted.current) setRefreshing(false);
    }
  };

  const cancel = async () => {
    if (!preview) return;
    setCancelling(true);
    setError(null);
    try {
      await discardDraft();
      setPreview(null);
    } catch (cancelError) {
      // Still prepared: leaving later must try again.
      draft.current = preview.operation_id;
      setError(cancelError);
    } finally {
      setCancelling(false);
    }
  };

  const commit = async () => {
    if (!preview) return;
    setError(null);
    // The commit owns the prepared install now; leaving must not cancel it.
    draft.current = null;
    try {
      await execute.mutateAsync(preview.operation_id);
      if (fromRow !== null) await refreshUpdates();
      setInstalled(preview);
      setPreviewState(null);
    } catch (executeError) {
      // Refused before it started, so it is still the review's to discard,
      // or, when the page has gone meanwhile, to discard now.
      if (!mounted.current) {
        discardLater(preview.operation_id);
        return;
      }
      draft.current = preview.operation_id;
      setError(executeError);
    }
  };

  const errorAlert = error ? (
    <div
      role="alert"
      className="p-3 rounded-lg bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-xs text-[var(--danger)] space-y-2"
    >
      <p>{errorSummary(error)}</p>
      {canRefreshPreview && (
        <Button
          variant="secondary"
          size="sm"
          disabled={busy}
          onClick={() => void refreshPreview()}
        >
          Refresh preview
        </Button>
      )}
    </div>
  ) : null;

  const inspect = async (path: string) => {
    setError(null);
    setArchivePath(path);
    setDownloaded(null);
    setFromRow(null);
    show(await api.inspectPackageForInstall(path, profileId));
  };

  /** An update downloaded from GitHub, reviewed like a chosen archive. */
  const showDownloaded = (
    { preview: next, ...origin }: DownloadedPreviewDto,
    mod: ModUpdateDto,
  ) => {
    setError(null);
    setFromRow(mod.profile_component_id);
    setReplaced(null);
    // Downloaded again rather than re-read from a path.
    setArchivePath(null);
    setDownloaded(origin);
    show(next);
  };

  return (
    <>
      <ModUpdates
        profileId={profileId}
        disabled={busy || preview !== null || updateBatch !== null}
        onDownloaded={showDownloaded}
        onUpdateAll={(mods) => {
          setError(null);
          setReplaced(null);
          setUpdateBatch(mods);
        }}
      />
      <ModDropZone onArchiveSelected={inspect} onArchivesSelected={setBatch} />
      <Button
        size="sm"
        variant="ghost"
        onClick={async () => {
          const paths = await api.pickArchivesDialog();
          if (paths.length === 1) {
            await inspect(paths[0]);
          } else if (paths.length > 1) {
            setBatch(paths);
          }
        }}
      >
        Install several archives...
      </Button>
      {batch && (
        <BatchInstall
          profileId={profileId}
          paths={batch}
          onClose={() => setBatch(null)}
        />
      )}
      {updateBatch && (
        <BatchInstall
          profileId={profileId}
          updates={updateBatch}
          onClose={() => {
            setUpdateBatch(null);
            void refreshUpdates();
          }}
          returnFocusTo={() => updatesFocusTarget(null)}
        />
      )}
      {!preview && errorAlert}
      {replaced && (
        <p role="status" className="text-xs">
          {replaced}
        </p>
      )}
      {installed && (
        <InstallResult
          preview={installed}
          onClose={() => setInstalled(null)}
          onInstallAnother={() => {
            setInstalled(null);
            document.getElementById("choose-mod-zip")?.click();
          }}
        />
      )}
      {preview && (
        <Modal
          labelledBy="install-review-title"
          onClose={busy ? undefined : () => void cancel()}
          returnFocusTo={
            fromRow === null ? undefined : () => updatesFocusTarget(fromRow)
          }
          className="space-y-4"
        >
          <h2 id="install-review-title" className="text-xl font-bold">
            Review mod installation
          </h2>
          <p className="break-all">{preview.original_filename}</p>
          <dl className="text-xs space-y-1">
            <div>
              <dt className="inline font-semibold">Into profile: </dt>
              <dd className="inline">{profileName ?? "the active profile"}</dd>
            </div>
            <div>
              <dt className="inline font-semibold">Archive size: </dt>
              <dd className="inline">{formatBytes(preview.byte_size)}</dd>
            </div>
            <div>
              <dt className="inline font-semibold">Source: </dt>
              <dd className="inline">
                {downloaded ? (
                  <>
                    downloaded from the GitHub release{" "}
                    <span className="font-mono">{downloaded.source.tag}</span>{" "}
                    of {downloaded.source.repo}
                    {downloaded.source.declared_by_mod
                      ? ", which the mod's manifest names"
                      : ", which the community compatibility list names"}
                    . Who published that release is not verified.
                  </>
                ) : (
                  "a file on this computer. Where it came from is not verified."
                )}
              </dd>
            </div>
            <div>
              <dt className="inline font-semibold">Integrity: </dt>
              <dd className="inline">
                fingerprint recorded (SHA-256{" "}
                <span className="font-mono break-all">
                  {preview.artifact_hash}
                </span>
                ), so later changes to the stored copy are caught.{" "}
                {downloaded?.checksum_verified
                  ? "It matches the fingerprint GitHub publishes for the file."
                  : "There is no published fingerprint to compare it with."}
              </dd>
            </div>
            <div>
              <dt className="inline font-semibold">Archive structure: </dt>
              <dd className="inline">
                checked: no paths outside the mod folder, no links, and within
                size limits.
              </dd>
            </div>
            <div>
              <dt className="inline font-semibold">What the code does: </dt>
              <dd className="inline">not assessed.</dd>
            </div>
          </dl>
          <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
            {MOD_TRUST_SUMMARY} The checks above the button confirm the archive
            is well formed, not what the mod's code does.
          </p>
          <ul>
            {preview.detected_components.map((component) => (
              <li key={`${component.unique_id}:${component.relative_root}`}>
                {component.name} {component.version} — {component.author}{" "}
                <span className="text-xs font-mono text-[var(--fg-muted)]">
                  {component.unique_id}, from{" "}
                  {component.relative_root
                    ? `"${component.relative_root}"`
                    : "the archive's top level"}
                </span>
              </li>
            ))}
          </ul>
          {(unmet.length > 0 ||
            packageBlockers.length > 0 ||
            unexplained.length > 0) && (
            <div
              role="alert"
              className="p-3 rounded-lg border border-[var(--danger)]/40 text-xs space-y-2"
            >
              <p className="font-semibold text-[var(--danger)]">
                This cannot be installed yet
              </p>
              <ul className="list-disc pl-4 space-y-2">
                {[...packageBlockers, ...unexplained].map((blocker) => (
                  <li key={blocker}>{blocker}</li>
                ))}
                {unmet.map((entry) => (
                  <li key={entry.requirement.unique_id} className="space-y-1">
                    <span className="block">{unmetText(entry)}</span>
                    <StoredDependencyInstall
                      requirement={entry.requirement}
                      requiredBy={listNames(entry.neededBy)}
                      profileId={profileId}
                      lookNow
                      beforeInstall={discardDraft}
                      afterInstall={() => refreshPreview(false)}
                    />
                  </li>
                ))}
              </ul>
              {unmet.length > 0 && (
                <p className="text-[var(--fg-muted)]">
                  Install what it needs first, then this archive. The manager
                  can install a copy it already stores; otherwise download the
                  mod with that UniqueID from its mod page.
                </p>
              )}
            </div>
          )}
          {preview.warnings.length > 0 && (
            <ul className="text-xs list-disc pl-4 space-y-1">
              {preview.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          )}
          {preview.files.length > 0 && (
            <details className="text-xs">
              <summary className="cursor-pointer text-[var(--fg-muted)]">
                All files ({preview.files.filter((f) => f.installed).length}{" "}
                installed
                {preview.files.some((f) => !f.installed)
                  ? `, ${preview.files.filter((f) => !f.installed).length} kept only in the archive`
                  : ""}
                )
              </summary>
              <table className="mt-1 w-full font-mono">
                <tbody>
                  {preview.files.map((file) => (
                    <tr key={`${file.installed}:${file.path}`}>
                      <td className="break-all pr-2">{file.path}</td>
                      <td className="text-right whitespace-nowrap pr-2">
                        {formatBytes(file.size_bytes)}
                      </td>
                      <td className="whitespace-nowrap">
                        {file.installed ? "installed" : "not installed"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </details>
          )}
          {replaces.length > 0 && (
            <div className="p-3 rounded-lg border border-[var(--border)] text-xs space-y-1">
              <p className="font-semibold">
                {isDowngrade
                  ? "This is an older version of a mod you have"
                  : "This replaces a mod you have"}
              </p>
              <ul className="list-disc pl-4">
                {replaces.map((r) => (
                  <li key={r.profile_component_id}>
                    {r.name}: {r.installed_version} → {r.incoming_version} (
                    {r.direction === "upgrade"
                      ? "newer"
                      : r.direction === "downgrade"
                        ? "older"
                        : r.direction === "same"
                          ? "same version"
                          : "different version"}
                    )
                  </li>
                ))}
              </ul>
              {stopsLoading.length > 0 && (
                <div className="text-[var(--warning)]">
                  <p className="font-semibold">
                    These enabled mods will stop loading:
                  </p>
                  <ul className="list-disc pl-4">
                    {stopsLoading.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                </div>
              )}
              {alsoRemoved.length > 0 && (
                <div className="text-[var(--warning)]">
                  <p className="font-semibold">
                    These mods were installed from the same archive and are not
                    in this one, so they will be removed too:
                  </p>
                  <ul className="list-disc pl-4">
                    {alsoRemoved.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                  <p>
                    Install them again afterwards if you still want them, or
                    keep the current version.
                  </p>
                </div>
              )}
              <p className="text-[var(--fg-muted)]">
                The installed copy is removed and this one installed. Your
                config.json settings are kept. If this one cannot be installed,
                the current version is put back.
              </p>
              {isDowngrade && (
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={downgradeConfirmed}
                    onChange={(event) =>
                      setDowngradeConfirmed(event.target.checked)
                    }
                  />
                  <span>I want the older version</span>
                </label>
              )}
            </div>
          )}
          {frozen && !error && (
            <p role="status" className="text-xs">
              This profile is frozen, so nothing can be installed into it.
              Unfreeze it on the Profiles page first.
            </p>
          )}
          {errorAlert}
          <div className="flex justify-end gap-3">
            <Button variant="secondary" disabled={busy} onClick={cancel}>
              Cancel
            </Button>
            {replaces.length > 0 ? (
              <Button
                disabled={
                  busy ||
                  frozen ||
                  otherBlockers.length > 0 ||
                  (isDowngrade && !downgradeConfirmed)
                }
                isLoading={replacing}
                onClick={replace}
              >
                {isDowngrade
                  ? "Install older version"
                  : "Replace installed version"}
              </Button>
            ) : (
              <Button
                disabled={
                  busy ||
                  frozen ||
                  !preview.dependencies_satisfied ||
                  preview.blockers.length > 0
                }
                isLoading={execute.isPending}
                onClick={commit}
              >
                {preview.detected_components.length > 1
                  ? `Install ${preview.detected_components.length} mods`
                  : "Install mod"}
              </Button>
            )}
          </div>
        </Modal>
      )}
    </>
  );
};
