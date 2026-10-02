import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview, useProfileMods } from "@/shared/api/hooks";
import { planFixes } from "@/shared/health/fixPlan";
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

  if (!overview || !mods) return null;
  const { fixes, leftAlone } = planFixes(
    overview.health_summary.findings,
    mods,
  );
  if (fixes.length === 0 && leftAlone.length === 0 && !status) return null;

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.setModsEnabled(
        fixes.map((f) => f.mod.profile_component_id),
        true,
      );
      setStatus(
        `Enabled ${result.changed.join(", ") || "nothing"}.${
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
      {fixes.length > 0 && (
        <div className="text-xs space-y-2">
          <ul className="list-disc pl-4 space-y-0.5">
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
            Enable {fixes.length} mod(s)
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
