import React from "react";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import { smapiBadge, smapiExplanation } from "@/shared/smapi/status";

/**
 * The SMAPI in the active game folder, next to the version this manager is
 * tested with. These are separate facts; neither is the "latest" release.
 */
export const SmapiCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const status = overview?.smapi_status;
  if (!status) return null;
  const badge = smapiBadge(status);
  return (
    <Card className="space-y-3 text-sm">
      <div className="flex items-center justify-between">
        <h3 className="font-bold">SMAPI</h3>
        <StatusBadge variant={badge.variant}>{badge.label}</StatusBadge>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
        <dt className="text-[var(--fg-muted)]">Installed</dt>
        <dd>
          {status.state === "absent"
            ? "None"
            : (status.observed_version ?? "Version unknown")}
          {status.state === "partial" ? " (incomplete)" : ""}
        </dd>
        <dt className="text-[var(--fg-muted)]">Tested with this manager</dt>
        <dd>{status.tested_version}</dd>
        <dt className="text-[var(--fg-muted)]">Latest release</dt>
        <dd>Not checked. No update source is connected.</dd>
      </dl>
      <p className="text-xs">{smapiExplanation(status)}</p>
      {status.evidence.length > 0 && (
        <details className="text-xs text-[var(--fg-muted)]">
          <summary className="cursor-pointer">What was checked</summary>
          <ul className="list-disc pl-4">
            {status.evidence.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      )}
    </Card>
  );
};
