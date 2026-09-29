import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { TroubleshootDto } from "@/shared/api/generated";
import { Search } from "lucide-react";

const KEY = ["troubleshoot"] as const;

/**
 * Finds which mod causes a problem by turning mods off and on in halves. The
 * user's original setup is recorded first and can be restored at any time.
 */
export const TroubleshootCard: React.FC = () => {
  const queryClient = useQueryClient();
  const { data: state } = useQuery<TroubleshootDto>({
    queryKey: KEY,
    queryFn: () => api.getTroubleshootStatus(),
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<TroubleshootDto>) => {
    setBusy(true);
    setError(null);
    try {
      queryClient.setQueryData(KEY, await action());
    } catch (failure) {
      setError(errorSummary(failure, "That step could not be applied"));
    } finally {
      setBusy(false);
    }
  };

  const restore = (
    <Button
      size="sm"
      variant="secondary"
      disabled={busy}
      onClick={() => run(() => api.restoreTroubleshoot())}
    >
      Restore my original mods
    </Button>
  );

  const question = (
    <div className="flex flex-wrap gap-2">
      <Button
        size="sm"
        variant="primary"
        disabled={busy}
        onClick={() => run(() => api.answerTroubleshoot(true))}
      >
        The problem still happens
      </Button>
      <Button
        size="sm"
        variant="secondary"
        disabled={busy}
        onClick={() => run(() => api.answerTroubleshoot(false))}
      >
        The problem is gone
      </Button>
    </div>
  );

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Search className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Find the mod causing a problem</h3>
      </div>

      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}

      {!state?.active && (
        <>
          <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
            This turns your mods off, then back on in halves, and asks after
            each step whether the problem happens. Mods that need each other are
            kept together. Your current setup is recorded first, and you can
            restore it at any point. Close the game before you start.
          </p>
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => run(() => api.startTroubleshoot())}
          >
            Start troubleshooting
          </Button>
        </>
      )}

      {state?.active && state.phase === "all_off" && (
        <div className="space-y-2 text-xs">
          <p>
            <strong>Step 1.</strong> Every mod is now off. Launch the game and
            check whether the problem still happens.
          </p>
          {question}
          {restore}
        </div>
      )}

      {state?.active && state.phase === "testing" && (
        <div className="space-y-2 text-xs">
          <p>
            <strong>Step {state.step + 1}.</strong> {state.enabled_mods.length}{" "}
            mod(s) are on, {state.suspects.length} still under suspicion. Launch
            the game and check again.
          </p>
          <details>
            <summary className="cursor-pointer text-[var(--fg-muted)]">
              Mods that are on for this test
            </summary>
            <ul className="list-disc pl-4">
              {state.enabled_mods.map((name) => (
                <li key={name}>{name}</li>
              ))}
            </ul>
          </details>
          {question}
          {restore}
        </div>
      )}

      {state?.active && state.phase === "found" && (
        <div className="space-y-2 text-xs" role="status">
          <p>
            Most likely cause: <strong>{state.culprit}</strong>
          </p>
          {state.note && <p className="text-[var(--fg-muted)]">{state.note}</p>}
          {restore}
        </div>
      )}

      {state?.active && state.phase === "inconclusive" && (
        <div className="space-y-2 text-xs" role="status">
          <p>No single mod could be blamed.</p>
          {state.note && <p className="text-[var(--fg-muted)]">{state.note}</p>}
          {restore}
        </div>
      )}
    </Card>
  );
};
