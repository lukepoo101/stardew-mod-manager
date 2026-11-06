import React from "react";
import { Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { OperationDetails } from "./OperationDetails";
import { EmptyState, LoadFailed, Loading } from "@/components/ui/EmptyState";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { OperationDto } from "@/shared/api/generated";
import { History, CheckCircle2, AlertTriangle, Clock } from "lucide-react";
import {
  followUps,
  markDone,
  markSeen,
  readSeenAt,
  snooze,
  unseen,
} from "@/shared/activity/seen";
import { usePreferences } from "@/shared/preferences";
import { recordedMessage } from "./recordedMessage";
import { askToConfirm } from "@/shared/platform/confirm";

const PAGE = 50;

/** The state in plain words. */
const STATE: Record<string, string> = {
  succeeded: "done",
  failed: "failed",
  cancelled: "not done",
  recovery_required: "needs recovery",
  draft: "review open",
  rolled_back: "undone",
};

/** The way out offered for an operation that is not finished. */
const UnfinishedActions: React.FC<{ op: OperationDto }> = ({ op }) => {
  const [status, setStatus] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  // Each of these commands announces its change, so the list refreshes
  // itself through the invalidation bridge.
  const run = async (action: () => Promise<void>, done: string) => {
    setBusy(true);
    setStatus(null);
    try {
      await action();
      setStatus(done);
    } catch (actionError) {
      setStatus(errorSummary(actionError, "That did not work"));
    } finally {
      setBusy(false);
    }
  };
  if (op.state === "draft") {
    return (
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span>
          A review that was never confirmed. Nothing has changed, and it does
          not stop other changes.
        </span>
        <Button
          size="sm"
          variant="secondary"
          isLoading={busy}
          onClick={() =>
            void run(
              () => api.cancelActiveOperation(op.id),
              "Discarded. Nothing was changed.",
            )
          }
        >
          Discard this review
        </Button>
        {status && <span role="status">{status}</span>}
      </div>
    );
  }
  if (op.state === "recovery_required") {
    return (
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <Button
          size="sm"
          variant="secondary"
          isLoading={busy}
          onClick={() =>
            void run(
              () => api.retryRecovery(),
              "Recovery ran; this entry shows how it ended.",
            )
          }
        >
          Retry recovery
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onClick={async () => {
            if (
              await askToConfirm(
                "Mark this as handled? Only do this if you have put the files right yourself. The manager will not check, nothing on disk changes, and the record stays here.",
              )
            )
              void run(
                () => api.markOperationHandled(op.id),
                "Marked as handled.",
              );
          }}
        >
          I fixed it myself
        </Button>
        {status && <span role="status">{status}</span>}
      </div>
    );
  }
  // Running, committing and the like. The backend says whether its owner
  // (this window, a batch it is driving, another process) is still carrying
  // it out; only one nobody is carrying out is offered to recovery, which
  // never touches a live one anyway.
  if (op.owner_alive) {
    return (
      <p className="text-xs">
        Being carried out right now. Changes to the same profile wait until it
        is finished.
      </p>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <span>
        Stopped part-way: whatever was doing it (this app or a command) is no
        longer running. Changes to the same profile wait until it is finished or
        undone.
      </span>
      <Button
        size="sm"
        variant="secondary"
        isLoading={busy}
        onClick={() =>
          void run(
            () => api.retryRecovery(),
            "Checked. This entry shows how it ended.",
          )
        }
      >
        Check and finish it
      </Button>
      {status && <span role="status">{status}</span>}
    </div>
  );
};

export const ActivityView: React.FC = () => {
  const [limit, setLimit] = React.useState(PAGE);
  const {
    data: operations,
    error,
    refetch,
  } = useQuery<OperationDto[]>({
    queryKey: ["operations", "history", limit],
    queryFn: () => api.listRecentOperations(limit),
    staleTime: 2000,
  });
  const [preferences] = usePreferences();
  const expert = preferences.expertMode;
  // What was new when the page opened stays marked as new while it is open;
  // opening it marks everything read for next time.
  const [seenAt] = React.useState(readSeenAt);
  const fresh = unseen(operations, seenAt);
  // Re-read after a follow-up is marked done or snoozed.
  const [followVersion, setFollowVersion] = React.useState(0);
  const toFollow = React.useMemo(
    () => new Set(followUps(operations).map((op) => op.id)),
    [operations, followVersion],
  );
  React.useEffect(() => {
    if (operations) markSeen();
  }, [operations]);

  // A review that was closed without being confirmed changed nothing; it is
  // only listed for troubleshooting.
  const shown = (operations ?? []).filter((op) => expert || !op.closed_preview);
  const hiddenPreviews = (operations?.length ?? 0) - shown.length;

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">Activity</h2>
        <p className="text-sm text-[var(--fg-muted)]">
          What the manager changed, newest first. Open an entry to see what
          changed; a removal can be undone from here.
        </p>
      </div>

      {error && !operations ? (
        <LoadFailed
          what="recent activity"
          message={errorSummary(error, "Unable to load recent operations")}
          onRetry={() => void refetch()}
        />
      ) : !operations ? (
        <Loading what="activity" />
      ) : operations.length > 0 ? (
        <>
          <Card className="p-0 divide-y divide-[var(--border)] border border-[var(--border)] overflow-hidden">
            {shown.map((op) => {
              const isSuccess = op.state === "succeeded";
              const rolledBack = op.rolled_back;
              const isFailed =
                !rolledBack &&
                (op.state === "failed" || op.state === "recovery_required");
              const isQuiet = op.state === "cancelled" || rolledBack;
              const unfinished = !isSuccess && !isFailed && !isQuiet;
              const message = recordedMessage(op.error_message);

              return (
                <div
                  key={op.id}
                  className="p-4 flex items-center justify-between gap-4"
                >
                  <div className="flex items-start gap-3 min-w-0">
                    <div className="p-2 rounded-lg bg-[var(--bg-elevated)] mt-0.5">
                      {isSuccess ? (
                        <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                      ) : isFailed ? (
                        <AlertTriangle className="w-4 h-4 text-[var(--danger)]" />
                      ) : (
                        <Clock className="w-4 h-4 text-[var(--accent-primary)]" />
                      )}
                    </div>
                    <div className="space-y-1 min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-bold text-sm text-[var(--fg-primary)]">
                          {op.title}
                        </span>
                        {fresh.ids.has(op.id) && (
                          <span className="text-[10px] uppercase tracking-wider px-1.5 rounded border border-[var(--border)]">
                            New
                          </span>
                        )}
                        {toFollow.has(op.id) && (
                          <span className="flex items-center gap-1 text-[10px]">
                            <span className="uppercase tracking-wider px-1.5 rounded border border-[var(--danger)] text-[var(--danger)]">
                              Follow up
                            </span>
                            <button
                              type="button"
                              className="underline cursor-pointer"
                              onClick={() => {
                                markDone(op.id);
                                setFollowVersion((v) => v + 1);
                              }}
                            >
                              Done with this
                            </button>
                            <button
                              type="button"
                              className="underline cursor-pointer"
                              onClick={() => {
                                snooze(
                                  op.id,
                                  new Date(Date.now() + 24 * 60 * 60 * 1000),
                                );
                                setFollowVersion((v) => v + 1);
                              }}
                            >
                              Remind me tomorrow
                            </button>
                          </span>
                        )}
                        <StatusBadge
                          variant={
                            isSuccess
                              ? "success"
                              : isFailed
                                ? "danger"
                                : rolledBack || isQuiet
                                  ? "warning"
                                  : "info"
                          }
                        >
                          {rolledBack
                            ? "rolled back"
                            : (STATE[op.state] ?? "in progress")}
                        </StatusBadge>
                      </div>
                      {expert && (
                        <p className="text-xs text-[var(--fg-muted)] font-mono truncate select-text">
                          {op.kind} · {op.state} · {op.id}
                          {op.error_code ? ` · ${op.error_code}` : ""}
                        </p>
                      )}
                      {op.part_of && (
                        <p className="text-xs text-[var(--fg-muted)]">
                          Part of: {op.part_of}
                        </p>
                      )}
                      {rolledBack && (
                        <p className="text-xs text-[var(--fg-muted)]">
                          It failed part-way, and the changes it had made were
                          undone. Nothing it started was left behind.
                        </p>
                      )}
                      {message &&
                        (isFailed ? (
                          <p className="text-xs text-[var(--danger)] mt-1">
                            Error: {message}
                          </p>
                        ) : (
                          <p className="text-xs text-[var(--fg-muted)] mt-1">
                            {message}
                          </p>
                        ))}
                      {(unfinished || op.state === "recovery_required") && (
                        <UnfinishedActions op={op} />
                      )}
                      <OperationDetails
                        operationId={op.id}
                        expert={expert}
                        around={
                          isSuccess && op.profile_id
                            ? { profileId: op.profile_id, at: op.created_at }
                            : null
                        }
                        undoRemovalInto={
                          op.kind === "mod_remove" && op.state === "succeeded"
                            ? op.profile_id
                            : null
                        }
                      />
                    </div>
                  </div>

                  <div className="text-right text-xs text-[var(--fg-muted)] shrink-0">
                    <div>{new Date(op.created_at).toLocaleTimeString()}</div>
                    <div>{new Date(op.created_at).toLocaleDateString()}</div>
                  </div>
                </div>
              );
            })}
          </Card>
          <div className="flex flex-wrap items-center gap-3 text-xs text-[var(--fg-muted)]">
            {hiddenPreviews > 0 && (
              <span>
                {hiddenPreviews} review(s) closed without changes are not shown
                (Expert mode shows them).
              </span>
            )}
            {operations.length >= limit && (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => setLimit((current) => current + PAGE)}
              >
                Show older
              </Button>
            )}
          </div>
        </>
      ) : (
        <EmptyState
          icon={<History className="w-6 h-6" />}
          title="Nothing has happened yet"
          description="Installing, removing and repairing are recorded here with their outcome, so you can see what changed and recover if something is interrupted."
          actions={
            <Link
              to="/app/mods"
              className="text-xs font-medium text-[var(--accent-primary)] hover:underline"
            >
              Install a mod
            </Link>
          }
        />
      )}
    </div>
  );
};
