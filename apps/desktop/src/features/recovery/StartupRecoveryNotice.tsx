import React from "react";
import { Link } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";

const OUTCOME: Record<string, string> = {
  finished: "Finished",
  undone: "Not done",
  needs_attention: "Needs your attention",
};

const QUERY_KEY = ["startup-recovery-report"];

/**
 * Tells the user what happened to changes the app was making when it last
 * stopped (a crash, a forced quit), once, at the top of every page until
 * dismissed. Without it, someone who clicked "Install mod" just before a
 * crash could not tell whether the install happened.
 */
export const StartupRecoveryNotice: React.FC = () => {
  const client = useQueryClient();
  const { data } = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => api.getStartupRecoveryReport(),
    staleTime: Number.POSITIVE_INFINITY,
  });
  if (!data || data.length === 0) return null;
  const attention = data.some((item) => item.outcome === "needs_attention");
  return (
    <section
      role="status"
      aria-label="What happened to interrupted changes"
      className={`mx-6 mt-3 p-3 rounded-lg border text-xs space-y-2 ${
        attention ? "border-[var(--danger)]" : "border-[var(--warning)]"
      }`}
    >
      <p className="font-semibold">
        Stardew Mod Manager stopped while it was changing your mods. This is
        what happened to each change:
      </p>
      <ul className="space-y-1">
        {data.map((item) => (
          <li key={item.operation_id}>
            <span className="font-semibold">
              {OUTCOME[item.outcome] ?? item.outcome}: {item.title}.
            </span>{" "}
            {item.message}
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap items-center gap-3">
        <Link
          to="/app/activity"
          className="text-[var(--accent-primary)] hover:underline"
        >
          Open Activity
        </Link>
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            await api.dismissStartupRecoveryReport();
            client.setQueryData(QUERY_KEY, []);
          }}
        >
          Dismiss
        </Button>
      </div>
    </section>
  );
};
