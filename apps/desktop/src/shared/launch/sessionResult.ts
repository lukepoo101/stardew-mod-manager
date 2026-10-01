import type { DiagnosticsDto, LaunchSessionDto } from "@/shared/api/generated";

export type SessionTone = "success" | "warning" | "danger" | "neutral";

export interface SessionResult {
  tone: SessionTone;
  title: string;
  detail: string;
  /** Notable things from this session's SMAPI log, never the whole log. */
  points: string[];
}

/** States the backend reports while the game may still be running. */
const RUNNING = new Set([
  "starting",
  "running_unverified",
  "mod_load_confirmed",
  "verification_unavailable",
]);

/** Whether a session in this state may still have the game running. */
export function isRunningState(state: string): boolean {
  return RUNNING.has(state);
}

export function isFinished(session: LaunchSessionDto): boolean {
  return (
    session.state === "failed" ||
    session.ended_at !== null ||
    !RUNNING.has(session.state)
  );
}

export function formatDuration(from: string, to: string | null): string | null {
  if (!to) return null;
  const ms = Date.parse(to) - Date.parse(from);
  if (!Number.isFinite(ms) || ms < 0) return null;
  const minutes = Math.round(ms / 60000);
  if (minutes < 1) return "under a minute";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ${minutes % 60} min`;
}

const names = (list: string[]) =>
  list.length > 3
    ? `${list.slice(0, 3).join(", ")} and ${list.length - 3} more`
    : list.join(", ");

/**
 * What a finished session showed. Log evidence is only used when the report
 * belongs to this session, and nothing here claims more than the evidence:
 * a process that exited is not proof that the session went well.
 */
export function describeSession(
  session: LaunchSessionDto,
  report?: DiagnosticsDto | null,
): SessionResult | null {
  if (!isFinished(session)) return null;
  const duration = formatDuration(session.launched_at, session.ended_at);
  const played = duration ? `The game ran for ${duration}.` : "";

  if (session.state === "failed") {
    return {
      tone: "danger",
      title: "The game did not start properly",
      detail: session.verification_details ?? "The launch failed.",
      points: [],
    };
  }
  if (session.launch_mode === "vanilla") {
    return {
      tone: "neutral",
      title: "Played without mods",
      detail:
        `${played} Mods were not loaded, so there is nothing to check.`.trim(),
      points: [],
    };
  }

  const points: string[] = [];
  const log =
    report && report.session_id === session.id ? report.log_summary : null;
  if (log) {
    if (log.skipped_mods.length > 0) {
      points.push(
        `SMAPI skipped ${log.skipped_mods.length} mod(s): ${names(
          log.skipped_mods.map((m) => m.name),
        )}.`,
      );
    }
    if (log.update_notices.length > 0) {
      points.push(`${log.update_notices.length} mod(s) reported an update.`);
    }
  }

  if (!session.ended_at) {
    return {
      tone: "warning",
      title: "The manager did not see the game close",
      detail:
        "It may have been closed while the manager was not running, so how the session ended is unknown.",
      points,
    };
  }
  if (session.verified_mods.length > 0) {
    return {
      tone: points.some((p) => p.startsWith("SMAPI skipped"))
        ? "warning"
        : "success",
      title: `SMAPI loaded ${session.verified_mods.length} mod(s)`,
      detail:
        `${played} This shows the mods loaded, not that everything worked in game.`.trim(),
      points,
    };
  }
  return {
    tone: "warning",
    title: "Could not confirm that mods loaded",
    detail: `${played} ${
      session.verification_details ??
      "The SMAPI log did not show which mods loaded."
    }`.trim(),
    points,
  };
}
