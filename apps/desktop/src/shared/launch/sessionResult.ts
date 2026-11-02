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

/** The mods SMAPI skipped and the ones it left out, each named. */
function skippedPoints(session: LaunchSessionDto): string[] {
  const skipped = skippedOf(session);
  const unconfirmed = unconfirmedOf(session);
  const points = skipped
    .slice(0, 5)
    .map(
      (mod) =>
        `SMAPI skipped ${mod.name} because ${mod.reason.replace(/\.$/, "")}.`,
    );
  if (skipped.length > 5) {
    points.push(`SMAPI skipped ${skipped.length - 5} more.`);
  }
  if (unconfirmed.length > 0) {
    points.push(
      `Not confirmed loaded: ${names(unconfirmed)}. They may not be running.`,
    );
  }
  return points;
}

// Sessions from before these were recorded may not carry them.
const skippedOf = (session: LaunchSessionDto) => session.skipped_mods ?? [];
const unconfirmedOf = (session: LaunchSessionDto) =>
  session.unconfirmed_mods ?? [];

export interface RunningResult {
  tone: SessionTone | "info";
  text: string;
  points: string[];
}

/**
 * What the log shows so far about a session that is still running. The
 * colour follows the evidence, so a problem is never shown as a success.
 */
export function describeRunningSession(
  session: LaunchSessionDto,
): RunningResult | null {
  if (session.launch_mode === "vanilla") return null;
  const points = skippedPoints(session);
  switch (session.outcome) {
    case "confirmed":
      return {
        tone: "success",
        text:
          session.launch_mode === "runtime_test"
            ? "SMAPI started with an empty Mods folder."
            : `SMAPI's log confirms all ${session.verified_mods.length} mod(s) loaded.`,
        points,
      };
    case "partial":
      return {
        tone: "warning",
        text: `SMAPI confirmed ${session.verified_mods.length} of ${session.expected_mods.length} mod(s) loaded.`,
        points,
      };
    case "wrong_mods_folder":
      return {
        tone: "danger",
        text:
          session.verification_details ??
          "SMAPI is not using this profile's Mods folder.",
        points,
      };
    case "unavailable":
      return {
        tone: "neutral",
        text: `Verification unavailable: ${
          session.verification_details ??
          "SMAPI's log cannot show which mods loaded."
        }`,
        points,
      };
    default:
      return session.state === "verification_unavailable"
        ? {
            tone: "neutral",
            text: "Verification unavailable: SMAPI's log has not shown this session.",
            points,
          }
        : {
            tone: "info",
            text: "Waiting for SMAPI's log to show which mods loaded...",
            points,
          };
  }
}

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
  const confirmed = session.evidence === "mods_loaded";
  const outcome = session.outcome ?? null;

  if (session.state === "failed") {
    return outcome === "wrong_mods_folder"
      ? {
          tone: "danger",
          title: "SMAPI used a different Mods folder",
          detail:
            session.verification_details ??
            "SMAPI did not load mods from this profile's folder.",
          points: [],
        }
      : {
          tone: "danger",
          title: "The game did not start properly",
          detail: session.verification_details ?? "The launch failed.",
          points: [],
        };
  }
  if (session.launch_mode === "runtime_test") {
    // Only SMAPI was started, with an empty Mods folder.
    return confirmed
      ? {
          tone: "success",
          title: "SMAPI started on its own",
          detail:
            `${played} Only SMAPI itself was tested, with an empty Mods folder. This does not show that your mods work.`.trim(),
          points: [],
        }
      : {
          tone: "warning",
          title: "Could not confirm that SMAPI started",
          detail: `${played} ${
            session.verification_details ??
            "SMAPI's log did not show this test run, so it is unknown whether SMAPI itself starts."
          } Your mods were not involved.`.trim(),
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

  const points = skippedPoints(session);
  const log =
    report && report.session_id === session.id ? report.log_summary : null;
  if (log) {
    // Sessions recorded before skipped mods were kept with the session.
    if (skippedOf(session).length === 0 && log.skipped_mods.length > 0) {
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
  if (outcome === "partial") {
    const skipped = skippedOf(session).length;
    return {
      tone: "warning",
      title:
        skipped > 0
          ? `SMAPI skipped ${skipped} mod(s)`
          : `${unconfirmedOf(session).length} mod(s) did not load`,
      detail:
        `${played} The game started, and SMAPI confirmed ${session.verified_mods.length} of ${session.expected_mods.length} mod(s) loaded.`.trim(),
      points,
    };
  }
  if (
    outcome === "unavailable" ||
    session.state === "verification_unavailable"
  ) {
    return {
      tone: "neutral",
      title: "Verification unavailable",
      detail: `${played} ${
        session.verification_details ??
        "SMAPI's log could not show which mods loaded."
      } This is neither a failure nor a confirmation.`.trim(),
      points,
    };
  }
  if (outcome === "confirmed" && session.expected_mods.length === 0) {
    return {
      tone: "success",
      title: "SMAPI started",
      detail:
        `${played} No mods were enabled in this profile, so none were expected.`.trim(),
      points,
    };
  }
  if (confirmed && session.verified_mods.length > 0) {
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
