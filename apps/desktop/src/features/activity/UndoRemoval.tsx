import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { RemovalUndoResultDto } from "@/shared/api/generated";

/** What undoing a removal did, in a sentence. */
export function undoOutcome(result: RemovalUndoResultDto): string {
  const names = result.installed.join(", ") || "The mod";
  if (result.settings_problem) {
    return `Installed again: ${names}. ${result.settings_problem}`;
  }
  if (result.settings_restored > 0) {
    return `Installed again: ${names}, with the settings it had (${result.settings_restored} settings file(s) restored).`;
  }
  return `Installed again: ${names}. It had no settings file to bring back.`;
}

/**
 * Undoes a finished removal: the mod is installed again from its stored
 * archive (checked like any install) and its settings are restored from the
 * copy the removal kept in the recovery folder.
 */
export const UndoRemovalButton: React.FC<{
  operationId: string;
  /** Asked before anything happens, when set. */
  confirmText?: string;
  label?: string;
  size?: "sm" | "md";
}> = ({ operationId, confirmText, label = "Undo", size = "sm" }) => {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const undo = async () => {
    if (confirmText && !window.confirm(confirmText)) return;
    setBusy(true);
    setStatus(null);
    try {
      const result = await api.undoRemoval(operationId);
      setFailed(false);
      setStatus(undoOutcome(result));
    } catch (undoError) {
      setFailed(true);
      setStatus(errorSummary(undoError, "It was not installed again"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      {!status || failed ? (
        <Button
          size={size}
          variant="secondary"
          isLoading={busy}
          onClick={() => void undo()}
        >
          {label}
        </Button>
      ) : null}
      {status && (
        <span
          role={failed ? "alert" : "status"}
          className={failed ? "text-[var(--danger)]" : undefined}
        >
          {status}
        </span>
      )}
    </span>
  );
};
