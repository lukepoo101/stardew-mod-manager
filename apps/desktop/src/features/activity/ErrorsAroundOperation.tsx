import React from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/api/client";
import { useLaunchSessions } from "@/shared/api/hooks";
import { compareLogs } from "@/shared/launch/compareSessions";
import { sessionsAround } from "@/shared/launch/operationErrors";

/**
 * Which SMAPI errors first appeared in the session after an operation,
 * compared with the session before it. This is timing, not proof of cause;
 * without a saved log on both sides nothing is compared.
 */
export const ErrorsAroundOperation: React.FC<{
  profileId: string;
  at: string;
  /** Names of the mods the operation changed, to point them out. */
  changed: readonly string[];
}> = ({ profileId, at, changed }) => {
  const { data: sessions } = useLaunchSessions(20);
  const { before, after } = sessionsAround(sessions ?? [], profileId, at);
  const report = (id: string | undefined) => ({
    queryKey: ["session-log-report", id],
    queryFn: () => api.getDiagnosticsReport(undefined, id),
    enabled: Boolean(id),
    staleTime: 60_000,
  });
  const { data: earlier } = useQuery(report(before?.id));
  const { data: later } = useQuery(report(after?.id));

  if (!sessions) return null;
  const unable = (why: string) => (
    <p className="text-[var(--fg-muted)]">
      Errors before and after this change cannot be compared: {why}
    </p>
  );
  if (!before) return unable("there is no game session before it.");
  if (!after) return unable("the game has not been played since.");
  if (!earlier || !later) return null;
  if (!earlier.log_is_saved_copy || !later.log_is_saved_copy)
    return unable(
      "a saved SMAPI log is missing for the session before or after it.",
    );

  const diff = compareLogs(earlier, later);
  const lower = new Set(changed.map((name) => name.toLowerCase()));
  const mark = (name: string) =>
    lower.has(name.toLowerCase()) ? `${name} (changed here)` : name;
  const already = later.log_summary.sources
    .filter((s) => s.errors > 0)
    .map((s) => s.source)
    .filter((source) => !diff.newErrorSources.includes(source));
  const newOnes = [
    ...diff.newErrorSources.map((s) => `${mark(s)} logged errors`),
    ...diff.newlySkipped.map((s) => `${mark(s)} was skipped`),
  ];
  return (
    <div>
      <p className="font-semibold">
        Errors in the next session (
        {new Date(after.launched_at).toLocaleString()})
      </p>
      {newOnes.length === 0 ? (
        <p>No new errors or skipped mods compared with the session before.</p>
      ) : (
        <ul className="list-disc pl-4">
          {newOnes.map((line) => (
            <li key={line}>New since the session before: {line}</li>
          ))}
        </ul>
      )}
      {already.length > 0 && (
        <p className="text-[var(--fg-muted)]">
          Already logging errors before: {already.join(", ")}
        </p>
      )}
      <p className="text-[var(--fg-muted)]">
        This is what changed between the two sessions around this operation, not
        proof that it caused anything. To go back, use a restore point from
        before it on the Profiles page.
      </p>
    </div>
  );
};
