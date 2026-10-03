import React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import type { UnfinishedChangeDto } from "@/shared/api/generated";

/**
 * Changes of several steps the app stopped in the middle of, of one kind.
 * Each says what was done and what was not; the parts done were each
 * journaled on their own. The actions are the caller's: finishing works the
 * change out again from the profile as it is now.
 */
export const UnfinishedChanges: React.FC<{
  profileId: string;
  kind: "restore_point" | "reference";
  actions: (change: UnfinishedChangeDto) => React.ReactNode;
}> = ({ profileId, kind, actions }) => {
  const queryKey = ["unfinished-changes", profileId];
  const client = useQueryClient();
  const { data } = useQuery({
    queryKey,
    queryFn: () => api.listUnfinishedChanges(profileId),
  });
  const changes = (data ?? []).filter((c) => c.resume_kind === kind);
  if (changes.length === 0) return null;
  return (
    <div role="alert" className="text-xs space-y-2">
      {changes.map((change) => (
        <div
          key={change.operation_id}
          className="p-2 rounded border border-[var(--warning)] space-y-1"
        >
          <p className="font-semibold">
            {change.title} was interrupted (
            {new Date(change.started_at).toLocaleString()}).
          </p>
          {change.parts_done.length > 0 && (
            <p>Done: {change.parts_done.join("; ")}.</p>
          )}
          <p>Not done: {change.parts_left.join("; ") || "nothing listed"}.</p>
          <div className="flex flex-wrap gap-2">
            {actions(change)}
            <Button
              size="sm"
              variant="ghost"
              onClick={async () => {
                await api.putAsideUnfinishedChange(change.operation_id);
                await client.refetchQueries({ queryKey });
              }}
            >
              Put aside
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
};
