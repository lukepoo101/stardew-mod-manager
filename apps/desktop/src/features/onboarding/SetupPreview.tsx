import React from "react";
import { useQuery } from "@tanstack/react-query";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { SetupPreviewDto } from "@/shared/api/generated";

const List: React.FC<{ title: string; items: string[] }> = ({
  title,
  items,
}) =>
  items.length === 0 ? null : (
    <div className="space-y-1">
      <h4 className="font-semibold">{title}</h4>
      <ul className="list-disc pl-4 space-y-0.5 break-all">
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </div>
  );

/**
 * What SMAPI setup will do, from the backend's own plan, and whether every
 * location it needs can be used. Nothing has changed while this is shown.
 */
export const SetupPreview: React.FC<{
  gameId?: string;
  /** A new value asks for the preview again. */
  attempt?: number;
  onReady: (preview: SetupPreviewDto | null) => void;
}> = ({ gameId, attempt = 0, onReady }) => {
  const { data: preview, error } = useQuery({
    queryKey: ["setup-preview", gameId, attempt],
    queryFn: () => api.previewSmapiSetup(gameId ?? ""),
    enabled: Boolean(gameId),
    staleTime: 0,
  });
  React.useEffect(() => {
    onReady(preview ?? null);
  }, [preview, onReady]);

  if (error) {
    return (
      <p role="alert" className="text-xs text-[var(--danger)]">
        {errorSummary(error, "The setup checks did not run")}
      </p>
    );
  }
  if (!preview) {
    return (
      <p role="status" className="text-xs text-[var(--fg-muted)]">
        Checking what setup will need...
      </p>
    );
  }
  const failed = preview.checks.filter((check) => !check.ok);
  return (
    <div className="space-y-3 text-xs">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 className="font-bold text-sm">SMAPI {preview.smapi_version}</h3>
          <p className="text-[var(--fg-muted)]">
            The release this version of the manager is tested with, for Stardew
            Valley {preview.supported_game_version}.
          </p>
        </div>
        <StatusBadge variant="info">Pinned release</StatusBadge>
      </div>
      <List title="Will change the game folder" items={preview.modifies} />
      <List title="Will create manager-owned files" items={preview.creates} />
      <List title="Will only read" items={preview.reads} />
      <List title="Already there" items={preview.notices} />
      <div className="space-y-1">
        <h4 className="font-semibold">Access checks</h4>
        <ul className="space-y-1">
          {preview.checks.map((check) => (
            <li key={check.path} className="flex gap-2 items-start">
              <StatusBadge variant={check.ok ? "success" : "danger"}>
                {check.ok ? "OK" : "Problem"}
              </StatusBadge>
              <span className="break-all">
                {check.label} ({check.path}) needs {check.needs}.
                {check.problem && (
                  <span className="block text-[var(--danger)]">
                    {check.problem}
                  </span>
                )}
                {check.remedy && (
                  <span className="block">What to do: {check.remedy}</span>
                )}
              </span>
            </li>
          ))}
        </ul>
        <p className="text-[var(--fg-muted)]">
          Passing these checks now does not guarantee the install succeeds;
          setup still stops safely if something changes.
        </p>
      </div>
      {failed.length > 0 && (
        <p role="alert" className="text-[var(--danger)]">
          Setup will not start until the problem above is fixed. Nothing has
          been changed.
        </p>
      )}
      <details className="text-[var(--fg-muted)]">
        <summary className="cursor-pointer">Download details</summary>
        <p className="break-all">Source: {preview.smapi_source}</p>
        <p className="break-all">SHA-256: {preview.smapi_sha256}</p>
      </details>
    </div>
  );
};
