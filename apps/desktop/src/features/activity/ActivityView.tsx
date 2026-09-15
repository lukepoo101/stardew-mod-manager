import React from "react";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useRecentOperations } from "@/shared/api/hooks";
import { History, CheckCircle2, AlertTriangle, Clock } from "lucide-react";

export const ActivityView: React.FC = () => {
  const { data: operations, isLoading, error } = useRecentOperations(50);

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">Activity & Operation Log</h2>
        <p className="text-sm text-[var(--fg-muted)]">
          Audit trail of durable operations, atomic filesystem deployments, and recovery events.
        </p>
      </div>

      {error && <p role="alert">{error.message}</p>}
      {isLoading ? (
        <Card className="text-center py-12">
          <div className="w-6 h-6 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin mx-auto mb-2" />
          <p className="text-xs text-[var(--fg-muted)]">Loading operations...</p>
        </Card>
      ) : operations && operations.length > 0 ? (
        <Card className="p-0 divide-y divide-[var(--border)] border border-[var(--border)] overflow-hidden">
          {operations.map((op) => {
            const isSuccess = op.state === "succeeded";
            const isFailed = op.state === "failed" || op.state === "recovery_required";

            return (
              <div key={op.id} className="p-4 flex items-center justify-between gap-4">
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
                      <StatusBadge
                        variant={isSuccess ? "success" : isFailed ? "danger" : "info"}
                      >
                        {op.state}
                      </StatusBadge>
                    </div>
                    <p className="text-xs text-[var(--fg-muted)] font-mono truncate">
                      ID: {op.id}
                    </p>
                    {op.error_message && (
                      <p className="text-xs text-[var(--danger)] mt-1">
                        Error: {op.error_message}
                      </p>
                    )}
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
        <Card className="text-center py-12 space-y-3">
          <div className="w-12 h-12 rounded-full bg-[var(--bg-elevated)] text-[var(--fg-muted)] flex items-center justify-center mx-auto">
            <History className="w-6 h-6" />
          </div>
          <p className="text-sm font-semibold text-[var(--fg-primary)]">
            No operations recorded yet
          </p>
          <p className="text-xs text-[var(--fg-muted)]">
            Operations like installing or removing mods will be logged here with durable progress.
          </p>
        </Card>
      )}
    </div>
  );
};
