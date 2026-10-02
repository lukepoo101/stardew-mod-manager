import React from "react";
import { useQuery } from "@tanstack/react-query";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import type { ModRequirementDto } from "@/shared/api/generated";
import { referenceReason } from "@/shared/mods/referenceReason";

const STATUS: Record<
  string,
  { label: string; variant: "success" | "warning" | "danger" | "neutral" }
> = {
  satisfied: { label: "OK", variant: "success" },
  missing: { label: "Not installed", variant: "danger" },
  disabled: { label: "Disabled", variant: "warning" },
  too_old: { label: "Too old", variant: "danger" },
};

const KIND: Record<string, string> = {
  required: "required",
  optional: "optional",
  content_pack_for: "content pack host",
};

function requirementText(r: ModRequirementDto): string {
  const needs = r.minimum_version ? ` ${r.minimum_version} or newer` : "";
  const have = r.installed_version ? `, ${r.installed_version} installed` : "";
  return `${r.name ?? r.unique_id}${needs}${have}`;
}

/**
 * Why a mod is installed, what it needs, what needs it, and the full path to
 * any requirement that is not met. Optional dependencies are shown but never
 * reported as problems.
 */
export const ModRelationsPanel: React.FC<{
  profileComponentId: string;
  /** With both, a group reference that lists the mod is given as a reason. */
  profileId?: string;
  uniqueId?: string;
}> = ({ profileComponentId, profileId, uniqueId }) => {
  const { data: reference } = useQuery({
    queryKey: ["reference-recipe", profileId],
    queryFn: () => api.getReferenceRecipe(profileId ?? ""),
    enabled: Boolean(profileId),
    staleTime: 3000,
  });
  const fromReference = uniqueId ? referenceReason(reference, uniqueId) : null;
  const { data: relations, error } = useQuery({
    queryKey: ["mod-relations", profileComponentId],
    queryFn: () => api.getModRelations(profileComponentId),
    staleTime: 3000,
  });

  if (error) {
    return (
      <p role="alert" className="text-xs text-[var(--danger)]">
        The mod's relationships could not be loaded.
      </p>
    );
  }
  if (!relations) return null;

  return (
    <div className="space-y-3 text-xs">
      <div className="space-y-1">
        <h4 className="font-bold text-[var(--fg-muted)] uppercase tracking-wider">
          Why it is installed
        </h4>
        <p>{relations.reason_detail}</p>
        {fromReference && <p>{fromReference}</p>}
      </div>

      {relations.broken_chains.length > 0 && (
        <div className="space-y-1" role="alert">
          <h4 className="font-bold text-[var(--danger)] uppercase tracking-wider">
            Unmet requirements
          </h4>
          <ul className="space-y-0.5 font-mono">
            {relations.broken_chains.map((chain) => (
              <li key={chain.join(">")}>{chain.join(" → ")}</li>
            ))}
          </ul>
          <p className="text-[var(--fg-muted)]">
            Each line ends at a requirement that is missing, disabled or too
            old.
          </p>
        </div>
      )}

      <div className="space-y-1">
        <h4 className="font-bold text-[var(--fg-muted)] uppercase tracking-wider">
          Needs
        </h4>
        {relations.requires.length === 0 ? (
          <p className="text-[var(--fg-muted)]">No other mods.</p>
        ) : (
          <ul className="space-y-1">
            {relations.requires.map((r) => (
              <li key={`${r.kind}:${r.unique_id}`} className="flex gap-2">
                <StatusBadge
                  variant={
                    r.kind === "optional" && r.status !== "satisfied"
                      ? "neutral"
                      : (STATUS[r.status]?.variant ?? "neutral")
                  }
                >
                  {STATUS[r.status]?.label ?? r.status}
                </StatusBadge>
                <span>
                  {requirementText(r)}{" "}
                  <span className="text-[var(--fg-muted)]">
                    ({KIND[r.kind] ?? r.kind})
                  </span>
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="space-y-1">
        <h4 className="font-bold text-[var(--fg-muted)] uppercase tracking-wider">
          Needed by
        </h4>
        {relations.required_by.length === 0 ? (
          <p className="text-[var(--fg-muted)]">
            No other mod in this profile depends on it.
          </p>
        ) : (
          <ul className="space-y-0.5">
            {relations.required_by.map((d) => (
              <li key={d.profile_component_id}>
                {d.name}
                {d.minimum_version ? ` (needs ${d.minimum_version}+)` : ""}{" "}
                <span className="text-[var(--fg-muted)]">
                  {KIND[d.kind] ?? d.kind}
                  {d.enabled ? "" : ", disabled"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
};
