import React, { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import type {
  DependencyMapEntryDto,
  ModRequirementDto,
} from "@/shared/api/generated";
import { GitBranch } from "lucide-react";

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

/** A requirement is a problem when it is required and not met. */
const broken = (r: ModRequirementDto) =>
  r.kind !== "optional" && r.status !== "satisfied";

/** Whether anything under this entry, following required edges, is broken. */
function hasProblem(
  entry: DependencyMapEntryDto,
  byId: Map<string, DependencyMapEntryDto>,
  seen: Set<string> = new Set(),
): boolean {
  if (seen.has(entry.unique_id.toLowerCase())) return false;
  seen.add(entry.unique_id.toLowerCase());
  return entry.requires.some((r) => {
    if (broken(r)) return true;
    if (r.kind === "optional") return false;
    const next = byId.get(r.unique_id.toLowerCase());
    return next ? hasProblem(next, byId, seen) : false;
  });
}

const Node: React.FC<{
  requirement: ModRequirementDto;
  byId: Map<string, DependencyMapEntryDto>;
  path: string[];
}> = ({ requirement, byId, path }) => {
  const id = requirement.unique_id.toLowerCase();
  const target = byId.get(id);
  const cycle = path.includes(id);
  const status = STATUS[requirement.status];
  return (
    <li className={requirement.kind === "optional" ? "opacity-70" : ""}>
      <span className="inline-flex items-center gap-1.5 flex-wrap">
        <StatusBadge
          variant={
            requirement.kind === "optional" &&
            requirement.status !== "satisfied"
              ? "neutral"
              : (status?.variant ?? "neutral")
          }
        >
          {status?.label ?? requirement.status}
        </StatusBadge>
        <span>
          {requirement.name ?? requirement.unique_id}
          {requirement.installed_version
            ? ` ${requirement.installed_version}`
            : ""}
          {requirement.minimum_version
            ? ` (needs ${requirement.minimum_version}+)`
            : ""}
        </span>
        <span className="text-[var(--fg-muted)]">
          {KIND[requirement.kind] ?? requirement.kind}
        </span>
        {cycle && (
          <span className="text-[var(--fg-muted)]">
            , which loops back to a mod above
          </span>
        )}
      </span>
      {target && !cycle && target.requires.length > 0 && path.length < 8 && (
        <ul className="pl-4 border-l border-[var(--border)] ml-1 space-y-1 mt-1">
          {target.requires.map((r) => (
            <Node
              key={`${r.kind}:${r.unique_id}`}
              requirement={r}
              byId={byId}
              path={[...path, id]}
            />
          ))}
        </ul>
      )}
    </li>
  );
};

/**
 * Every mod's requirements as a tree, from the mods nothing else needs down
 * to the frameworks they rest on. Optional links are shown but never counted
 * as problems; a loop is marked where it closes instead of repeating.
 */
export const DependencyMapCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: entries } = useQuery({
    queryKey: ["dependency-map", profileId],
    queryFn: () => api.getDependencyMap(profileId ?? ""),
    enabled: Boolean(profileId),
    staleTime: 3000,
  });
  const [problemsOnly, setProblemsOnly] = useState(false);
  const byId = useMemo(
    () => new Map((entries ?? []).map((e) => [e.unique_id.toLowerCase(), e])),
    [entries],
  );
  if (!entries) return null;
  // Start from mods nothing needs; a loop with no such mod still shows once.
  let roots = entries.filter(
    (e) => e.required_by.length === 0 && e.requires.length > 0,
  );
  if (roots.length === 0) roots = entries.filter((e) => e.requires.length > 0);
  if (problemsOnly) roots = roots.filter((e) => hasProblem(e, byId));

  return (
    <Card className="space-y-3">
      <div className="flex items-center justify-between gap-2 border-b border-[var(--border)] pb-3">
        <div className="flex items-center gap-2">
          <GitBranch className="w-4 h-4 text-[var(--accent-primary)]" />
          <h3 className="font-bold text-sm">Dependency map</h3>
        </div>
        <label className="flex items-center gap-1.5 text-xs">
          <input
            type="checkbox"
            checked={problemsOnly}
            onChange={(event) => setProblemsOnly(event.target.checked)}
          />
          Only mods with a problem
        </label>
      </div>
      {roots.length === 0 ? (
        <p className="text-xs text-[var(--fg-muted)]">
          {problemsOnly
            ? "No mod has an unmet requirement."
            : "No mod in this profile declares a requirement."}
        </p>
      ) : (
        <ul className="text-xs space-y-3">
          {roots.map((entry) => (
            <li key={entry.profile_component_id}>
              <p className="font-semibold">
                {entry.name} {entry.version}
                {entry.enabled ? "" : " (disabled)"}
              </p>
              <ul className="pl-4 border-l border-[var(--border)] ml-1 space-y-1 mt-1">
                {entry.requires.map((r) => (
                  <Node
                    key={`${r.kind}:${r.unique_id}`}
                    requirement={r}
                    byId={byId}
                    path={[entry.unique_id.toLowerCase()]}
                  />
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
};
