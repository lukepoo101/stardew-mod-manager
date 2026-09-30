import React, { useMemo, useState } from "react";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useProfileMods, useProfiles } from "@/shared/api/hooks";
import {
  compareProfiles,
  type ProfileDifference,
} from "@/shared/profiles/compare";
import { GitCompare } from "lucide-react";

const LABELS: Record<ProfileDifference["kind"], string> = {
  only_a: "Only in the first",
  only_b: "Only in the second",
  version: "Different version",
  package: "Same version, different file",
  enabled: "Enabled in one only",
};

function describe(d: ProfileDifference, a: string, b: string): string {
  switch (d.kind) {
    case "only_a":
      return `${d.a.version}, ${d.a.enabled ? "enabled" : "disabled"}`;
    case "only_b":
      return `${d.b.version}, ${d.b.enabled ? "enabled" : "disabled"}`;
    case "version":
      return `${a}: ${d.a.version}, ${b}: ${d.b.version} (newer in ${
        d.newer === "a" ? a : b
      })`;
    case "package":
      return `${d.a.version} in both, installed from different archives`;
    case "enabled":
      return `enabled in ${d.a.enabled ? a : b}, disabled in ${
        d.a.enabled ? b : a
      }`;
  }
}

/**
 * Compares the mods of two profiles. It only reads: changing either profile is
 * done through the normal install, remove and enable workflows.
 */
export const ProfileCompareCard: React.FC = () => {
  const { data: profiles } = useProfiles();
  const [first, setFirst] = useState("");
  const [second, setSecond] = useState("");
  const { data: modsA, error: errorA } = useProfileMods(first || undefined);
  const { data: modsB, error: errorB } = useProfileMods(second || undefined);
  const nameA = profiles?.find((p) => p.id === first)?.name ?? "first";
  const nameB = profiles?.find((p) => p.id === second)?.name ?? "second";

  const comparison = useMemo(
    () =>
      first && second && first !== second && modsA && modsB
        ? compareProfiles(modsA, modsB)
        : null,
    [first, second, modsA, modsB],
  );

  if (!profiles || profiles.length < 2) return null;

  const select = (
    label: string,
    value: string,
    onChange: (value: string) => void,
  ) => (
    <label className="flex items-center gap-2 text-xs">
      <span className="font-medium">{label}</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
      >
        <option value="">Choose a profile</option>
        {profiles.map((profile) => (
          <option key={profile.id} value={profile.id}>
            {profile.name}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <Card className="space-y-4">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <GitCompare className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Compare two profiles</h3>
      </div>
      <div className="flex flex-wrap gap-4">
        {select("First", first, setFirst)}
        {select("Second", second, setSecond)}
      </div>
      {first && first === second && (
        <p className="text-xs text-[var(--fg-muted)]">
          Choose two different profiles.
        </p>
      )}
      {(errorA || errorB) && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          The mods of one of these profiles could not be loaded.
        </p>
      )}
      {comparison && (
        <div className="space-y-2" aria-live="polite">
          <p className="text-xs">
            <StatusBadge
              variant={comparison.differences.length === 0 ? "success" : "info"}
            >
              {comparison.differences.length === 0
                ? "Same mods"
                : `${comparison.differences.length} difference(s)`}
            </StatusBadge>{" "}
            {comparison.identical} mod(s) are the same version, file and enabled
            state in both. Matching is by UniqueID; changes made to a mod's
            files outside the manager are not detected here.
          </p>
          {comparison.differences.length > 0 && (
            <table className="w-full text-xs">
              <thead>
                <tr className="text-left text-[var(--fg-muted)]">
                  <th className="py-1 font-medium">Mod</th>
                  <th className="py-1 font-medium">Difference</th>
                  <th className="py-1 font-medium">Detail</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border)]">
                {comparison.differences.map((d) => {
                  const mod = "a" in d ? d.a : d.b;
                  return (
                    <tr key={`${d.kind}:${mod.profile_component_id}`}>
                      <td className="py-1 pr-2">
                        <span className="font-medium">{mod.name}</span>{" "}
                        <span className="font-mono text-[var(--fg-muted)]">
                          {mod.unique_id}
                        </span>
                      </td>
                      <td className="py-1 pr-2">{LABELS[d.kind]}</td>
                      <td className="py-1">{describe(d, nameA, nameB)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
      )}
    </Card>
  );
};
