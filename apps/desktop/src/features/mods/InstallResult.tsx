import { Modal } from "@/components/ui/Modal";
import React, { useEffect, useRef } from "react";
import { Link } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import type { OperationPreviewDto } from "@/shared/api/generated";

/**
 * What an install actually did, shown after it commits. Success and anything
 * still needing attention are listed separately, so a warning is never hidden
 * by a successful install. Closing it keeps the operation in Activity.
 */
export const InstallResult: React.FC<{
  preview: OperationPreviewDto;
  onClose: () => void;
  onInstallAnother: () => void;
}> = ({ preview, onClose, onInstallAnother }) => {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  const count = preview.detected_components.length;
  const attention = [
    ...(preview.dependencies_satisfied
      ? []
      : ["Some required mods are not installed or enabled yet."]),
    ...preview.warnings,
  ];

  return (
    <Modal
      labelledBy="install-result-title"
      onClose={onClose}
      className="space-y-4"
    >
      <h2
        id="install-result-title"
        ref={heading}
        tabIndex={-1}
        className="text-xl font-bold outline-none"
      >
        Installed {count === 1 ? "1 mod" : `${count} mods`}
      </h2>
      <p className="text-xs text-[var(--fg-muted)] break-all">
        From {preview.original_filename}
        {count > 1 ? ", which contained several mods" : ""}.
      </p>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-left text-[var(--fg-muted)]">
            <th className="py-1 font-medium">Mod</th>
            <th className="py-1 font-medium">UniqueID</th>
            <th className="py-1 font-medium">Version</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-[var(--border)]">
          {preview.detected_components.map((component) => (
            <tr key={component.unique_id}>
              <td className="py-1 pr-2">{component.name}</td>
              <td className="py-1 pr-2 font-mono break-all">
                {component.unique_id}
              </td>
              <td className="py-1 font-mono">{component.version}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {attention.length > 0 && (
        <div className="p-3 rounded-lg border border-[var(--warning)]/40 text-xs space-y-1">
          <p className="font-semibold text-[var(--warning)]">
            Installed, but check these before playing
          </p>
          <ul className="list-disc pl-4">
            {attention.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          <p className="text-[var(--fg-muted)]">
            Each mod's details show what it needs and what is missing.
          </p>
        </div>
      )}

      <div className="flex flex-wrap justify-end gap-2">
        <Link
          to="/app/activity"
          onClick={onClose}
          className="px-3 py-2 text-xs font-medium text-[var(--accent-primary)] hover:underline"
        >
          View in Activity
        </Link>
        <Button variant="secondary" size="sm" onClick={onInstallAnother}>
          Install another
        </Button>
        <Link
          to="/app/overview"
          onClick={onClose}
          className="px-3 py-2 text-xs font-medium text-[var(--accent-primary)] hover:underline"
        >
          Go to launch
        </Link>
        <Button size="sm" onClick={onClose}>
          Done
        </Button>
      </div>
    </Modal>
  );
};
