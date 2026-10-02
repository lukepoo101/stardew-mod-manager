import React from "react";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { CopyButton } from "@/components/ui/CopyButton";
import type { GameInspectionDto } from "@/shared/api/generated";
import { installationLabel } from "@/shared/platform/labels";
import { describeSupportState } from "@/shared/onboarding/supportState";

/**
 * What was found at one game folder: where it is, what it is, whether it can
 * be used and why not, with the raw evidence one click away.
 */
export const InspectionSummary: React.FC<{
  inspection: GameInspectionDto;
  /** Offered for an already-modded installation only. */
  onContinueUnmanaged?: () => void;
  busy?: boolean;
}> = ({ inspection, onContinueUnmanaged, busy }) => {
  const state = describeSupportState(inspection.support_state);
  const storefront =
    inspection.storefront.toLowerCase() === "manual"
      ? "Chosen by hand, store unknown"
      : installationLabel(inspection.storefront, inspection.operating_system);
  return (
    <div className="space-y-2 text-xs">
      <div className="flex items-center gap-2 flex-wrap">
        <StatusBadge variant={state.tone}>{state.label}</StatusBadge>
        <StatusBadge variant="info">{storefront}</StatusBadge>
        <span className="px-2 py-0.5 rounded-md bg-[var(--bg-elevated)] border border-[var(--border)] font-mono">
          {inspection.detected_version
            ? `v${inspection.detected_version}`
            : "Version unknown"}
        </span>
        {inspection.has_existing_smapi && (
          <StatusBadge variant="info">SMAPI found</StatusBadge>
        )}
      </div>
      <p className="flex items-center gap-1 font-mono break-all select-text text-[var(--fg-muted)]">
        <span>{inspection.candidate_path}</span>
        <CopyButton value={inspection.candidate_path} label="game folder" />
      </p>
      {(!inspection.is_usable ||
        inspection.support_state === "supported_managed") && (
        <div
          className={`space-y-1 ${
            state.tone === "danger"
              ? "text-[var(--danger)]"
              : state.tone === "warning"
                ? "text-[var(--warning)]"
                : ""
          }`}
        >
          <p>{state.explanation}</p>
          {state.next && (
            <p>
              <span className="font-semibold">What to do: </span>
              {state.next}
            </p>
          )}
        </div>
      )}
      {inspection.support_state === "existing_modded_unmanaged" &&
        onContinueUnmanaged && (
          <div className="p-2 rounded-lg border border-[var(--border)] space-y-1">
            <p>
              For advanced users: add it without letting the manager change it.
              SMAPI will not be installed or changed there, and its Mods folder
              is left exactly as it is. It is marked as not managed until a
              supported way to take it over exists.
            </p>
            <button
              type="button"
              disabled={busy}
              onClick={onContinueUnmanaged}
              className="text-[var(--accent-primary)] hover:underline cursor-pointer disabled:opacity-50"
            >
              Continue without managing it
            </button>
          </div>
        )}
      {inspection.evidence.length > 0 && (
        <details className="text-[var(--fg-muted)]">
          <summary className="cursor-pointer">What was checked</summary>
          <ul className="list-disc pl-4 mt-1 select-text">
            {inspection.evidence.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          <p className="flex items-center gap-1">
            State: <span className="font-mono">{inspection.support_state}</span>
            <CopyButton
              value={[
                inspection.candidate_path,
                `State: ${inspection.support_state}`,
                ...inspection.evidence,
              ].join("\n")}
              label="these details"
            />
          </p>
        </details>
      )}
    </div>
  );
};
