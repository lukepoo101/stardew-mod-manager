import React, { useState } from "react";
import { Link } from "react-router-dom";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useActiveProfileOverview,
  useDiagnosticsReport,
  useLatestLaunchSession,
} from "@/shared/api/hooks";
import { describeSession } from "@/shared/launch/sessionResult";
import { History, X } from "lucide-react";

const DISMISSED_KEY = "smm-dismissed-session";

function readDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

/**
 * How the last game session ended, shown once the game has closed. Hiding it
 * only hides this card; the session stays in the history and diagnostics.
 */
export const LastSessionCard: React.FC = () => {
  const { data: session } = useLatestLaunchSession();
  const { data: overview } = useActiveProfileOverview();
  const { data: report } = useDiagnosticsReport(overview?.game.id);
  const [dismissed, setDismissed] = useState(readDismissed);

  if (!session || session.id === dismissed) return null;
  const result = describeSession(session, report);
  if (!result) return null;

  const hide = () => {
    try {
      localStorage.setItem(DISMISSED_KEY, session.id);
    } catch {
      // Without storage the card simply returns next time.
    }
    setDismissed(session.id);
  };

  return (
    <Card className="space-y-2" aria-labelledby="last-session-title">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2">
          <History className="w-4 h-4 text-[var(--accent-primary)]" />
          <h3 id="last-session-title" className="font-bold text-sm">
            Last session
          </h3>
          <StatusBadge
            variant={result.tone === "neutral" ? "neutral" : result.tone}
          >
            {result.title}
          </StatusBadge>
        </div>
        <button
          type="button"
          onClick={hide}
          aria-label="Hide last session result"
          className="p-1 rounded text-[var(--fg-muted)] hover:text-[var(--fg-primary)] cursor-pointer"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>
      <p className="text-xs text-[var(--fg-muted)]">{result.detail}</p>
      <p className="text-xs text-[var(--fg-muted)]">
        Started on Stardew Valley {session.game_version ?? "(version unknown)"}
        {session.launch_mode === "vanilla"
          ? ""
          : `, SMAPI ${session.smapi_version ?? "(version unknown)"}`}
        .
      </p>
      {result.points.length > 0 && (
        <ul className="text-xs list-disc pl-4 space-y-0.5">
          {result.points.map((point) => (
            <li key={point}>{point}</li>
          ))}
        </ul>
      )}
      <Link
        to="/app/diagnostics"
        className="text-xs text-[var(--accent-primary)] hover:underline font-medium"
      >
        Open diagnostics →
      </Link>
    </Card>
  );
};
