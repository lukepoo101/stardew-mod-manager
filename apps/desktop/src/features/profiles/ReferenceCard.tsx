import React, { useEffect, useMemo, useRef, useState } from "react";
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
  type Difference,
  type ProfileRecipe,
} from "@/shared/recipe/recipe";
import { RevisionUpdate } from "./RevisionUpdate";
import { Provenance } from "./Provenance";
import { chosenAlready, requirementsFor } from "@/shared/recipe/recommendation";
import { settingsDifferences } from "@/shared/recipe/settings";
import { useQuery } from "@tanstack/react-query";
import { CopyButton } from "@/components/ui/CopyButton";
import { CuratorNotes } from "@/components/ui/CuratorNotes";
import { UnfinishedChanges } from "@/components/ui/UnfinishedChanges";
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
  const [update, setUpdate] = useState<{
    before: ProfileRecipe;
    next: ProfileRecipe;
    text: string;
  } | null>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);

  const parsed = useMemo(
    () => (reference ? parseRecipe(reference.recipe_json) : null),
    [reference],
  );
  const comparison =
    parsed?.ok && mods ? compareWithRecipe(mods, parsed.recipe) : null;
  const accepted = new Set(reference?.accepted ?? []);
  // An optional mod that is not installed is a recommendation, not a
  // difference: declining it leaves the profile in step.
  const isRecommendation = (d: Difference) =>
    d.kind === "missing" && d.optional;
  const recommendations =
    comparison?.differences.filter(
      (d) => isRecommendation(d) && !accepted.has(differenceKey(d)),
    ) ?? [];
  const declined =
    comparison?.differences.filter(
      (d) => isRecommendation(d) && accepted.has(differenceKey(d)),
    ) ?? [];
  const open =
    comparison?.differences.filter(
      (d) =>
        !accepted.has(differenceKey(d)) &&
        !d.clientOnly &&
        !isRecommendation(d),
    ) ?? [];
  // Client-only mods need not match between players; listed, not counted.
  const clientOnly =
    comparison?.differences.filter(
      (d) => d.clientOnly && !accepted.has(differenceKey(d)),
    ) ?? [];
  const acceptedList =
    comparison?.differences.filter(
      (d) => accepted.has(differenceKey(d)) && !isRecommendation(d),
    ) ?? [];

  const installedIds = new Set(
    (mods ?? []).map((m) => m.unique_id.toLowerCase()),
  );

  // Shared settings, compared by checksum with this profile's.
  const sharesSettings = Boolean(
    parsed?.ok && parsed.recipe.components.some((c) => c.settings?.length),
  );
  const { data: hashes } = useQuery({
    queryKey: ["settings-hashes", profileId],
    queryFn: () => api.settingsHashes(profileId ?? ""),
    enabled: Boolean(profileId) && sharesSettings,
  });
  const allSettingsDiffs =
    parsed?.ok && hashes
      ? settingsDifferences(parsed.recipe, hashes, installedIds)
      : [];
  const settingsOpen = allSettingsDiffs.filter((d) => !accepted.has(d.key));
  const settingsKept = allSettingsDiffs.filter((d) => accepted.has(d.key));

  // Which packages the reference asks for are stored here, so a difference
  // can be fixed without fetching anything.
  const [stored, setStored] = useState<ReadonlySet<string>>(new Set());
  const wantedHashes = useMemo(
    () =>
      open
        .map((d) => d.recipe?.artifact_hash)
        .filter((hash): hash is string => Boolean(hash))
        .sort()
        .join(","),
    [open],
  );
  useEffect(() => {
    if (!wantedHashes) return;
    api
      .storedPackages(wantedHashes.split(","))
      .then((hashes) => setStored(new Set(hashes.map((h) => h.toLowerCase()))))
      .catch(() => setStored(new Set()));
  }, [wantedHashes]);

  if (!profileId) return null;

  const run = async (action: () => Promise<unknown>) => {
    setError(null);
    try {
      await action();
    } catch (actionError) {
      setError(errorSummary(actionError, "That did not work"));
    }
  };

  /** Puts one difference right through the normal workflows. */
  const fix = (d: Difference) => {
    const want = d.recipe;
    const have = d.installed;
    if (d.kind === "enabled" && have && want) {
      void run(async () => {
        const impact = await api.getToggleImpact(
          have.profile_component_id,
          want.enabled,
        );
        const notes = [
          impact.dependents.length > 0
            ? `These need it and will stop working: ${impact.dependents.join(", ")}.`
            : "",
          impact.unmet_requirements.length > 0
            ? impact.unmet_requirements.join(" ")
            : "",
        ]
          .filter(Boolean)
          .join(" ");
        if (
          !window.confirm(
            `${want.enabled ? "Enable" : "Disable"} ${have.name} as in the group's setup?${notes ? ` ${notes}` : ""}`,
          )
        )
          return;
        await api.setModsEnabled([have.profile_component_id], want.enabled);
      });
    } else if (d.kind === "extra" && have) {
      void run(async () => {
        const impact = await api.getToggleImpact(
          have.profile_component_id,
          false,
        );
        if (
          !window.confirm(
            `Disable ${have.name}? It is not in the group's setup.${
              impact.dependents.length > 0
                ? ` These need it and will stop working: ${impact.dependents.join(", ")}.`
                : ""
            } It stays installed and can be enabled again.`,
          )
        )
          return;
        await api.setModsEnabled([have.profile_component_id], false);
      });
    } else if ((d.kind === "version" || d.kind === "package") && want && have) {
      if (
        !window.confirm(
          `Replace ${have.name} ${have.version} with the group's ${want.version}? Its settings are kept and a restore point is saved first.`,
        )
      )
        return;
      void run(() => api.replaceModVersion(profileId, want.artifact_hash));
    } else if (d.kind === "missing" && want) {
      if (
        !window.confirm(
          `Install ${want.name} ${want.version} from the package stored here?`,
        )
      )
        return;
      void run(() => api.installStoredPackage(profileId, want.artifact_hash));
    }
  };

  /**
   * Installs a recommended mod from its stored package, then the required
   * mods it turns out to be missing, after one more review. Only a required
   * mod with exactly one fitting stored package is installed; the rest are
   * named.
   */
  const installRecommendation = (d: Difference, others: string[]) => {
    const want = d.recipe;
    if (!want) return;
    if (
      !window.confirm(
        `Install ${want.name} ${want.version} from the package stored here?${
          others.length > 0
            ? ` This choice asks you to pick one, and you already have ${others.join(", ")}.`
            : ""
        }`,
      )
    )
      return;
    void run(async () => {
      await api.installStoredPackage(profileId, want.artifact_hash);
      const overview = await api.getActiveProfileOverview();
      const plan = await requirementsFor(
        want.unique_id,
        overview.health_summary.findings,
        (id, minimum) => api.findStoredMod(id, minimum),
      );
      if (
        plan.install.length > 0 &&
        window.confirm(
          `${want.name} needs ${plan.install
            .map((r) => `${r.candidate.name} ${r.candidate.version}`)
            .join(
              ", ",
            )}. Install ${plan.install.length === 1 ? "it" : "them"} from the stored packages too?`,
        )
      ) {
        for (const requirement of plan.install)
          await api.installStoredPackage(
            profileId,
            requirement.candidate.artifact_hash,
            true,
          );
      }
      if (plan.unresolved.length > 0)
        throw new Error(
          `${want.name} was installed, but still needs ${plan.unresolved.join(", ")}, which no single stored package provides. See Diagnostics.`,
        );
    });
  };

  /**
   * Uses a file the user downloaded for a mod the group's package is not
   * stored for. The file is inspected first; it must contain that mod, and a
   * different version or file is only used after asking.
   */
  const supplyFile = (d: Difference) => {
    const want = d.recipe;
    if (!want) return;
    void run(async () => {
      const path = await api.pickArchiveDialog();
      if (!path) return;
      const preview = await api.inspectPackageForInstall(path, profileId);
      const cancel = () => api.cancelActiveOperation(preview.operation_id);
      const match = preview.detected_components.find(
        (c) => c.unique_id.toLowerCase() === want.unique_id.toLowerCase(),
      );
      if (!match) {
        await cancel();
        throw new Error(`That file does not contain ${want.unique_id}.`);
      }
      if (
        preview.artifact_hash.toLowerCase() !== want.artifact_hash.toLowerCase()
      ) {
        const why =
          match.version !== want.version
            ? `That file has ${match.name} ${match.version}; the group uses ${want.version}.`
            : `That file has ${match.name} ${match.version}, but it is not the same file the group uses.`;
        if (
          !window.confirm(
            `${why} Use it anyway? The difference stays listed until it matches.`,
          )
        ) {
          await cancel();
          return;
        }
      }
      if (d.kind === "missing") {
        if (preview.blockers.length > 0) {
          await cancel();
          throw new Error(preview.blockers.join(" "));
        }
        await api.executeOperation(preview.operation_id);
      } else {
        // An installed copy is swapped through the reviewed replace, which
        // keeps its settings and saves a restore point first.
        await cancel();
        await api.replaceModVersion(profileId, preview.artifact_hash);
      }
    });
  };

  /**
   * Puts every difference that has a fix right, after one review listing
   * them all. A restore point is saved first, so the reset can be undone;
   * differences with no fix here are left and named.
   */
  const resetAll = async () => {
    const fixable = open.filter((d) => "label" in fixFor(d));
    const left = open.filter((d) => !("label" in fixFor(d)));
    const name = parsed?.ok
      ? (parsed.recipe.collection?.name ?? parsed.recipe.profile_name)
      : "the reference";
    if (
      !window.confirm(
        `Put ${fixable.length + settingsOpen.length} difference(s) right to match ${name}?\n\n${[
          ...fixable.map((d) => `- ${d.unique_id}: ${d.detail}`),
          ...settingsOpen.map(
            (d) =>
              `- ${d.name}: use the shared settings (${d.files.join(", ")}); yours are backed up first`,
          ),
        ].join("\n")}\n\nA restore point is saved first.${
          left.length > 0
            ? ` ${left.length} difference(s) need files that are not stored here and are left.`
            : ""
        } Accepted differences are not touched.`,
      )
    )
      return;
    await run(async () => {
      await api.createRestorePoint(profileId, `Before matching ${name}`);
      // Journaled as one change, so an interrupted one is listed to finish.
      const journal = await api.beginChangeSet(
        profileId,
        `Putting differences right to match ${name}`,
        [
          ...fixable.map((d) => `${d.unique_id}: ${d.detail}`),
          ...settingsOpen.map((d) => `${d.name}: shared settings`),
        ],
      );
      const failed: string[] = [];
      for (const [index, d] of fixable.entries()) {
        const want = d.recipe;
        const have = d.installed;
        const failedBefore = failed.length;
        try {
          if (d.kind === "enabled" && have && want) {
            await api.setModsEnabled([have.profile_component_id], want.enabled);
          } else if (d.kind === "extra" && have) {
            await api.setModsEnabled([have.profile_component_id], false);
          } else if ((d.kind === "version" || d.kind === "package") && want) {
            await api.replaceModVersion(profileId, want.artifact_hash);
          } else if (d.kind === "missing" && want) {
            await api.installStoredPackage(profileId, want.artifact_hash);
          }
        } catch (fixError) {
          failed.push(
            `${d.unique_id} (${errorSummary(fixError, "not changed")})`,
          );
        }
        if (journal)
          await api.changeSetPartDone(
            journal,
            index,
            failed.length > failedBefore ? (failed.at(-1) ?? null) : null,
          );
      }
      // Settings last, into mods that are now installed.
      for (const [offset, d] of settingsOpen.entries()) {
        let problem: string | null = null;
        try {
          await api.applySharedSettings(profileId, d.unique_id, d.settings);
        } catch (settingsError) {
          problem = `${d.name} settings (${errorSummary(settingsError, "not changed")})`;
          failed.push(problem);
        }
        if (journal)
          await api.changeSetPartDone(
            journal,
            fixable.length + offset,
            problem,
          );
      }
      if (journal) await api.finishChangeSet(journal, failed);
      if (failed.length > 0) {
        throw new Error(
          `Some differences were not put right: ${failed.join("; ")}. The rest were.`,
        );
      }
    });
  };

  /** The fix offered for a difference, or why there is none. */
  const fixFor = (d: Difference): { label: string } | { missing: string } => {
    const storedHere = Boolean(
      d.recipe?.artifact_hash &&
        stored.has(d.recipe.artifact_hash.toLowerCase()),
    );
    switch (d.kind) {
      case "enabled":
        return { label: d.recipe?.enabled ? "Enable" : "Disable" };
      case "extra":
        return { label: "Disable" };
      case "version":
      case "package":
        return storedHere
          ? { label: "Use the group's version" }
          : {
              missing: `Get ${d.recipe?.name} ${d.recipe?.version} to match; that package is not stored here.`,
            };
      case "missing":
        return storedHere
          ? { label: "Install" }
          : {
              missing: d.recipe?.manual
                ? `Get ${d.recipe.name} ${d.recipe.version} by hand from ${d.recipe.manual.url}${
                    d.recipe.manual.instructions
                      ? ` (${d.recipe.manual.instructions})`
                      : ""
                  }, then choose the file here.`
                : `Get ${d.recipe?.name} ${d.recipe?.version} and install it from the Mods page.`,
            };
    }
  };

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Users className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Group reference</h3>
        {comparison && (
          <StatusBadge
            variant={
              open.length + settingsOpen.length === 0 ? "success" : "warning"
            }
          >
            {open.length + settingsOpen.length === 0
              ? "In step"
              : `${open.length + settingsOpen.length} difference(s)`}
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
          {parsed?.ok && (parsed.recipe.incomplete?.length ?? 0) > 0 && (
            <p>
              Whoever shared this did not have every mod their own reference
              asks for:{" "}
              {parsed.recipe.incomplete
                ?.map((i) => `${i.name} ${i.version}`)
                .join(", ")}
              . These are not listed as differences here.
            </p>
          )}
          {parsed?.ok && parsed.recipe.frozen && (
            <p>
              Frozen by whoever shared it on{" "}
              {new Date(parsed.recipe.frozen.frozen_at).toLocaleString()}
              {parsed.recipe.frozen.reason
                ? `: ${parsed.recipe.frozen.reason}`
                : ""}
              . These are the versions agreed for the group.
            </p>
          )}
          {parsed?.ok && parsed.recipe.collection && (
            <p>
              Collection "{parsed.recipe.collection.name}" revision{" "}
              {parsed.recipe.collection.revision}
              {parsed.recipe.collection.author
                ? ` by ${parsed.recipe.collection.author}`
                : ""}
              {parsed.recipe.collection.forked_from
                ? `, based on "${parsed.recipe.collection.forked_from.name}" revision ${parsed.recipe.collection.forked_from.revision}`
                : ""}
              .
            </p>
          )}
          {parsed?.ok && parsed.recipe.collection && (
            <CuratorNotes
              notes={parsed.recipe.collection.notes}
              className="text-[var(--fg-muted)]"
            />
          )}
          <UnfinishedChanges
            profileId={profileId}
            kind="reference"
            actions={() =>
              open.some((d) => "label" in fixFor(d)) ? (
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => void resetAll()}
                >
                  Review putting the rest right
                </Button>
              ) : null
            }
          />
          {comparison && open.length + settingsOpen.length === 0 && (
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
                    {d.recipe?.group && (
                      <span className="text-[var(--fg-muted)]">
                        {" "}
                        (optional, from the choice "{d.recipe.group}")
                      </span>
                    )}
                    {d.recipe?.manual && (
                      <CopyButton
                        value={d.recipe.manual.url}
                        label="download page"
                      />
                    )}
                    {d.recipe && d.kind !== "enabled" && (
                      <Provenance component={d.recipe} />
                    )}
                    {(() => {
                      const offer = fixFor(d);
                      return "missing" in offer ? (
                        <span className="block text-[var(--fg-muted)]">
                          {offer.missing}{" "}
                          <button
                            type="button"
                            onClick={() => supplyFile(d)}
                            className="text-[var(--accent-primary)] hover:underline cursor-pointer"
                          >
                            Choose the downloaded file...
                          </button>
                        </span>
                      ) : null;
                    })()}
                  </span>
                  {(() => {
                    const offer = fixFor(d);
                    return "label" in offer ? (
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => fix(d)}
                      >
                        {offer.label}
                      </Button>
                    ) : null;
                  })()}
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => {
                      // An optional reason, kept with the acceptance.
                      const note = window.prompt(
                        `Accept this difference for ${d.unique_id}? Say why if you like (optional).`,
                        "",
                      );
                      if (note === null) return;
                      void run(() =>
                        api.setReferenceDifferenceAccepted(
                          profileId,
                          differenceKey(d),
                          true,
                          note,
                        ),
                      );
                    }}
                  >
                    Accept
                  </Button>
                </li>
              ))}
            </ul>
          )}
          {settingsOpen.length > 0 && (
            <div className="space-y-1">
              <p className="font-semibold">Settings that differ</p>
              <ul className="space-y-1">
                {settingsOpen.map((d) => (
                  <li
                    key={d.key}
                    className="flex items-start justify-between gap-2"
                  >
                    <span>
                      {d.name}: {d.files.join(", ")} differ from the shared
                      settings.
                    </span>
                    <span className="flex gap-1">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => {
                          if (
                            !window.confirm(
                              `Replace ${d.name}'s settings (${d.settings
                                .map((s) => s.path)
                                .join(
                                  ", ",
                                )}) with the shared ones? Your current settings are backed up first and can be restored from the mod's details.`,
                            )
                          )
                            return;
                          void run(() =>
                            api.applySharedSettings(
                              profileId,
                              d.unique_id,
                              d.settings,
                            ),
                          );
                        }}
                      >
                        Use the shared settings
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          run(() =>
                            api.setReferenceDifferenceAccepted(
                              profileId,
                              d.key,
                              true,
                            ),
                          )
                        }
                      >
                        Keep mine
                      </Button>
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {settingsKept.length > 0 && (
            <details>
              <summary className="cursor-pointer">
                Settings kept as yours ({settingsKept.length})
              </summary>
              <ul className="mt-1 space-y-1">
                {settingsKept.map((d) => (
                  <li
                    key={d.key}
                    className="flex items-center justify-between gap-2"
                  >
                    <span>
                      {d.name}: {d.files.join(", ")}
                    </span>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        run(() =>
                          api.setReferenceDifferenceAccepted(
                            profileId,
                            d.key,
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
          {recommendations.length > 0 && parsed?.ok && (
            <div className="space-y-1">
              <p className="font-semibold">
                Recommended
                {parsed.recipe.collection
                  ? ` by ${parsed.recipe.collection.author || "the curator"} in "${parsed.recipe.collection.name}" revision ${parsed.recipe.collection.revision}`
                  : " in the recipe"}
                , not required
              </p>
              <ul className="space-y-1">
                {recommendations.map((d) => {
                  const group = parsed.recipe.groups?.find(
                    (g) => g.name === d.recipe?.group,
                  );
                  const offer = fixFor(d);
                  const others = chosenAlready(
                    group,
                    parsed.recipe.components,
                    installedIds,
                    d.unique_id,
                  );
                  return (
                    <li key={differenceKey(d)} className="space-y-0.5">
                      <div className="flex flex-wrap items-center gap-2">
                        <span>
                          {d.recipe?.name} {d.recipe?.version}
                        </span>
                        {"label" in offer && (
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => installRecommendation(d, others)}
                          >
                            Install
                          </Button>
                        )}
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
                          Not for me
                        </Button>
                      </div>
                      {group && (
                        <p className="text-[var(--fg-muted)]">
                          Part of the choice "{group.name}"
                          {group.choose === "one" ? " (pick one)" : ""}
                          {group.description ? `: ${group.description}` : ""}
                        </p>
                      )}
                      {others.length > 0 && (
                        <p>
                          You already have {others.join(", ")} from this choice,
                          which asks you to pick one.
                        </p>
                      )}
                      {(() => {
                        // From its manifest, kept apart from the curator's words.
                        const needs = (d.recipe?.requires ?? []).filter(
                          (id) => !installedIds.has(id.toLowerCase()),
                        );
                        return needs.length > 0 ? (
                          <p>
                            Also needs {needs.join(", ")} (from its manifest),
                            which you do not have.
                          </p>
                        ) : null;
                      })()}
                      {d.recipe && <Provenance component={d.recipe} />}
                      {d.recipe?.note ? (
                        <p>
                          <span className="text-[var(--fg-muted)]">
                            The curator says:{" "}
                          </span>
                          “{d.recipe.note}”
                        </p>
                      ) : (
                        <p className="text-[var(--fg-muted)]">
                          The curator gave no reason; it is only marked
                          optional.
                        </p>
                      )}
                      {"missing" in offer && (
                        <p className="text-[var(--fg-muted)]">
                          {offer.missing}{" "}
                          <button
                            type="button"
                            onClick={() => supplyFile(d)}
                            className="text-[var(--accent-primary)] hover:underline cursor-pointer"
                          >
                            Choose the downloaded file...
                          </button>
                        </p>
                      )}
                    </li>
                  );
                })}
              </ul>
              <p className="text-[var(--fg-muted)]">
                Installing one still goes through the normal checks, including
                its own requirements.
              </p>
            </div>
          )}
          {declined.length > 0 && (
            <details>
              <summary className="cursor-pointer">
                Not for me ({declined.length})
              </summary>
              <ul className="mt-1 space-y-1">
                {declined.map((d) => (
                  <li
                    key={differenceKey(d)}
                    className="flex items-center justify-between gap-2"
                  >
                    <span>
                      {d.recipe?.name} {d.recipe?.version}
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
                      Recommend again
                    </Button>
                  </li>
                ))}
              </ul>
            </details>
          )}
          {clientOnly.length > 0 && (
            <details>
              <summary className="cursor-pointer">
                Client-only differences, which need not match (
                {clientOnly.length})
              </summary>
              <ul className="mt-1 space-y-1">
                {clientOnly.map((d) => (
                  <li key={differenceKey(d)}>
                    <span className="font-mono">{d.unique_id}</span>: {d.detail}
                  </li>
                ))}
              </ul>
            </details>
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
                      {reference?.accepted_notes?.[differenceKey(d)] && (
                        <span className="block text-[var(--fg-muted)]">
                          Your reason:{" "}
                          {reference.accepted_notes[differenceKey(d)]}
                        </span>
                      )}
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
          <div className="flex flex-wrap gap-2">
            {(open.some((d) => "label" in fixFor(d)) ||
              settingsOpen.length > 0) && (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void resetAll()}
              >
                Put every difference right...
              </Button>
            )}
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
          const before = parsed?.ok ? parsed.recipe : null;
          const incoming = check.recipe.collection;
          const current = before?.collection;
          if (before && incoming && current) {
            if (incoming.id !== current.id) {
              if (
                !window.confirm(
                  `This is a different collection ("${incoming.name}") from the one this profile follows ("${current.name}"). Follow it instead? Differences you accepted are only kept where they still apply.`,
                )
              )
                return;
            } else if (incoming.revision <= current.revision) {
              if (
                !window.confirm(
                  `This is revision ${incoming.revision}, not newer than revision ${current.revision} that this profile follows. Use it anyway?`,
                )
              )
                return;
            } else {
              // A newer revision: reviewed in three ways before anything.
              setUpdate({ before, next: check.recipe, text });
              return;
            }
          }
          await run(() => api.attachReferenceRecipe(profileId, text));
        }}
      />
      {update && mods && (
        <RevisionUpdate
          profileId={profileId}
          before={update.before}
          next={update.next}
          text={update.text}
          mods={mods}
          onDone={(message) => {
            setUpdate(null);
            setUpdateStatus(message);
          }}
        />
      )}
      {updateStatus && (
        <p role="status" className="text-xs">
          {updateStatus}
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
