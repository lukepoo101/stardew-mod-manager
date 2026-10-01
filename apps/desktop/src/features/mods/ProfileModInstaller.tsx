import { Modal } from "@/components/ui/Modal";
import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { ModDropZone } from "./ModDropZone";
import { InstallResult } from "./InstallResult";
import { api } from "@/shared/api/client";
import { useExecuteOperation, useProfiles } from "@/shared/api/hooks";
import { formatBytes } from "@/features/settings/StorageCleanupCard";
import { OperationPreviewDto } from "@/shared/api/generated";
import { MOD_TRUST_SUMMARY } from "@/shared/security/trust";
import { errorRecoverability, errorSummary } from "@/shared/api/errors";

export const ProfileModInstaller: React.FC<{ profileId: string }> = ({
  profileId,
}) => {
  const { data: profiles } = useProfiles();
  const profileName = profiles?.find((p) => p.id === profileId)?.name;
  const [preview, setPreview] = useState<OperationPreviewDto | null>(null);
  const [installed, setInstalled] = useState<OperationPreviewDto | null>(null);
  const [archivePath, setArchivePath] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [cancelling, setCancelling] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [downgradeConfirmed, setDowngradeConfirmed] = useState(false);
  const [replaced, setReplaced] = useState<string | null>(null);
  const execute = useExecuteOperation();
  const busy = execute.isPending || cancelling || refreshing || replacing;
  // The "already installed" blocker is what a replacement resolves; any other
  // blocker still stops it.
  const replaces = preview?.replaces ?? [];
  const otherBlockers =
    preview?.blockers.filter((b) => !/already installed/i.test(b)) ?? [];
  const isDowngrade = replaces.some((r) => r.direction === "downgrade");

  const replace = async () => {
    if (!preview) return;
    setReplacing(true);
    setError(null);
    try {
      // The preview's own draft must not hold the profile during the swap.
      await api.cancelActiveOperation(preview.operation_id);
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
          result.restore_point
            ? " A restore point was saved first; you can go back from Profiles."
            : ""
        }`,
      );
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

  const refreshPreview = async () => {
    if (!archivePath) return;
    setRefreshing(true);
    setError(null);
    try {
      setPreview(await api.inspectPackageForInstall(archivePath, profileId));
    } catch (refreshError) {
      setError(refreshError);
    } finally {
      setRefreshing(false);
    }
  };

  const cancel = async () => {
    if (!preview) return;
    setCancelling(true);
    setError(null);
    try {
      await api.cancelActiveOperation(preview.operation_id);
      setPreview(null);
    } catch (cancelError) {
      setError(cancelError);
    } finally {
      setCancelling(false);
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
          onClick={refreshPreview}
        >
          Refresh preview
        </Button>
      )}
    </div>
  ) : null;

  return (
    <>
      <ModDropZone
        onArchiveSelected={async (path) => {
          setError(null);
          setArchivePath(path);
          setPreview(await api.inspectPackageForInstall(path, profileId));
        }}
      />
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
                a file on this computer. Where it came from is not verified.
              </dd>
            </div>
            <div>
              <dt className="inline font-semibold">SHA-256: </dt>
              <dd className="inline font-mono break-all">
                {preview.artifact_hash}
              </dd>
            </div>
          </dl>
          <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
            {MOD_TRUST_SUMMARY} The checks above the button confirm the archive
            is well formed, not what the mod's code does.
          </p>
          <ul>
            {preview.detected_components.map((component) => (
              <li key={component.unique_id}>
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
          {preview.warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
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
          {(replaces.length > 0 ? otherBlockers : preview.blockers).map(
            (blocker) => (
              <p role="alert" key={blocker}>
                {blocker}
              </p>
            ),
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
                  !preview.dependencies_satisfied ||
                  preview.blockers.length > 0
                }
                isLoading={execute.isPending}
                onClick={async () => {
                  setError(null);
                  try {
                    await execute.mutateAsync(preview.operation_id);
                    setInstalled(preview);
                    setPreview(null);
                  } catch (executeError) {
                    setError(executeError);
                  }
                }}
              >
                Install mod
              </Button>
            )}
          </div>
        </Modal>
      )}
    </>
  );
};
