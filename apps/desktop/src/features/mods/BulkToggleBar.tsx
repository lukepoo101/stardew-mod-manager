import { Modal } from "@/components/ui/Modal";
import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import type {
  BulkToggleResultDto,
  ToggleImpactDto,
} from "@/shared/api/generated";

/**
 * Enables or disables the selected mods as one reviewed change. The review
 * lists every mod that moves (including ones sharing a package), mods outside
 * the selection that would stop loading, and requirements left unmet.
 */
export const BulkToggleBar: React.FC<{
  selectedIds: string[];
  onClear: () => void;
}> = ({ selectedIds, onClear }) => {
  const [plan, setPlan] = useState<{
    enable: boolean;
    impact: ToggleImpactDto;
  } | null>(null);
  const [result, setResult] = useState<BulkToggleResultDto | null>(null);
  const { data: overview, isFetching: healthRefreshing } =
    useActiveProfileOverview();
  const health = overview?.health_summary;
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const review = async (enable: boolean) => {
    setError(null);
    setResult(null);
    setBusy(true);
    try {
      setPlan({
        enable,
        impact: await api.getBulkToggleImpact(selectedIds, enable),
      });
    } catch (reviewError) {
      setError(errorSummary(reviewError, "Could not check what this affects"));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!plan) return;
    setBusy(true);
    try {
      const outcome = await api.setModsEnabled(selectedIds, plan.enable);
      setResult(outcome);
      setPlan(null);
      if (outcome.failed.length === 0) onClear();
    } catch (applyError) {
      setError(errorSummary(applyError, "The change did not run"));
      setPlan(null);
    } finally {
      setBusy(false);
    }
  };

  if (selectedIds.length === 0 && !result) return null;

  return (
    <div className="space-y-2">
      {selectedIds.length > 0 && (
        <div
          role="toolbar"
          aria-label="Selected mods"
          className="flex flex-wrap items-center gap-2 p-3 rounded-lg border border-[var(--accent-primary)]/40 bg-[var(--accent-primary)]/5 text-xs"
        >
          <span className="font-medium">{selectedIds.length} selected</span>
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => review(true)}
          >
            Enable selected
          </Button>
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => review(false)}
          >
            Disable selected
          </Button>
          <Button size="sm" variant="ghost" onClick={onClear}>
            Clear selection
          </Button>
        </div>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
      {result && (
        <div role="status" className="text-xs space-y-1">
          <p>
            {result.changed.length} mod(s) changed
            {result.failed.length > 0
              ? `; ${result.failed.length} could not be changed. Running it again finishes the job once the cause is fixed.`
              : "."}
          </p>
          {result.failed.length > 0 && (
            <ul className="list-disc pl-4 text-[var(--fg-muted)]">
              {result.failed.map((failure) => (
                <li key={failure.name}>
                  {failure.name}: {failure.message}
                </li>
              ))}
            </ul>
          )}
          {health && (
            <p>
              Setup health now
              {healthRefreshing ? " (rechecking)" : ""}: {health.error_count}{" "}
              error(s), {health.warning_count} warning(s). See Diagnostics for
              details.
            </p>
          )}
        </div>
      )}
      {plan && (
        <Modal
          labelledBy="bulk-toggle-title"
          onClose={busy ? undefined : () => setPlan(null)}
          className="space-y-3 text-sm"
        >
          <h2 id="bulk-toggle-title" className="text-lg font-bold">
            {plan.enable ? "Enable" : "Disable"}{" "}
            {plan.impact.affected_mods.length} mod(s)?
          </h2>
          <p className="text-xs text-[var(--fg-muted)]">
            Mods that came in the same package move together, so this list can
            be longer than your selection.
          </p>
          <ul className="text-xs list-disc pl-4 max-h-40 overflow-y-auto">
            {plan.impact.affected_mods.map((name) => (
              <li key={name}>{name}</li>
            ))}
          </ul>
          {plan.impact.dependents.length > 0 && (
            <p className="text-xs text-[var(--warning)]">
              These enabled mods need one of them and will not load while it is
              disabled: {plan.impact.dependents.join(", ")}.
            </p>
          )}
          {plan.impact.unmet_requirements.map((line) => (
            <p key={line} className="text-xs text-[var(--warning)]">
              {line}
            </p>
          ))}
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setPlan(null)}>
              Cancel
            </Button>
            <Button onClick={apply} isLoading={busy}>
              {plan.enable ? "Enable" : "Disable"}
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
};
