import React, { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useProfileMods,
  useReferenceRecipe,
} from "@/shared/api/hooks";
import {
  buildCollectionRecipe,
  type CollectionDraft,
  newDraft,
  readDraft,
} from "@/shared/recipe/collection";
import {
  diffRecipes,
  isEmptyChangelog,
  renderChangelog,
} from "@/shared/recipe/curator";
import {
  parseRecipe,
  type ProfileRecipe,
  serializeRecipe,
} from "@/shared/recipe/recipe";
import { downloadText } from "@/shared/support/actions";
import { checkCollection } from "@/shared/recipe/collectionChecks";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { Library } from "lucide-react";

const fileName = (draft: CollectionDraft, revision: number) =>
  `${(draft.name || "collection").replace(/[^A-Za-z0-9-_]+/g, "-")}-r${revision}.json`;

/**
 * Publishes the active profile as a collection: a named, versioned recipe
 * with the curator's choices (optional mods, option groups, versions that
 * may be newer, where to fetch files by hand). Each published revision is
 * kept unchanged; the next one is listed with what changed.
 */
export const CollectionCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: mods } = useProfileMods(profileId);
  const { data: savedDraft, isFetched } = useQuery({
    queryKey: ["collection-draft", profileId],
    queryFn: () => api.getCollectionDraft(profileId ?? ""),
    enabled: Boolean(profileId),
  });
  const { data: reference } = useReferenceRecipe(profileId);
  // A profile that follows someone's collection starts its own as a fork of
  // that revision; the original is never changed.
  const followed = useMemo(() => {
    if (!reference) return null;
    const parsed = parseRecipe(reference.recipe_json);
    return parsed.ok && parsed.recipe.collection ? parsed.recipe : null;
  }, [reference]);
  const [draft, setDraft] = useState<CollectionDraft | null>(null);
  useEffect(() => {
    if (!overview || !isFetched) return;
    const fresh = newDraft(overview.profile.name, crypto.randomUUID());
    const parent = followed?.collection;
    setDraft(
      readDraft(
        savedDraft ?? null,
        parent
          ? {
              ...fresh,
              name: `${parent.name} (my version)`,
              forkedFrom: {
                id: parent.id,
                revision: parent.revision,
                name: parent.name,
              },
            }
          : fresh,
      ),
    );
  }, [overview, savedDraft, isFetched, followed]);
  const { data: revisions, refetch: refetchRevisions } = useQuery({
    queryKey: ["collection-revisions", draft?.id],
    queryFn: () => api.listCollectionRevisions(draft?.id ?? ""),
    enabled: Boolean(draft?.id),
  });
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [warningsReviewed, setWarningsReviewed] = useState(false);
  const { data: map } = useQuery({
    queryKey: ["dependency-map", profileId],
    queryFn: () => api.getDependencyMap(profileId ?? ""),
    enabled: Boolean(profileId),
  });

  const nextRevision = (revisions?.at(-1)?.revision ?? 0) + 1;
  const lastRecipe: ProfileRecipe | null = useMemo(() => {
    const last = revisions?.at(-1);
    if (!last) return null;
    const parsed = parseRecipe(last.recipe_json);
    return parsed.ok ? parsed.recipe : null;
  }, [revisions]);
  const next =
    overview && mods && draft
      ? buildCollectionRecipe(
          overview,
          mods,
          draft,
          nextRevision,
          new Date().toISOString(),
        )
      : null;
  // Before a fork's first revision, compare with the collection it is
  // based on; after that, with its own last revision.
  const baseline =
    lastRecipe ??
    (draft?.forkedFrom && followed?.collection?.id === draft.forkedFrom.id
      ? followed
      : null);
  const changelog = next && baseline ? diffRecipes(baseline, next) : null;

  const report =
    next && draft
      ? checkCollection(next, draft, {
          findings: overview?.health_summary?.findings,
          map,
          installedAsDependency: new Set(
            (mods ?? [])
              .filter((m) => m.installed_reason === "dependency")
              .map((m) => m.unique_id.toLowerCase()),
          ),
          changelog,
        })
      : null;
  const errors = report?.checks.filter((c) => c.level === "error") ?? [];
  const warnings = report?.checks.filter((c) => c.level === "warning") ?? [];
  const limitations =
    report?.checks.filter((c) => c.level === "limitation") ?? [];

  if (!overview || !mods || !draft) return null;

  const update = (change: Partial<CollectionDraft>) =>
    setDraft({ ...draft, ...change });
  const choice = (uniqueId: string) => draft.mods[uniqueId.toLowerCase()] ?? {};
  const setChoice = (uniqueId: string, change: object) =>
    update({
      mods: {
        ...draft.mods,
        [uniqueId.toLowerCase()]: { ...choice(uniqueId), ...change },
      },
    });

  const save = async () => {
    setStatus(null);
    try {
      await api.saveCollectionDraft(overview.profile.id, JSON.stringify(draft));
      setStatus("Draft saved.");
    } catch (error) {
      setStatus(errorSummary(error, "The draft was not saved"));
    }
  };

  const publish = async () => {
    if (!next) return;
    if (
      !window.confirm(
        `Publish revision ${nextRevision} of "${draft.name}"? A published revision is kept exactly as it is; later changes become revision ${nextRevision + 1}.`,
      )
    )
      return;
    setBusy(true);
    setStatus(null);
    try {
      await api.saveCollectionDraft(overview.profile.id, JSON.stringify(draft));
      const text = serializeRecipe(next);
      await api.publishCollectionRevision(text);
      downloadText(fileName(draft, nextRevision), text);
      setStatus(
        `Published revision ${nextRevision}. The recipe file was saved through your browser's download.`,
      );
      update({ notes: "" });
      await refetchRevisions();
    } catch (error) {
      setStatus(errorSummary(error, "Nothing was published"));
    } finally {
      setBusy(false);
    }
  };

  /**
   * Makes an empty profile that follows the latest published revision, so
   * the curator can rebuild the collection the way a recipient would. The
   * curator's own profile is not touched.
   */
  const tryClean = async () => {
    const latest = revisions?.at(-1);
    if (!latest) return;
    setStatus(null);
    try {
      const profile = await api.createProfile(
        `${draft.name} r${latest.revision} test`,
        overview.game.id,
      );
      await api.attachReferenceRecipe(profile.id, latest.recipe_json);
      setStatus(
        `Made "${profile.name}", an empty profile following revision ${latest.revision}. Switch to it and use Put every difference right on its Group reference to rebuild it as a recipient would; anything it cannot fetch is listed there. Archive and delete it when done.`,
      );
    } catch (error) {
      setStatus(errorSummary(error, "The test profile was not made"));
    }
  };

  const input =
    "px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] text-xs";

  return (
    <Card className="space-y-3 text-xs">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Library className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Publish as a collection</h3>
      </div>
      <p className="text-[var(--fg-muted)]">
        A collection is a recipe others can follow, with a name, an author and
        numbered revisions. Nothing is uploaded: publishing keeps the revision
        here and saves its file for you to share.
      </p>
      {draft.forkedFrom && (
        <p>
          Based on "{draft.forkedFrom.name}" revision{" "}
          {draft.forkedFrom.revision}. That collection is not changed.
        </p>
      )}
      <div className="grid grid-cols-[auto_1fr] gap-2 items-center">
        <label htmlFor="collection-name">Name</label>
        <input
          id="collection-name"
          className={input}
          value={draft.name}
          onChange={(e) => update({ name: e.target.value })}
        />
        <label htmlFor="collection-author">Your name</label>
        <input
          id="collection-author"
          className={input}
          value={draft.author}
          onChange={(e) => update({ author: e.target.value })}
        />
        <label htmlFor="collection-notes">What changed</label>
        <textarea
          id="collection-notes"
          className={input}
          rows={2}
          value={draft.notes}
          onChange={(e) => update({ notes: e.target.value })}
        />
      </div>

      <details>
        <summary className="cursor-pointer font-semibold">
          Option groups ({draft.groups.length})
        </summary>
        <ul className="space-y-1 mt-1">
          {draft.groups.map((group, index) => (
            <li key={group.name} className="flex flex-wrap gap-2 items-center">
              <input
                aria-label="Group name"
                className={input}
                value={group.name}
                onChange={(e) =>
                  update({
                    groups: draft.groups.map((g, i) =>
                      i === index ? { ...g, name: e.target.value } : g,
                    ),
                  })
                }
              />
              <input
                aria-label="Group description"
                className={input}
                value={group.description}
                placeholder="What the choice is about"
                onChange={(e) =>
                  update({
                    groups: draft.groups.map((g, i) =>
                      i === index ? { ...g, description: e.target.value } : g,
                    ),
                  })
                }
              />
              <select
                aria-label="Recipients choose"
                className={input}
                value={group.choose}
                onChange={(e) =>
                  update({
                    groups: draft.groups.map((g, i) =>
                      i === index
                        ? { ...g, choose: e.target.value as "any" | "one" }
                        : g,
                    ),
                  })
                }
              >
                <option value="any">Choose any</option>
                <option value="one">Choose one</option>
              </select>
            </li>
          ))}
        </ul>
        <button
          type="button"
          className="underline cursor-pointer mt-1"
          onClick={() =>
            update({
              groups: [
                ...draft.groups,
                {
                  name: `Group ${draft.groups.length + 1}`,
                  description: "",
                  choose: "any",
                },
              ],
            })
          }
        >
          Add a group
        </button>
      </details>

      <details>
        <summary className="cursor-pointer font-semibold">
          Choices per mod ({mods.length})
        </summary>
        <ul className="divide-y divide-[var(--border)] mt-1">
          {mods.map((mod) => {
            const c = choice(mod.unique_id);
            return (
              <li key={mod.profile_component_id} className="py-1 space-y-1">
                <p className="font-medium">
                  {mod.name} {mod.version}
                </p>
                <div className="flex flex-wrap gap-3 items-center">
                  <label className="flex items-center gap-1">
                    <input
                      type="checkbox"
                      checked={Boolean(c.optional)}
                      onChange={(e) =>
                        setChoice(mod.unique_id, { optional: e.target.checked })
                      }
                    />
                    Optional
                  </label>
                  <label className="flex items-center gap-1">
                    <input
                      type="checkbox"
                      checked={Boolean(c.newerOk)}
                      onChange={(e) =>
                        setChoice(mod.unique_id, { newerOk: e.target.checked })
                      }
                    />
                    Newer versions are fine
                  </label>
                  <label className="flex items-center gap-1">
                    <input
                      type="checkbox"
                      checked={Boolean(c.clientOnly)}
                      onChange={(e) =>
                        setChoice(mod.unique_id, {
                          clientOnly: e.target.checked,
                        })
                      }
                    />
                    Client-only (players need not match)
                  </label>
                  {draft.groups.length > 0 && (
                    <select
                      aria-label={`Group for ${mod.name}`}
                      className={input}
                      value={c.group ?? ""}
                      onChange={(e) =>
                        setChoice(mod.unique_id, {
                          group: e.target.value || undefined,
                        })
                      }
                    >
                      <option value="">No group</option>
                      {draft.groups.map((g) => (
                        <option key={g.name} value={g.name}>
                          {g.name}
                        </option>
                      ))}
                    </select>
                  )}
                  <input
                    aria-label={`Why ${mod.name} is included`}
                    className={input}
                    placeholder="Why it is included (optional)"
                    value={c.note ?? ""}
                    onChange={(e) =>
                      setChoice(mod.unique_id, { note: e.target.value })
                    }
                  />
                  <input
                    aria-label={`Where to get ${mod.name} by hand`}
                    className={input}
                    placeholder="Download page, if fetched by hand"
                    value={c.manualUrl ?? ""}
                    onChange={(e) =>
                      setChoice(mod.unique_id, { manualUrl: e.target.value })
                    }
                  />
                  {c.manualUrl && (
                    <input
                      aria-label={`How to get ${mod.name}`}
                      className={input}
                      placeholder="What to download there"
                      value={c.manualInstructions ?? ""}
                      onChange={(e) =>
                        setChoice(mod.unique_id, {
                          manualInstructions: e.target.value,
                        })
                      }
                    />
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      </details>

      {changelog && baseline && next && (
        <details open>
          <summary className="cursor-pointer font-semibold">
            {lastRecipe
              ? `Changes since revision ${lastRecipe.collection?.revision}`
              : `Changes from "${baseline.collection?.name}" revision ${baseline.collection?.revision}`}
          </summary>
          <pre className="whitespace-pre-wrap font-mono mt-1">
            {isEmptyChangelog(changelog)
              ? "No mod changes."
              : renderChangelog(baseline, next, changelog)}
          </pre>
        </details>
      )}

      {report && (
        <div className="space-y-1">
          <p className="font-semibold flex items-center gap-2">
            Before publishing
            <StatusBadge variant={errors.length > 0 ? "danger" : "success"}>
              {errors.length > 0
                ? `${errors.length} to fix`
                : "Nothing blocks publishing"}
            </StatusBadge>
          </p>
          <p>
            {report.reproducibility}% of mods are pinned to an exact file.{" "}
            {report.fullyAutomatic
              ? "Recipients need no manual steps."
              : "Not fully automatic: recipients have manual steps."}{" "}
            A recipient meets {report.recipient.exactFiles} exact file(s),{" "}
            {report.recipient.manual} download(s) by hand and{" "}
            {report.recipient.optional} optional mod(s).
          </p>
          {[...errors, ...warnings, ...limitations].length > 0 && (
            <ul className="space-y-0.5">
              {[...errors, ...warnings, ...limitations].map((check) => (
                <li key={check.message} className="flex flex-wrap gap-2">
                  <StatusBadge
                    variant={
                      check.level === "error"
                        ? "danger"
                        : check.level === "warning"
                          ? "warning"
                          : "neutral"
                    }
                  >
                    {check.level === "error"
                      ? "Fix"
                      : check.level === "warning"
                        ? "Check"
                        : "Note"}
                  </StatusBadge>
                  <span>{check.message}</span>
                  {check.subject &&
                    mods.some(
                      (m) =>
                        m.unique_id.toLowerCase() ===
                        check.subject?.toLowerCase(),
                    ) &&
                    check.message.includes("mark it as intended") && (
                      <button
                        type="button"
                        className="underline cursor-pointer"
                        onClick={() =>
                          setChoice(check.subject ?? "", { intended: true })
                        }
                      >
                        Mark as intended
                      </button>
                    )}
                </li>
              ))}
            </ul>
          )}
          {warnings.length > 0 && (
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={warningsReviewed}
                onChange={(e) => setWarningsReviewed(e.target.checked)}
              />
              I have read the warnings and want to publish anyway
            </label>
          )}
          <p className="text-[var(--fg-muted)]">
            These checks are about how exactly others can reproduce this
            collection, not whether its mods are safe or work together.
          </p>
        </div>
      )}

      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="secondary" onClick={() => void save()}>
          Save draft
        </Button>
        <Button
          size="sm"
          disabled={
            busy ||
            !draft.name.trim() ||
            errors.length > 0 ||
            (warnings.length > 0 && !warningsReviewed)
          }
          isLoading={busy}
          onClick={() => void publish()}
        >
          Publish revision {nextRevision}
        </Button>
      </div>

      {revisions && revisions.length > 0 && (
        <div>
          <button
            type="button"
            className="underline cursor-pointer"
            onClick={() => void tryClean()}
          >
            Try the latest revision in a clean profile
          </button>
          <p className="font-semibold">Published revisions</p>
          <ul>
            {revisions
              .slice()
              .reverse()
              .map((revision) => (
                <li key={revision.revision} className="flex gap-2 items-center">
                  <span>
                    Revision {revision.revision},{" "}
                    {new Date(revision.published_at).toLocaleString()}
                  </span>
                  <button
                    type="button"
                    className="underline cursor-pointer"
                    onClick={() =>
                      downloadText(
                        fileName(draft, revision.revision),
                        revision.recipe_json,
                      )
                    }
                  >
                    Save file
                  </button>
                </li>
              ))}
          </ul>
        </div>
      )}
      {status && <p role="status">{status}</p>}
    </Card>
  );
};
