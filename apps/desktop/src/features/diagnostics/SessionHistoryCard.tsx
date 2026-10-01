import React from "react";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useLaunchSessions } from "@/shared/api/hooks";
import { describeSession } from "@/shared/launch/sessionResult";
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
    </Card>
  );
};
