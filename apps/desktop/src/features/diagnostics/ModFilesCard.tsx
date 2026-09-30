import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import type { ModFilesCheckDto } from "@/shared/api/generated";
import { FileSearch } from "lucide-react";

const STATUS: Record<
  string,
  { label: string; variant: "success" | "warning" | "danger" | "neutral" }
> = {
  unchanged: { label: "As installed", variant: "success" },
  changed: { label: "Changed", variant: "warning" },
  missing_folder: { label: "Folder missing", variant: "danger" },
  no_record: { label: "No install record", variant: "neutral" },
};

const list = (title: string, items: string[]) =>
  items.length > 0 && (
    <p>
      <span className="font-medium">{title}: </span>
      <span className="font-mono break-all">{items.join(", ")}</span>
    </p>
  );

/**
 * Compares each mod folder with the files the manager installed, to find
 * changes made outside the manager. It only reads.
 */
export const ModFilesCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const [results, setResults] = useState<ModFilesCheckDto[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const profileId = overview?.profile.id;
  if (!profileId) return null;

  const check = async () => {
    setBusy(true);
    setError(null);
    try {
      setResults(await api.checkModFiles(profileId));
    } catch (checkError) {
      setError(errorSummary(checkError, "The files could not be checked"));
    } finally {
      setBusy(false);
    }
  };

  const flagged = results?.filter((r) => r.status !== "unchanged") ?? [];

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <FileSearch className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Check mod files</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)]">
        Compares every mod folder with the files that were installed, to find
        changes made outside the manager. Edited config.json files and files a
        mod created itself are normal and shown separately.
      </p>
      <Button size="sm" variant="secondary" onClick={check} isLoading={busy}>
        {results ? "Check again" : "Check files"}
      </Button>
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
      {results && (
        <div className="text-xs space-y-2" aria-live="polite">
          <p>
            {results.length - flagged.length} of {results.length} mod folder(s)
            match what was installed.
          </p>
          <ul className="space-y-2">
            {results
              .filter(
                (r) =>
                  r.status !== "unchanged" ||
                  r.added.length > 0 ||
                  r.config_changed.length > 0,
              )
              .map((r) => (
                <li
                  key={r.deployment_id}
                  className="p-2 border border-[var(--border)] rounded-lg space-y-1"
                >
                  <div className="flex items-center gap-2">
                    <StatusBadge
                      variant={STATUS[r.status]?.variant ?? "neutral"}
                    >
                      {STATUS[r.status]?.label ?? r.status}
                    </StatusBadge>
                    <span className="font-semibold">{r.mods.join(", ")}</span>
                  </div>
                  {r.status === "no_record" && (
                    <p className="text-[var(--fg-muted)]">
                      Installed before the manager kept file records, so there
                      is nothing to compare with.
                    </p>
                  )}
                  {r.status === "missing_folder" && (
                    <p>The mod's folder is not where the manager put it.</p>
                  )}
                  {list("Missing", r.missing)}
                  {list("Changed", r.modified)}
                  {list("Settings edited", r.config_changed)}
                  {list("Added since install", r.added)}
                </li>
              ))}
          </ul>
        </div>
      )}
    </Card>
  );
};
