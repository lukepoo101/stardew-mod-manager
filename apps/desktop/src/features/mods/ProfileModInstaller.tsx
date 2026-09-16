import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { ModDropZone } from "./ModDropZone";
import { api } from "@/shared/api/client";
import { useExecuteOperation } from "@/shared/api/hooks";
import { OperationPreviewDto } from "@/shared/api/generated";

export const ProfileModInstaller: React.FC<{ profileId: string }> = ({
  profileId,
}) => {
  const [preview, setPreview] = useState<OperationPreviewDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const execute = useExecuteOperation();
  const busy = execute.isPending || cancelling;
  const cancel = async () => {
    if (!preview) return;
    setCancelling(true);
    setError(null);
    try {
      await api.cancelActiveOperation(preview.operation_id);
      setPreview(null);
    } catch (error) {
      setError(String(error));
    } finally {
      setCancelling(false);
    }
  };

  return (
    <>
      <ModDropZone
        onArchiveSelected={async (path) => {
          setError(null);
          setPreview(await api.inspectPackageForInstall(path, profileId));
        }}
      />
      {preview && (
        <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-6">
          <section
            role="dialog"
            aria-modal="true"
            aria-labelledby="install-review-title"
            className="bg-[var(--bg-surface)] border border-[var(--border)] rounded-xl p-6 w-full max-w-lg space-y-4"
          >
            <h2 id="install-review-title" className="text-xl font-bold">
              Review mod installation
            </h2>
            <p className="break-all">{preview.original_filename}</p>
            <ul>
              {preview.detected_components.map((component) => (
                <li key={component.unique_id}>
                  {component.name} {component.version} — {component.author}
                </li>
              ))}
            </ul>
            {preview.warnings.map((warning) => (
              <p key={warning}>{warning}</p>
            ))}
            {preview.blockers.map((blocker) => (
              <p role="alert" key={blocker}>
                {blocker}
              </p>
            ))}
            {error && <p role="alert">{error}</p>}
            <div className="flex justify-end gap-3">
              <Button variant="secondary" disabled={busy} onClick={cancel}>
                Cancel
              </Button>
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
                    setPreview(null);
                  } catch (error) {
                    setError(String(error));
                  }
                }}
              >
                Install mod
              </Button>
            </div>
          </section>
        </div>
      )}
    </>
  );
};
