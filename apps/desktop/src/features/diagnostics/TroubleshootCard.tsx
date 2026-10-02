import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { TroubleshootDto } from "@/shared/api/generated";
import { useActiveProfileOverview, useExperiments } from "@/shared/api/hooks";
import { Search } from "lucide-react";

const KEY = ["troubleshoot"] as const;

/**
 * Finds which mod causes a problem by turning mods off and on in halves.
 * Normally this runs on a copy of the profile, so the original is never
 * touched and the copy is discarded or kept at the end. Running it on the
 * profile itself records the original setup first and restores it exactly.
 */
export const TroubleshootCard: React.FC = () => {
  const queryClient = useQueryClient();
  const { data: state } = useQuery<TroubleshootDto>({
    queryKey: KEY,
    queryFn: () => api.getTroubleshootStatus(),
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { data: overview } = useActiveProfileOverview();
  const { data: experiments } = useExperiments();
  const active = overview?.profile;
  const copyOf = experiments?.find((e) => e.profile_id === active?.id);

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

  /** Copies the profile, switches to the copy and starts there. */
  const startOnCopy = () =>
    run(async () => {
      if (!active) throw new Error("No active profile");
      const copy = await api.startExperiment(
        active.id,
        `${active.name} troubleshooting`,
      );
      await api.activateProfile(copy.profile_id);
      return api.startTroubleshoot();
    });

  /** Switches back to the original and deletes the copy with its tests. */
  const discardCopy = () => {
    if (!copyOf) return;
    if (
      !window.confirm(
        `Delete "${active?.name}" and go back to "${copyOf.source_name}"? "${copyOf.source_name}" was never changed by the tests.`,
      )
    )
      return;
    void run(async () => {
      await api.restoreTroubleshoot();
      await api.activateProfile(copyOf.source_profile_id);
      await api.archiveProfile(copyOf.profile_id);
      await api.deleteProfile(copyOf.profile_id);
      await api.keepExperiment(copyOf.profile_id);
      return api.getTroubleshootStatus();
    });
  };

  /** Ends the tests with every mod back on and keeps the copy. */
  const keepCopy = () =>
    run(async () => {
      const done = await api.restoreTroubleshoot();
      if (copyOf) await api.keepExperiment(copyOf.profile_id);
      return done;
    });

  const restore = copyOf ? (
    <div className="flex flex-wrap gap-2">
      <Button
        size="sm"
        variant="secondary"
        disabled={busy}
        onClick={discardCopy}
      >
        Finish: delete the copy and go back to "{copyOf.source_name}"
      </Button>
      <Button size="sm" variant="ghost" disabled={busy} onClick={keepCopy}>
        Keep the copy as a profile
      </Button>
    </div>
  ) : (
    <Button
      size="sm"
      variant="secondary"
      disabled={busy}
      onClick={() => run(() => api.restoreTroubleshoot())}
    >
      Restore my original mods
    </Button>
  );

  const history = state && state.history.length > 0 && (
    <details className="text-xs">
      <summary className="cursor-pointer text-[var(--fg-muted)]">
        Steps so far ({state.history.length})
      </summary>
      <ol className="list-decimal pl-4">
        {state.history.map((step) => (
          <li key={step.step}>
            {step.mods_on} mod(s) on:{" "}
            {step.problem_present ? "problem happened" : "no problem"}
            {step.session_id
              ? ` (game session at ${new Date(step.answered_at).toLocaleTimeString()}, ${step.session_state?.replace(/_/g, " ")})`
              : " (no game session recorded for this step)"}
          </li>
        ))}
      </ol>
    </details>
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
            kept together. By default it works on a copy of "
            {active?.name ?? "this profile"}", so the original is never changed;
            at the end you delete the copy or keep it. Close the game before you
            start.
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="primary"
              disabled={busy || !active}
              onClick={startOnCopy}
            >
              Start on a copy
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              onClick={() => run(() => api.startTroubleshoot())}
              title="Changes this profile's mods during the tests; they are restored exactly at the end."
            >
              Troubleshoot this profile directly
            </Button>
          </div>
        </>
      )}

      {state?.active && state.phase === "all_off" && (
        <div className="space-y-2 text-xs">
          <p>
            <strong>Step 1.</strong> Every mod is now off. Launch the game and
            check whether the problem still happens.
          </p>
          {question}
          {history}
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
          {state.together.length > 0 && (
            <div>
              <p className="font-semibold">Why some mods are on together</p>
              <ul className="list-disc pl-4">
                {state.together.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </div>
          )}
          {question}
          {history}
          {restore}
        </div>
      )}

      {state?.active && state.phase === "found" && (
        <div className="space-y-2 text-xs" role="status">
          <p>
            Most likely cause: <strong>{state.culprit}</strong>
          </p>
          {state.note && <p className="text-[var(--fg-muted)]">{state.note}</p>}
          <p className="text-[var(--fg-muted)]">
            This is what the answers point to, not proof. Check it by playing
            with only that mod turned off.
          </p>
          {history}
          {restore}
        </div>
      )}

      {state?.active && state.phase === "inconclusive" && (
        <div className="space-y-2 text-xs" role="status">
          <p>No single mod could be blamed.</p>
          {state.note && <p className="text-[var(--fg-muted)]">{state.note}</p>}
          {history}
          {restore}
        </div>
      )}
    </Card>
  );
};
