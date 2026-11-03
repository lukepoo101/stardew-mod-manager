import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import type {
  ReinstallPreviewDto,
  ReinstallResultDto,
} from "@/shared/api/generated";
import { formatBytes } from "@/features/settings/StorageCleanupCard";

/**
 * What a reinstall will do, shown before it starts: the folder is rebuilt
 * from its archive and settings are kept, and each file added since install
 * is listed with a choice. Data files are kept by default; code (a DLL or
 * program) and a second manifest are not, because they would load with the
 * reinstalled mod. Files not kept are moved aside with the old folder, not
 * deleted.
 */
export const ReinstallReview: React.FC<{
  preview: ReinstallPreviewDto;
  busy: boolean;
  onConfirm: (keep: string[]) => void;
  onCancel: () => void;
}> = ({ preview, busy, onConfirm, onCancel }) => {
  const [keep, setKeep] = useState<ReadonlySet<string>>(
    () =>
      new Set(
        preview.added.filter((f) => f.keep_by_default).map((f) => f.path),
      ),
  );
  const toggle = (path: string) => {
    const next = new Set(keep);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    setKeep(next);
  };
  return (
    <section
      aria-label="Reinstall from archive"
      className="text-xs space-y-2 p-3 rounded-lg border border-[var(--border)]"
    >
      <p>
        The folder is removed and installed again from the archive it came from,
        so changed or missing files are put back as they came. Settings in
        config.json are kept, and a disabled mod stays disabled.
      </p>
      {preview.no_install_record && (
        <p>
          There is no record of what was installed, so files added to the folder
          since then cannot be told apart and are not kept. The old folder is
          kept aside with them.
        </p>
      )}
      {preview.added.length > 0 && (
        <fieldset className="space-y-1">
          <legend className="font-semibold">
            Files added since install ({preview.added.length})
          </legend>
          <p className="text-[var(--fg-muted)]">
            Ticked files are kept in the reinstalled folder. The others stay
            with the old folder in the profile's recovery folder.
          </p>
          {preview.added.map((file) => (
            <label key={file.path} className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={keep.has(file.path)}
                onChange={() => toggle(file.path)}
              />
              <span className="font-mono break-all">{file.path}</span>
              <span className="text-[var(--fg-muted)]">
                {formatBytes(file.size_bytes)}
                {file.keep_by_default
                  ? ""
                  : ", code or a manifest: not kept unless you tick it"}
              </span>
            </label>
          ))}
        </fieldset>
      )}
      <div className="flex gap-2">
        <Button
          size="sm"
          variant="secondary"
          onClick={onCancel}
          disabled={busy}
        >
          Cancel
        </Button>
        <Button
          size="sm"
          variant="primary"
          onClick={() => onConfirm([...keep])}
          disabled={busy}
          isLoading={busy}
        >
          Reinstall
        </Button>
      </div>
    </section>
  );
};

/** What a finished reinstall did, in one line. */
export function reinstallSummary(result: ReinstallResultDto): string {
  return [
    `Reinstalled ${result.mods.join(", ")}.`,
    result.kept_settings.length > 0
      ? `Kept settings: ${result.kept_settings.join(", ")}.`
      : "",
    result.kept_added.length > 0
      ? `Kept files added since install: ${result.kept_added.join(", ")}.`
      : "",
    result.left_out.length > 0
      ? `Not kept: ${result.left_out.join(", ")}${
          result.removed_copy ? `, still in ${result.removed_copy}` : ""
        }.`
      : "",
    result.no_install_record
      ? "There is no record of what was installed, so files added to the folder since then could not be told apart and were not kept."
      : "",
    result.settings_backup ? "A backup of the settings was saved first." : "",
    result.restore_point
      ? "A restore point was saved on the Profiles page."
      : "",
  ]
    .filter(Boolean)
    .join(" ");
}
