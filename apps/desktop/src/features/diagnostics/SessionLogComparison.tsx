import React from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/api/client";
import type { LaunchSessionDto } from "@/shared/api/generated";
import { compareLogs } from "@/shared/launch/compareSessions";

/**
 * Compares what two sessions' saved SMAPI logs say. A session without a
 * saved log is named rather than compared against SMAPI's current file.
 */
export const SessionLogComparison: React.FC<{
  earlier: LaunchSessionDto;
  later: LaunchSessionDto;
}> = ({ earlier, later }) => {
  const report = (session: LaunchSessionDto) => ({
    queryKey: ["session-log-report", session.id],
    queryFn: () => api.getDiagnosticsReport(undefined, session.id),
    staleTime: 60_000,
  });
  const { data: before } = useQuery(report(earlier));
  const { data: after } = useQuery(report(later));
  if (!before || !after) return null;
  const missing = [
    !before.log_is_saved_copy ? "the earlier" : null,
    !after.log_is_saved_copy ? "the later" : null,
  ].filter(Boolean);
  if (missing.length > 0) {
    return (
      <p className="text-[var(--fg-muted)]">
        There is no saved SMAPI log for {missing.join(" or ")} session, so their
        messages are not compared. Logs are saved when a session ends, for the
        last 20 sessions.
      </p>
    );
  }
  const diff = compareLogs(before, after);
  const lines = [
    diff.newlySkipped.length > 0 &&
      `Skipped later but not earlier: ${diff.newlySkipped.join(", ")}`,
    diff.noLongerSkipped.length > 0 &&
      `Skipped earlier but not later: ${diff.noLongerSkipped.join(", ")}`,
    diff.newErrorSources.length > 0 &&
      `Logged errors later but not earlier: ${diff.newErrorSources.join(", ")}`,
    diff.goneErrorSources.length > 0 &&
      `Logged errors earlier but not later: ${diff.goneErrorSources.join(", ")}`,
  ].filter((line): line is string => Boolean(line));
  return (
    <div>
      <p className="font-semibold">What the SMAPI logs say differently</p>
      {lines.length === 0 ? (
        <p>The same mods were skipped and the same sources logged errors.</p>
      ) : (
        <ul className="list-disc pl-4">
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}
    </div>
  );
};
