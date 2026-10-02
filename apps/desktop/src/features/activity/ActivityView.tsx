import React from "react";
import { Link } from "react-router-dom";
import { Card } from "@/components/ui/Card";
import { OperationDetails } from "./OperationDetails";
import { EmptyState, LoadFailed, Loading } from "@/components/ui/EmptyState";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useRecentOperations } from "@/shared/api/hooks";
import { errorSummary } from "@/shared/api/errors";
import { History, CheckCircle2, AlertTriangle, Clock } from "lucide-react";
import { markSeen, readSeenAt, unseen } from "@/shared/activity/seen";

export const ActivityView: React.FC = () => {
  const { data: operations, error, refetch } = useRecentOperations(50);
  // What was new when the page opened stays marked as new while it is open;
  // opening it marks everything read for next time.
  const [seenAt] = React.useState(readSeenAt);
  const fresh = unseen(operations, seenAt);
  React.useEffect(() => {
    if (operations) markSeen();
  }, [operations]);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">
          Activity & Operation Log
        </h2>
        <p className="text-sm text-[var(--fg-muted)]">
          Audit trail of durable operations, atomic filesystem deployments, and
          recovery events.
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
        <Card className="p-0 divide-y divide-[var(--border)] border border-[var(--border)] overflow-hidden">
          {operations.map((op) => {
            const isSuccess = op.state === "succeeded";
            const rolledBack = op.rolled_back;
            const isFailed =
              !rolledBack &&
              (op.state === "failed" || op.state === "recovery_required");

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
                    <div className="flex items-center gap-2">
                      <span className="font-bold text-sm text-[var(--fg-primary)]">
                        {op.kind}
                      </span>
                      {fresh.ids.has(op.id) && (
                        <span className="text-[10px] uppercase tracking-wider px-1.5 rounded border border-[var(--border)]">
                          New
                        </span>
                      )}
                      <StatusBadge
                        variant={
                          isSuccess
                            ? "success"
                            : isFailed
                              ? "danger"
                              : rolledBack
                                ? "warning"
                                : "info"
                        }
                      >
                        {rolledBack ? "rolled back" : op.state}
                      </StatusBadge>
                    </div>
                    <p className="text-xs text-[var(--fg-muted)] font-mono truncate">
                      ID: {op.id}
                    </p>
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
                    {op.error_message && (
                      <p className="text-xs text-[var(--danger)] mt-1">
                        Error: {op.error_message}
                      </p>
                    )}
                    <OperationDetails
                      operationId={op.id}
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
                  <div className="text-[10px]">
                    {new Date(op.created_at).toLocaleDateString()}
                  </div>
                </div>
              </div>
            );
          })}
        </Card>
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
