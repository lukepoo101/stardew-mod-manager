import React, { useEffect, useMemo, useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { CuratorNotes } from "@/components/ui/CuratorNotes";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { ModListItemDto } from "@/shared/api/generated";
import { diffRecipes, renderChangelog } from "@/shared/recipe/curator";
import { type MergeItem, planMerge } from "@/shared/recipe/merge";
import {
  compareWithRecipe,
  differenceKey,
  type ProfileRecipe,
} from "@/shared/recipe/recipe";

type Choice = "upstream" | "mine" | "later";

/**
 * Reviews updating a followed collection to a newer revision, from three
 * sides: what the collection changed, what you changed, and where both did.
 * Collection changes you had not touched are applied; for each conflict you
 * choose. Everything applied goes through the normal checks as one journaled
 * change, after a restore point.
 */
export const RevisionUpdate: React.FC<{
  profileId: string;
  before: ProfileRecipe;
  next: ProfileRecipe;
  text: string;
  mods: readonly ModListItemDto[];
  onDone: (message: string | null) => void;
}> = ({ profileId, before, next, text, mods, onDone }) => {
  const items = useMemo(
    () => planMerge(before, next, mods),
    [before, next, mods],
  );
  const [choices, setChoices] = useState<Record<string, Choice>>({});
  const [stored, setStored] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const hashes = items
      .map((i) => i.theirs?.artifact_hash)
      .filter((h): h is string => Boolean(h));
    if (hashes.length === 0) return;
    api
      .storedPackages(hashes)
      .then((found) => setStored(new Set(found.map((h) => h.toLowerCase()))))
      .catch(() => setStored(new Set()));
  }, [items]);

  const choiceOf = (item: MergeItem): Choice =>
    item.kind === "upstream"
      ? "upstream"
      : item.kind === "local"
        ? "mine"
        : (choices[item.unique_id] ?? "later");
  const collection = next.collection;
  const notes = collection?.notes ?? "";
  const log = renderChangelog(before, next, diffRecipes(before, next));
  const of = (kind: MergeItem["kind"]) => items.filter((i) => i.kind === kind);
  /** Whether taking the collection's side means installing a package. */
  const needsPackage = (i: MergeItem) =>
    Boolean(
      i.theirs &&
        (!i.mine ||
          i.theirs.version !== i.mine.version ||
          (i.theirs.artifact_hash &&
            i.mine.artifact_hash &&
            i.theirs.artifact_hash.toLowerCase() !==
              i.mine.artifact_hash.toLowerCase())),
    );

  /** Brings one mod to the collection's side, through the normal paths. */
  const take = async (item: MergeItem) => {
    const want = item.theirs;
    const have = item.mine;
    const isStored = (hash?: string) =>
      Boolean(hash && stored.has(hash.toLowerCase()));
    if (!want && have) {
      // Removed by the collection: turned off, not deleted.
      await api.setModsEnabled([have.profile_component_id], false);
    } else if (want && !have) {
      if (!isStored(want.artifact_hash))
        throw new Error("its package is not stored here");
      await api.installStoredPackage(profileId, want.artifact_hash);
    } else if (want && have) {
      if (
        want.version !== have.version ||
        (want.artifact_hash &&
          have.artifact_hash &&
          want.artifact_hash.toLowerCase() !== have.artifact_hash.toLowerCase())
      ) {
        if (!isStored(want.artifact_hash))
          throw new Error("its package is not stored here");
        await api.replaceModVersion(profileId, want.artifact_hash);
      }
      if (want.enabled !== have.enabled)
        await api.setModsEnabled([have.profile_component_id], want.enabled);
    }
  };

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.attachReferenceRecipe(profileId, text);
      const toTake = items.filter((i) => choiceOf(i) === "upstream");
      const failed: string[] = [];
      if (toTake.length > 0) {
        await api.createRestorePoint(
          profileId,
          `Before revision ${collection?.revision ?? ""} of ${collection?.name ?? "the collection"}`.slice(
            0,
            80,
          ),
        );
        const journal = await api.beginChangeSet(
          profileId,
          `Updating to revision ${collection?.revision} of ${collection?.name}`,
          toTake.map((i) => `${i.name}: ${i.upstream}`),
        );
        for (const [index, item] of toTake.entries()) {
          let problem: string | null = null;
          try {
            await take(item);
          } catch (takeError) {
            problem = `${item.name} (${errorSummary(takeError, "not changed")})`;
            failed.push(problem);
          }
          if (journal) await api.changeSetPartDone(journal, index, problem);
        }
        if (journal) await api.finishChangeSet(journal, failed);
      }
      // Kept on purpose: accepted as differences from the new revision.
      const keep = items.filter(
        (i) => i.kind === "conflict" && choiceOf(i) === "mine",
      );
      if (keep.length > 0) {
        const now = await api.listProfileMods(profileId);
        const keepIds = new Set(keep.map((i) => i.unique_id.toLowerCase()));
        for (const d of compareWithRecipe(now, next).differences)
          if (keepIds.has(d.unique_id.toLowerCase()))
            await api.setReferenceDifferenceAccepted(
              profileId,
              differenceKey(d),
              true,
            );
      }
      const later = items.filter((i) => choiceOf(i) === "later").length;
      onDone(
        `Now following revision ${collection?.revision}. ${
          toTake.length - failed.length
        } change(s) taken from the collection${
          keep.length > 0 ? `, ${keep.length} of yours kept` : ""
        }${later > 0 ? `, ${later} left to decide` : ""}.${
          failed.length > 0 ? ` Not changed: ${failed.join("; ")}.` : ""
        }`,
      );
    } catch (applyError) {
      setError(errorSummary(applyError, "The update did not finish"));
    } finally {
      setBusy(false);
    }
  };

  const list = (
    title: string,
    rows: MergeItem[],
    text: (i: MergeItem) => string,
  ) =>
    rows.length > 0 && (
      <div>
        <p className="font-semibold">{title}</p>
        <ul className="list-disc pl-4">
          {rows.map((i) => (
            <li key={i.unique_id}>{text(i)}</li>
          ))}
        </ul>
      </div>
    );

  return (
    <Modal
      labelledBy="revision-update-title"
      onClose={busy ? undefined : () => onDone(null)}
      className="space-y-3 text-xs max-h-[85vh] overflow-y-auto"
    >
      <h2 id="revision-update-title" className="text-lg font-bold">
        Update to revision {collection?.revision} of "{collection?.name}"?
      </h2>
      <details>
        <summary className="cursor-pointer font-semibold">
          What changed (worked out by the manager)
        </summary>
        <pre className="whitespace-pre-wrap">{log}</pre>
      </details>
      {notes.trim() ? (
        <div>
          <p className="font-semibold">The curator's notes</p>
          <CuratorNotes notes={notes} />
        </div>
      ) : (
        <p className="text-[var(--fg-muted)]">The curator left no notes.</p>
      )}
      {items.length === 0 && (
        <p>Your profile already matches what this revision changes.</p>
      )}
      {list(
        "From the collection, applied",
        of("upstream"),
        (i) =>
          `${i.name}: ${i.upstream}${
            needsPackage(i) &&
            !stored.has((i.theirs?.artifact_hash ?? "").toLowerCase())
              ? " (package not stored here, so it stays a difference)"
              : ""
          }`,
      )}
      {of("conflict").length > 0 && (
        <div className="space-y-1">
          <p className="font-semibold">
            Changed by both you and the collection
          </p>
          {of("conflict").map((i) => (
            <fieldset key={i.unique_id} className="space-y-0.5">
              <legend>
                {i.name}: the collection {i.upstream}; you {i.local}
              </legend>
              {(
                [
                  ["upstream", "Take the collection's"],
                  ["mine", "Keep mine"],
                  ["later", "Decide later"],
                ] as const
              ).map(([value, label]) => (
                <label key={value} className="mr-3 inline-flex gap-1">
                  <input
                    type="radio"
                    name={`merge-${i.unique_id}`}
                    checked={choiceOf(i) === value}
                    onChange={() =>
                      setChoices({ ...choices, [i.unique_id]: value })
                    }
                  />
                  {label}
                </label>
              ))}
            </fieldset>
          ))}
        </div>
      )}
      {list(
        "Your own changes, kept",
        of("local"),
        (i) => `${i.name}: ${i.local}`,
      )}
      <p className="text-[var(--fg-muted)]">
        A restore point is saved first. Changes go through the normal checks,
        including each mod's requirements, as one change listed in Activity;
        mods the collection removed are turned off, not deleted. Anything that
        cannot be done here stays listed as a difference.
      </p>
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => onDone(null)}
        >
          Cancel
        </Button>
        <Button isLoading={busy} onClick={() => void apply()}>
          Update
        </Button>
      </div>
    </Modal>
  );
};
