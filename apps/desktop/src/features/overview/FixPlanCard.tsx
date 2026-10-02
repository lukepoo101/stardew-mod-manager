import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview, useProfileMods } from "@/shared/api/hooks";
import {
  missingRequirement,
  planFixes,
  uniqueStoredFix,
} from "@/shared/health/fixPlan";
import { useQuery } from "@tanstack/react-query";
import { Wrench } from "lucide-react";

/**
 * Offers the fixes the manager can make with certainty (enabling a required
 * mod that is installed but turned off), each with the finding behind it, and
 * lists what it leaves alone and why. Nothing changes until confirmed.
 */
export const FixPlanCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const { data: mods } = useProfileMods(overview?.profile.id);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const missing = (overview?.health_summary.findings ?? [])
    .map((finding) => ({ finding, need: missingRequirement(finding) }))
    .filter((m) => m.need !== null);
  // A missing requirement is fixable only when exactly one stored package
  // provides a version that meets it.
  const { data: storedFixes } = useQuery({
    queryKey: [
      "stored-fixes",
      missing.map((m) => `${m.need?.uniqueId}@${m.need?.minimum}`).join(","),
    ],
    queryFn: async () =>
      Promise.all(
        missing.map(async (m) => ({
          finding: m.finding,
          need: m.need,
          candidate: uniqueStoredFix(
            await api.findStoredMod(
              m.need?.uniqueId ?? "",
              m.need?.minimum ?? null,
            ),
          ),
        })),
      ),
    enabled: missing.length > 0,
  });

  if (!overview || !mods) return null;
  const { fixes, leftAlone: allLeft } = planFixes(
    overview.health_summary.findings,
    mods,
  );
  const installs = (storedFixes ?? []).filter((f) => f.candidate !== null);
  const installed = new Set(installs.map((f) => f.finding.fingerprint));
  const leftAlone = allLeft.filter(
    (item) => !installed.has(item.finding.fingerprint),
  );
  if (
    fixes.length === 0 &&
    installs.length === 0 &&
    leftAlone.length === 0 &&
    !status
  )
    return null;

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      const installedNames: string[] = [];
      const installFailures: string[] = [];
      for (const fix of installs) {
        if (!fix.candidate) continue;
        try {
          await api.installStoredPackage(
            overview.profile.id,
            fix.candidate.artifact_hash,
            true,
          );
          installedNames.push(`${fix.candidate.name} ${fix.candidate.version}`);
        } catch (installError) {
          installFailures.push(
            `${fix.candidate.name} (${errorSummary(installError, "not installed")})`,
          );
        }
      }
      const result =
        fixes.length > 0
          ? await api.setModsEnabled(
              fixes.map((f) => f.mod.profile_component_id),
              true,
            )
          : { changed: [], failed: [] };
      setStatus(
        `${
          installedNames.length > 0
            ? `Installed ${installedNames.join(", ")}. `
            : ""
        }${installFailures.length > 0 ? `Not installed: ${installFailures.join("; ")}. ` : ""}Enabled ${result.changed.join(", ") || "nothing"}.${
          result.failed.length > 0
            ? ` Not changed: ${result.failed.map((f) => `${f.name} (${f.message})`).join("; ")}.`
            : ""
        }${leftAlone.length > 0 ? " Some findings still need you; see below." : ""}`,
      );
    } catch (applyError) {
      setError(errorSummary(applyError, "Nothing was changed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Wrench className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Straightforward fixes</h3>
      </div>
      {fixes.length + installs.length > 0 && (
        <div className="text-xs space-y-2">
          <ul className="list-disc pl-4 space-y-0.5">
            {installs.map((fix) => (
              <li key={fix.finding.fingerprint}>
                Install {fix.candidate?.name} {fix.candidate?.version} from the
                copy the manager stores ({fix.candidate?.original_filename}
                ), because: {fix.finding.summary}
              </li>
            ))}
            {fixes.map((fix) => (
              <li key={fix.mod.profile_component_id}>
                Enable {fix.mod.name} {fix.mod.version}, because:{" "}
                {fix.because.summary}
              </li>
            ))}
          </ul>
          <Button
            size="sm"
            disabled={busy}
            isLoading={busy}
            onClick={() => void apply()}
          >
            Make {fixes.length + installs.length} fix(es)
          </Button>
        </div>
      )}
      {leftAlone.length > 0 && (
        <div className="text-xs space-y-1">
          <p className="font-semibold">Left for you</p>
          <ul className="list-disc pl-4 space-y-0.5">
            {leftAlone.map((item) => (
              <li key={item.finding.fingerprint}>
                {item.finding.title}: {item.reason}
              </li>
            ))}
          </ul>
        </div>
      )}
      {status && (
        <p role="status" className="text-xs">
          {status}
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
