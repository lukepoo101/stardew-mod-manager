import React, { useMemo, useRef, useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useProfileMods,
  useReferenceRecipe,
} from "@/shared/api/hooks";
import {
  compareWithRecipe,
  differenceKey,
  parseRecipe,
} from "@/shared/recipe/recipe";
import { Users } from "lucide-react";

const MAX_BYTES = 2 * 1024 * 1024;

/**
 * Keeps a shared recipe, such as a multiplayer group's setup, as this
 * profile's reference and shows how the profile differs from it. Differences
 * can be accepted for the group; an accepted difference returns if either
 * side's version changes. It never changes any mods.
 */
export const ReferenceCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: reference } = useReferenceRecipe(profileId);
  const { data: mods } = useProfileMods(profileId);
  const input = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);

  const parsed = useMemo(
    () => (reference ? parseRecipe(reference.recipe_json) : null),
    [reference],
  );
  const comparison =
    parsed?.ok && mods ? compareWithRecipe(mods, parsed.recipe) : null;
  const accepted = new Set(reference?.accepted ?? []);
  const open =
    comparison?.differences.filter((d) => !accepted.has(differenceKey(d))) ??
    [];
  const acceptedList =
    comparison?.differences.filter((d) => accepted.has(differenceKey(d))) ?? [];

  if (!profileId) return null;

  const run = async (action: () => Promise<unknown>) => {
    setError(null);
    try {
      await action();
    } catch (actionError) {
      setError(errorSummary(actionError, "That did not work"));
    }
  };

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Users className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Group reference</h3>
        {comparison && (
          <StatusBadge variant={open.length === 0 ? "success" : "warning"}>
            {open.length === 0 ? "In step" : `${open.length} difference(s)`}
          </StatusBadge>
        )}
      </div>
      {!reference ? (
        <div className="text-xs space-y-2">
          <p className="text-[var(--fg-muted)]">
            Keep a recipe someone shared, for example your multiplayer group's
            setup, as this profile's reference. The profile is then compared
            with it whenever you open this page. Nothing is installed.
          </p>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => input.current?.click()}
          >
            Choose the group's recipe...
          </Button>
        </div>
      ) : (
        <div className="text-xs space-y-2">
          <p className="text-[var(--fg-muted)]">
            {parsed?.ok ? `"${parsed.recipe.profile_name}"` : "A recipe"}, kept{" "}
            {new Date(reference.attached_at).toLocaleString()}.
          </p>
          {comparison && open.length === 0 && (
            <p>
              This profile matches the reference
              {acceptedList.length > 0
                ? `, apart from ${acceptedList.length} accepted difference(s)`
                : ""}
              .
            </p>
          )}
          {open.length > 0 && (
            <ul className="space-y-1">
              {open.map((d) => (
                <li
                  key={differenceKey(d)}
                  className="flex items-start justify-between gap-2"
                >
                  <span>
                    <span className="font-mono">{d.unique_id}</span>: {d.detail}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      run(() =>
                        api.setReferenceDifferenceAccepted(
                          profileId,
                          differenceKey(d),
                          true,
                        ),
                      )
                    }
                  >
                    Accept
                  </Button>
                </li>
              ))}
            </ul>
          )}
          {acceptedList.length > 0 && (
            <details>
              <summary className="cursor-pointer">
                Accepted for this group ({acceptedList.length})
              </summary>
              <ul className="mt-1 space-y-1">
                {acceptedList.map((d) => (
                  <li
                    key={differenceKey(d)}
                    className="flex items-start justify-between gap-2"
                  >
                    <span>
                      <span className="font-mono">{d.unique_id}</span>:{" "}
                      {d.detail}
                    </span>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        run(() =>
                          api.setReferenceDifferenceAccepted(
                            profileId,
                            differenceKey(d),
                            false,
                          ),
                        )
                      }
                    >
                      Show again
                    </Button>
                  </li>
                ))}
              </ul>
            </details>
          )}
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="secondary"
              onClick={() => input.current?.click()}
            >
              Replace reference...
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => run(() => api.detachReferenceRecipe(profileId))}
            >
              Stop comparing
            </Button>
          </div>
        </div>
      )}
      <input
        ref={input}
        type="file"
        accept=".json,application/json"
        className="sr-only"
        aria-label="Group recipe file"
        onChange={async (event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (!file) return;
          if (file.size > MAX_BYTES) {
            setError("That file is too large to be a recipe.");
            return;
          }
          const text = await file.text();
          const check = parseRecipe(text);
          if (!check.ok) {
            setError(check.errors.join(" "));
            return;
          }
          await run(() => api.attachReferenceRecipe(profileId, text));
        }}
      />
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
