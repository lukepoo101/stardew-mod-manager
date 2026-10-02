import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useLaunchSessions } from "@/shared/api/hooks";
import { describeSession } from "@/shared/launch/sessionResult";
import {
  compareSessions,
  describeVersion,
} from "@/shared/launch/compareSessions";
import type { LaunchSessionDto } from "@/shared/api/generated";
import { History } from "lucide-react";

const MODE: Record<string, string> = {
  modded: "Modded",
  vanilla: "Vanilla",
  runtime_test: "Test run",
};

function runtime(session: LaunchSessionDto): string {
  const game = `Stardew Valley ${session.game_version ?? "(unknown)"}`;
  return session.launch_mode === "vanilla"
    ? game
    : `${game}, SMAPI ${session.smapi_version ?? "(unknown)"}`;
}

/**
 * Every recorded game session of the active profile, newest first, with what
 * the manager saw. Each one is a record; nothing here changes the profile.
 */
export const SessionHistoryCard: React.FC = () => {
  const { data: sessions } = useLaunchSessions(20);
  const [picked, setPicked] = useState<string[]>([]);
  const toggle = (id: string) =>
    setPicked((current) =>
      current.includes(id)
        ? current.filter((x) => x !== id)
        : [...current, id].slice(-2),
    );
  const pair = (sessions ?? []).filter((s) => picked.includes(s.id));
  const comparison =
    pair.length === 2 ? compareSessions(pair[0], pair[1]) : null;

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <History className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Session history</h3>
      </div>
      {!sessions || sessions.length === 0 ? (
        <p className="text-xs text-[var(--fg-muted)]">
          No game sessions have been recorded for this profile yet.
        </p>
      ) : (
        <ul className="text-xs divide-y divide-[var(--border)]">
          {sessions.map((session) => {
            const result = describeSession(session);
            return (
              <li key={session.id} className="py-2 space-y-1">
                <div className="flex flex-wrap items-center gap-2">
                  <input
                    type="checkbox"
                    checked={picked.includes(session.id)}
                    onChange={() => toggle(session.id)}
                    aria-label={`Compare the session of ${new Date(session.launched_at).toLocaleString()}`}
                  />
                  <span className="font-medium">
                    {new Date(session.launched_at).toLocaleString()}
                  </span>
                  <span className="text-[var(--fg-muted)]">
                    {MODE[session.launch_mode] ?? session.launch_mode}
                  </span>
                  {result ? (
                    <StatusBadge
                      variant={
                        result.tone === "neutral" ? "neutral" : result.tone
                      }
                    >
                      {result.title}
                    </StatusBadge>
                  ) : (
                    <StatusBadge variant="info">Running</StatusBadge>
                  )}
                </div>
                <p className="text-[var(--fg-muted)]">
                  Started on {runtime(session)}
                  {session.launch_mode === "vanilla"
                    ? ""
                    : ` with ${session.verified_mods.length} mod(s) confirmed loaded`}
                  .
                </p>
                {session.acknowledged_warnings.length > 0 && (
                  <p className="text-[var(--warning)]">
                    Started past: {session.acknowledged_warnings.join("; ")}
                  </p>
                )}
                {session.verification_details && (
                  <details>
                    <summary className="cursor-pointer text-[var(--fg-muted)]">
                      What the manager saw
                    </summary>
                    <p className="mt-1 whitespace-pre-line break-words">
                      {session.verification_details}
                    </p>
                  </details>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {sessions && sessions.length > 1 && !comparison && (
        <p className="text-xs text-[var(--fg-muted)]">
          Tick two sessions to compare them.
        </p>
      )}
      {comparison && (
        <div className="text-xs space-y-1 border-t border-[var(--border)] pt-2">
          <p className="font-semibold">
            {new Date(comparison.earlier.launched_at).toLocaleString()} compared
            with {new Date(comparison.later.launched_at).toLocaleString()}
          </p>
          <p>
            Result: {describeSession(comparison.earlier)?.title ?? "running"} →{" "}
            {describeSession(comparison.later)?.title ?? "running"}
          </p>
          <p>Stardew Valley: {describeVersion(comparison.game)}</p>
          <p>SMAPI: {describeVersion(comparison.smapi)}</p>
          {comparison.added.length + comparison.removed.length === 0 ? (
            <p>The same mods were enabled at both starts.</p>
          ) : (
            <>
              {comparison.added.length > 0 && (
                <p>
                  Enabled only in the later one: {comparison.added.join(", ")}
                </p>
              )}
              {comparison.removed.length > 0 && (
                <p>
                  Enabled only in the earlier one:{" "}
                  {comparison.removed.join(", ")}
                </p>
              )}
            </>
          )}
          {comparison.later.acknowledged_warnings.length > 0 && (
            <p>
              The later one started past:{" "}
              {comparison.later.acknowledged_warnings.join("; ")}
            </p>
          )}
          <p className="text-[var(--fg-muted)]">
            These are differences between the two starts, not proof of what
            changed a result. SMAPI logs of earlier sessions are not kept, so
            their messages cannot be compared here.
          </p>
        </div>
      )}
    </Card>
  );
};
