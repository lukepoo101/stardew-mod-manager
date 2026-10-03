import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import { Gamepad2 } from "lucide-react";

/**
 * The detected game version, and a way to set the right one when detection
 * gets it wrong or cannot read it. Only the game version can be set this
 * way; folder locations, ownership and safety checks never can. The setting
 * is shown wherever it is used, and flagged if detection changes later.
 */
export const GameVersionCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const gameId = overview?.game.id;
  const client = useQueryClient();
  const queryKey = ["game-version-override", gameId];
  const { data } = useQuery({
    queryKey,
    queryFn: () => api.getGameVersionOverride(gameId ?? ""),
    enabled: Boolean(gameId),
  });
  const [version, setVersion] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  if (!gameId || !data) return null;

  const save = async (value: string | null) => {
    setError(null);
    try {
      await api.setGameVersionOverride(gameId, value, reason);
      setVersion("");
      setReason("");
      await client.refetchQueries({ queryKey });
    } catch (saveError) {
      setError(errorSummary(saveError, "Nothing was changed"));
    }
  };

  return (
    <Card className="space-y-3 text-xs">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Gamepad2 className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Game version</h3>
      </div>
      <p>Detected: {data.detected ?? "could not be read"}.</p>
      {data.value ? (
        <div className="space-y-1">
          <p>
            Set by you: <span className="font-semibold">{data.value}</span>
            {data.reason ? ` (${data.reason})` : ""}, on{" "}
            {data.set_at ? new Date(data.set_at).toLocaleString() : "?"}. Checks
            use this instead of the detected version.
          </p>
          {data.stale && (
            <p role="alert" className="text-[var(--warning)]">
              Detection has changed since you set it (it read{" "}
              {data.detected_then ?? "nothing"} then). Check whether your
              setting still applies.
            </p>
          )}
          <Button size="sm" variant="secondary" onClick={() => void save(null)}>
            Use the detected version again
          </Button>
        </div>
      ) : (
        <form
          className="flex flex-wrap gap-2 items-end"
          onSubmit={(event) => {
            event.preventDefault();
            void save(version);
          }}
        >
          <label className="space-y-1">
            <span className="block">Set the version yourself</span>
            <input
              className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
              value={version}
              placeholder="e.g. 1.6.15"
              onChange={(event) => setVersion(event.target.value)}
            />
          </label>
          <label className="space-y-1 flex-1">
            <span className="block">Why (optional)</span>
            <input
              className="w-full px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          </label>
          <Button size="sm" type="submit" disabled={!version.trim()}>
            Use this version
          </Button>
        </form>
      )}
      <p className="text-[var(--fg-muted)]">
        Only the game version can be set here. Health always shows when it comes
        from you, and support summaries include it.
      </p>
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
