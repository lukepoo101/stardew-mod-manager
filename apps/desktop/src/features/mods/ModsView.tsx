import { matchSkipped, skippedByComponent } from "@/shared/diagnostics/skipped";
import { handleRowNavigation } from "@/shared/a11y/rowNavigation";
import { Modal } from "@/components/ui/Modal";
import * as RadixDialog from "@radix-ui/react-dialog";
import { focusedElement, returnFocus } from "@/shared/a11y/focusReturn";
import { CopyButton } from "@/components/ui/CopyButton";
import { usePreferences, type ModFilter } from "@/shared/preferences";
import React, { useState, useMemo, useRef } from "react";
import { Card } from "@/components/ui/Card";
import { EmptyState, LoadFailed, Loading } from "@/components/ui/EmptyState";
import { Button } from "@/components/ui/Button";
import {
  useActiveProfileOverview,
  useProfileMods,
  useExecuteOperation,
  useModAnnotations,
  useDiagnosticsReport,
  useModProblems,
  useQuickFileCheck,
} from "@/shared/api/hooks";
import {
  ModListItemDto,
  ModDetailsDto,
  ToggleImpactDto,
  OperationPreviewDto,
} from "@/shared/api/generated";
import { api } from "@/shared/api/client";
import { errorCode, errorSummary } from "@/shared/api/errors";
import { UndoRemovalButton } from "@/features/activity/UndoRemoval";
import { ProfileModInstaller } from "./ProfileModInstaller";
import { ModNotesPanel, type UnsavedState } from "./ModNotesPanel";
import { askToConfirm } from "@/shared/platform/confirm";
import {
  MOD_SORTS,
  MOD_SORT_LABELS,
  allTags,
  annotationFor,
  hasTag,
  indexAnnotations,
  sortMods,
  type ModSort,
} from "@/shared/mods/organise";
import { BulkToggleBar } from "./BulkToggleBar";
import { matchesEveryWord } from "@/shared/search/search";
import { ModRelationsPanel } from "./ModRelationsPanel";
import { PackageFiles } from "./PackageFiles";
import { copyText } from "@/shared/support/actions";
import { folderOrAsk, useShareFolder } from "@/features/profiles/ShareFolder";
import { buildInventory, serializeInventory } from "@/shared/support/inventory";
import { unreadManifestFields } from "@/shared/mods/manifestFields";
import { enabledDependents } from "@/shared/mods/dependents";
import { MOD_TRUST_DETAIL, MOD_TRUST_SUMMARY } from "@/shared/security/trust";
import { Search, Package, X, FileCode, Download } from "lucide-react";
import { KIND_LABELS, ModRow, type ModRowActions } from "./ModRow";

/** Filter value for mods with no tags, so organising never hides them. */
const UNTAGGED = "\u0000untagged";

export const ModsView: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const {
    data: mods,
    error: modsError,
    refetch: refetchMods,
  } = useProfileMods(profileId);
  const execute = useExecuteOperation();
  const [removalExtras, setRemovalExtras] = useState<string[] | null>(null);
  // Enabled mods that stop loading without the removed one, and whether to
  // turn them off in the same reviewed change.
  const [removalDependents, setRemovalDependents] = useState<ModListItemDto[]>(
    [],
  );
  const [alsoDisable, setAlsoDisable] = useState(false);
  const [removalPreview, setRemovalPreview] =
    useState<OperationPreviewDto | null>(null);

  // A search opened from the global search arrives as `?q=` on the hash route.
  const [search, setSearch] = useState(() => {
    try {
      const query = window.location.hash.split("?")[1] ?? "";
      return new URLSearchParams(query).get("q") ?? "";
    } catch {
      return "";
    }
  });
  const [savedPreferences, updatePreferences] = usePreferences();
  const filterEnabled = savedPreferences.modFilter;
  const setFilterEnabled = (modFilter: ModFilter) =>
    updatePreferences({ modFilter });
  const setSort = (modSort: ModSort) => updatePreferences({ modSort });
  const { data: annotationList } = useModAnnotations();
  const annotations = useMemo(
    () => indexAnnotations(annotationList),
    [annotationList],
  );
  const tagOptions = useMemo(() => allTags(annotations), [annotations]);
  const [tagFilter, setTagFilter] = useState("");
  const [kindFilter, setKindFilter] = useState("");
  const { data: report } = useDiagnosticsReport(overview?.game.id);
  const skipped = useMemo(
    () =>
      skippedByComponent(
        matchSkipped(report?.log_summary.skipped_mods ?? [], mods ?? []),
      ),
    [report, mods],
  );
  const { data: quickCheck } = useQuickFileCheck(profileId);
  // Folders whose files look changed outside the manager, by deployment.
  const changedFiles = useMemo(
    () =>
      new Map(
        (quickCheck ?? [])
          .filter((c) => c.status === "changed")
          .map((c) => [c.deployment_id, c]),
      ),
    [quickCheck],
  );
  const [favouritesOnly, setFavouritesOnly] = useState(false);
  const [attentionOnly, setAttentionOnly] = useState(false);
  const { data: problemList } = useModProblems(profileId);
  const problems = useMemo(
    () =>
      new Map(
        (problemList ?? []).map((p) => [
          p.profile_component_id,
          p.unmet_requirements,
        ]),
      ),
    [problemList],
  );
  const [selectedModId, setSelectedModId] = useState<string | null>(null);
  const [checked, setChecked] = useState<ReadonlySet<string>>(new Set());
  // The row last ticked or unticked, where a Shift+click range starts.
  const selectionAnchor = useRef<string | null>(null);
  const [modDetails, setModDetails] = useState<ModDetailsDto | null>(null);
  const [loadingDetails, setLoadingDetails] = useState(false);
  const [isRemoving, setIsRemoving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tagStatus, setTagStatus] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState<string | null>(null);
  const [togglePlan, setTogglePlan] = useState<{
    mod: ModListItemDto;
    enable: boolean;
    impact: ToggleImpactDto;
  } | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);
  // A finished removal, shown with a way to undo it.
  const [removalResult, setRemovalResult] = useState<{
    operationId: string;
    names: string[];
    note: string | null;
  } | null>(null);
  // Another folder with the mod's name is in Mods; the repair is offered.
  const [collision, setCollision] = useState<{
    mod: ModListItemDto;
    enable: boolean;
  } | null>(null);

  const applyToggle = async (mod: ModListItemDto, enable: boolean) => {
    setToggling(mod.profile_component_id);
    setError(null);
    setCollision(null);
    try {
      await api.setModEnabled(mod.profile_component_id, enable);
      setTogglePlan(null);
      setCopyStatus(`${mod.name} ${enable ? "enabled" : "disabled"}`);
    } catch (toggleError) {
      setTogglePlan(null);
      if (errorCode(toggleError) === "MOD_FOLDER_COLLISION") {
        setCollision({ mod, enable });
      }
      setError(
        errorSummary(
          toggleError,
          `Failed to ${enable ? "enable" : "disable"} ${mod.name}`,
        ),
      );
    } finally {
      setToggling(null);
    }
  };

  const repairCollision = async () => {
    if (!collision) return;
    const { mod, enable } = collision;
    setToggling(mod.profile_component_id);
    try {
      await api.moveAsideAndSetModEnabled(mod.profile_component_id, enable);
      setCollision(null);
      setError(null);
      setCopyStatus(
        `${mod.name} ${enable ? "enabled" : "disabled"}; the other folder is in the recovery folder`,
      );
    } catch (repairError) {
      setError(errorSummary(repairError, "The other folder was not moved"));
    } finally {
      setToggling(null);
    }
  };

  // Anything that reaches beyond the one mod is shown before it happens.
  const requestToggle = async (mod: ModListItemDto) => {
    const enable = !mod.enabled;
    setError(null);
    try {
      const impact = await api.getToggleImpact(
        mod.profile_component_id,
        enable,
      );
      const reachesFurther =
        impact.affected_mods.length > 1 ||
        impact.dependents.length > 0 ||
        impact.unmet_requirements.length > 0;
      if (reachesFurther) {
        setTogglePlan({ mod, enable, impact });
      } else {
        await applyToggle(mod, enable);
      }
    } catch (impactError) {
      setError(errorSummary(impactError, "Failed to check what this affects"));
    }
  };

  // Copies exactly the canonical UniqueID, never the display name or a
  // formatted string.
  const handleCopyId = async (uniqueId: string) => {
    setCopyStatus(
      (await copyText(uniqueId))
        ? `Copied ${uniqueId}`
        : "Could not access the clipboard",
    );
  };

  // Saved where shared files go (the folder recipes and bundles use), never
  // over a file; asked for the first time.
  const [shareFolder, setShareFolder] = useShareFolder();
  const handleExportInventory = async () => {
    if (!overview || !mods) return;
    try {
      const target = await folderOrAsk(
        shareFolder,
        "Choose where to save the inventory",
        setShareFolder,
      );
      if (!target) return;
      const path = await api.saveJsonExport(
        target,
        `${overview.profile.name} inventory.json`,
        serializeInventory(
          buildInventory(overview, mods, {
            generatedAt: new Date().toISOString(),
          }),
        ),
      );
      setCopyStatus(`Saved the inventory to ${path}.`);
    } catch (exportError) {
      setError(errorSummary(exportError, "The inventory was not saved"));
    }
  };

  const filteredMods = useMemo(() => {
    if (!mods) return [];
    const matching = mods.filter((m) => {
      // Every word, in any order, ignoring case and accents: the same rule
      // as global search.
      if (!matchesEveryWord(search, [m.name, m.unique_id, m.author]))
        return false;
      if (filterEnabled === "enabled" && !m.enabled) return false;
      if (filterEnabled === "disabled" && m.enabled) return false;
      if (tagFilter === UNTAGGED) {
        if ((annotationFor(annotations, m)?.tags.length ?? 0) > 0) return false;
      } else if (tagFilter && !hasTag(annotations, m, tagFilter)) return false;
      if (kindFilter && m.kind !== kindFilter) return false;
      if (favouritesOnly && !annotationFor(annotations, m)?.favourite)
        return false;
      if (
        attentionOnly &&
        !problems.has(m.profile_component_id) &&
        !skipped.has(m.profile_component_id) &&
        !m.folder_missing &&
        !changedFiles.has(m.deployment_id)
      )
        return false;
      return true;
    });
    return sortMods(
      matching,
      savedPreferences.modSort,
      annotations,
      savedPreferences.modSortDescending,
    );
  }, [
    mods,
    search,
    filterEnabled,
    tagFilter,
    kindFilter,
    annotations,
    savedPreferences.modSort,
    savedPreferences.modSortDescending,
    favouritesOnly,
    attentionOnly,
    problems,
    skipped,
    changedFiles,
  ]);

  const filtersOn =
    filterEnabled !== "all" ||
    Boolean(tagFilter) ||
    Boolean(kindFilter) ||
    favouritesOnly ||
    attentionOnly;

  /** Renames (or, onto an existing tag, merges) a tag on every mod. */
  const renameTag = async (from: string) => {
    const to = window.prompt(`Rename the tag "${from}" on every mod to:`, from);
    if (to === null || to.trim() === "" || to.trim() === from) return;
    try {
      const changed = await api.renameModTag(from, to.trim());
      setTagFilter(to.trim());
      setTagStatus(`Renamed "${from}" to "${to.trim()}" on ${changed} mod(s).`);
    } catch (renameError) {
      setError(errorSummary(renameError, "The tag was not renamed"));
    }
  };

  const toggleFavourite = async (mod: ModListItemDto) => {
    const current = annotationFor(annotations, mod);
    try {
      await api.setModAnnotation({
        unique_id: mod.unique_id,
        favourite: !current?.favourite,
        tags: current?.tags ?? [],
        note: current?.note ?? "",
      });
    } catch (favouriteError) {
      setError(errorSummary(favouriteError, "Could not update favourites"));
    }
  };

  // Shown inside the details panel: an error on the page behind it is out of
  // reach while the panel is open.
  const [detailsError, setDetailsError] = useState<string | null>(null);
  const handleOpenDetails = async (profileComponentId: string) => {
    setSelectedModId(profileComponentId);
    setModDetails(null);
    setDetailsError(null);
    setLoadingDetails(true);
    try {
      const details = await api.getModDetails(profileComponentId);
      setModDetails(details);
    } catch (e: unknown) {
      setDetailsError(errorSummary(e, "Failed to load mod details"));
    } finally {
      setLoadingDetails(false);
    }
  };

  // A note or tags typed in the details panel are only kept by their Save
  // button, so closing the panel over them asks first, and waits while
  // they are being saved.
  const [notesState, setNotesState] = useState<UnsavedState>("clean");
  const closeDetails = async () => {
    if (notesState === "saving") return;
    if (
      notesState === "dirty" &&
      !(await askToConfirm(
        "Close the details without saving the note, tags or source link you changed? What you typed is lost.",
      ))
    )
      return;
    setNotesState("clean");
    setSelectedModId(null);
  };

  const handleRemoveMod = async (mod: ModListItemDto) => {
    setIsRemoving(mod.profile_component_id);
    setError(null);
    setRemovalExtras(null);
    setRemovalDependents([]);
    setAlsoDisable(false);
    try {
      const preview = await api.prepareRemoval(mod.profile_component_id);
      setRemovalPreview(preview);
      if (profileId && mods) {
        const removedNames = preview.affected_profile_component_ids
          .map((id) => mods.find((m) => m.profile_component_id === id)?.name)
          .filter((name): name is string => Boolean(name));
        api
          .getDependencyMap(profileId)
          .then((map) =>
            setRemovalDependents(enabledDependents(removedNames, map, mods)),
          )
          .catch(() => setRemovalDependents([]));
      }
      // Files in the folder that the manager did not install go with it.
      if (profileId) {
        api
          .checkModFiles(profileId)
          .then((checks) => {
            const check = checks.find(
              (c) => c.deployment_id === mod.deployment_id,
            );
            setRemovalExtras(
              check
                ? [...check.added, ...check.modified].filter(
                    (file) => !/(^|\/)config\.json$/i.test(file),
                  )
                : null,
            );
          })
          .catch(() => setRemovalExtras(null));
      }
    } catch (error) {
      setError(errorSummary(error, "Failed to prepare the removal preview"));
    } finally {
      setIsRemoving(null);
    }
  };

  // Rows get one actions object for the page's lifetime, which calls the
  // current handlers, so ticking a box re-renders that row and not all of
  // them (hundreds of mods made every tick slow).
  const latest = useRef({
    requestToggle,
    handleCopyId,
    toggleFavourite,
    handleOpenDetails,
    handleRemoveMod,
    filteredMods,
  });
  latest.current = {
    requestToggle,
    handleCopyId,
    toggleFavourite,
    handleOpenDetails,
    handleRemoveMod,
    filteredMods,
  };
  const rowActions = useMemo<ModRowActions>(
    () => ({
      check: (id, index, range) => {
        const shown = latest.current.filteredMods;
        // By id, so a search typed in between cannot shift the range.
        const anchor = shown.findIndex(
          (m) => m.profile_component_id === selectionAnchor.current,
        );
        const ids =
          range && anchor >= 0
            ? shown
                .slice(Math.min(anchor, index), Math.max(anchor, index) + 1)
                .map((m) => m.profile_component_id)
            : [id];
        selectionAnchor.current = id;
        setChecked((previous) => {
          // A range takes the state the clicked row is changing to.
          const select = !previous.has(id);
          const next = new Set(previous);
          for (const each of ids) {
            if (select) next.add(each);
            else next.delete(each);
          }
          return next;
        });
      },
      favourite: (mod) => void latest.current.toggleFavourite(mod),
      copyId: (uniqueId) => void latest.current.handleCopyId(uniqueId),
      toggle: (mod) => void latest.current.requestToggle(mod),
      details: (id) => void latest.current.handleOpenDetails(id),
      remove: (mod) => void latest.current.handleRemoveMod(mod),
    }),
    [],
  );
  const modIds = useMemo(
    () => new Set((mods ?? []).map((m) => m.profile_component_id)),
    [mods],
  );
  const shownIds = filteredMods.map((m) => m.profile_component_id);
  const shownSet = new Set(shownIds);
  const shownChecked = shownIds.filter((id) => checked.has(id)).length;

  return (
    <div className="space-y-6">
      {removalPreview && (
        <Modal
          labelledBy="removal-title"
          onClose={
            execute.isPending
              ? undefined
              : () => {
                  api
                    .cancelActiveOperation(removalPreview.operation_id)
                    .then(() => setRemovalPreview(null))
                    .catch((cancelError) =>
                      setError(
                        errorSummary(
                          cancelError,
                          "Failed to cancel the removal",
                        ),
                      ),
                    );
                }
          }
          className="space-y-4"
        >
          <h2 id="removal-title" className="text-xl font-bold">
            Review mod removal
          </h2>
          <p>
            {removalPreview.detected_components.length > 0
              ? `Remove ${removalPreview.detected_components
                  .map((c) => c.name)
                  .join(", ")}?`
              : `Remove ${removalPreview.affected_profile_component_ids.length} mod(s)?`}{" "}
            <span className="text-xs text-[var(--fg-muted)] break-all">
              From {removalPreview.original_filename}.
            </span>
          </p>
          {removalPreview.affected_profile_component_ids.length > 1 && (
            <p>
              These mods came in the same archive and share one folder, so they
              are removed together:{" "}
              {removalPreview.affected_profile_component_ids
                .map(
                  (id) =>
                    mods?.find((m) => m.profile_component_id === id)?.name ??
                    id,
                )
                .join(", ")}
              .
            </p>
          )}
          <p className="text-xs text-[var(--fg-muted)]">
            The folder, with any settings the mod saved in it, moves to the
            manager's recovery area rather than being deleted at once. The
            archive stays stored, so the mod can be installed again from
            Activity; its settings are not brought back with it.
          </p>
          <p className="text-xs text-[var(--fg-muted)]">
            SMAPI is not affected. It lives in the game folder, and removing
            mods, even the last one, never removes it.
          </p>
          {removalExtras && removalExtras.length > 0 && (
            <p className="text-xs">
              The folder also has {removalExtras.length} file(s) the manager did
              not install or that changed since (
              {removalExtras.slice(0, 5).join(", ")}
              {removalExtras.length > 5 ? ", …" : ""}). They go with it.
            </p>
          )}
          {removalPreview.warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
          {removalDependents.length > 0 && (
            <label className="flex items-start gap-2 text-sm">
              <input
                type="checkbox"
                checked={alsoDisable}
                onChange={(event) => setAlsoDisable(event.target.checked)}
              />
              <span>
                Also turn off the {removalDependents.length} mod(s) that would
                stop loading ({removalDependents.map((m) => m.name).join(", ")}
                ). They stay installed and can be turned on again.
              </span>
            </label>
          )}
          {error && <p role="alert">{error}</p>}
          <div className="flex justify-end gap-3">
            <Button
              variant="secondary"
              disabled={execute.isPending}
              onClick={async () => {
                try {
                  await api.cancelActiveOperation(removalPreview.operation_id);
                  setRemovalPreview(null);
                } catch (error) {
                  setError(errorSummary(error, "Failed to cancel the removal"));
                }
              }}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              isLoading={execute.isPending}
              disabled={execute.isPending}
              onClick={async () => {
                try {
                  const removedNames =
                    removalPreview.affected_profile_component_ids
                      .map(
                        (id) =>
                          mods?.find((m) => m.profile_component_id === id)
                            ?.name,
                      )
                      .filter((name): name is string => Boolean(name));
                  await execute.mutateAsync(removalPreview.operation_id);
                  let note: string | null = null;
                  if (alsoDisable && removalDependents.length > 0) {
                    const result = await api.setModsEnabled(
                      removalDependents.map((m) => m.profile_component_id),
                      false,
                    );
                    note =
                      result.failed.length > 0
                        ? `These were not turned off: ${result.failed
                            .map((f) => `${f.name} (${f.message})`)
                            .join(", ")}`
                        : `Also turned off: ${result.changed.join(", ")}.`;
                  }
                  setRemovalResult({
                    operationId: removalPreview.operation_id,
                    names: removedNames,
                    note,
                  });
                  setRemovalPreview(null);
                  setSelectedModId(null);
                  setModDetails(null);
                } catch (error) {
                  setError(errorSummary(error, "Failed to remove the mod"));
                }
              }}
            >
              Remove mod
            </Button>
          </div>
        </Modal>
      )}

      {togglePlan && (
        <Modal
          labelledBy="toggle-title"
          onClose={() => setTogglePlan(null)}
          className="space-y-3"
        >
          <h2 id="toggle-title" className="text-xl font-bold">
            {togglePlan.enable ? "Enable" : "Disable"} {togglePlan.mod.name}?
          </h2>
          {togglePlan.impact.affected_mods.length > 1 && (
            <p className="text-sm">
              These mods share one package and change together:{" "}
              {togglePlan.impact.affected_mods.join(", ")}.
            </p>
          )}
          {togglePlan.impact.dependents.length > 0 && (
            <p className="text-sm text-[var(--warning)]">
              These enabled mods need it and will not load while it is disabled:{" "}
              {togglePlan.impact.dependents.join(", ")}.
            </p>
          )}
          {togglePlan.impact.unmet_requirements.map((line) => (
            <p key={line} className="text-sm text-[var(--warning)]">
              {line}.
            </p>
          ))}
          <div className="flex justify-end gap-3">
            <Button
              variant="secondary"
              disabled={toggling !== null}
              onClick={() => setTogglePlan(null)}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              isLoading={toggling === togglePlan.mod.profile_component_id}
              onClick={() => applyToggle(togglePlan.mod, togglePlan.enable)}
            >
              {togglePlan.enable ? "Enable" : "Disable"}
            </Button>
          </div>
        </Modal>
      )}

      {/* Header */}
      <div className="flex flex-col @2xl:flex-row @2xl:items-center justify-between gap-4">
        <div>
          <h2 className="text-xl font-bold tracking-tight">Installed Mods</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            {mods?.length ?? 0} mod(s) in active profile (
            {mods?.filter((m) => m.enabled).length ?? 0} enabled)
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => void handleExportInventory()}
          disabled={!overview || !mods}
          className="flex items-center gap-1.5 self-start @2xl:self-auto"
          title="Save a versioned JSON inventory with no local paths"
        >
          <Download className="w-3.5 h-3.5" />
          <span>Export inventory</span>
        </Button>
      </div>

      <div role="status" className="sr-only" aria-live="polite">
        {copyStatus}
      </div>

      <details className="text-xs text-[var(--fg-muted)] p-3 rounded-lg border border-[var(--border)]">
        <summary className="cursor-pointer font-medium text-[var(--fg-primary)]">
          {MOD_TRUST_SUMMARY}
        </summary>
        <p className="mt-2 leading-relaxed">{MOD_TRUST_DETAIL}</p>
      </details>

      {error && (
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm space-y-2">
          <p>{error}</p>
          {collision && (
            <Button
              size="sm"
              variant="secondary"
              isLoading={toggling === collision.mod.profile_component_id}
              onClick={() => void repairCollision()}
            >
              Move the other folder aside and{" "}
              {collision.enable ? "enable" : "disable"} {collision.mod.name}
            </Button>
          )}
        </div>
      )}

      {removalResult && (
        <div
          role="status"
          className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-surface)] text-sm space-y-2"
        >
          <p>
            Removed {removalResult.names.join(", ") || "the mod"}. Its folder,
            settings included, was moved to the profile's recovery folder, and
            the archive is still stored, so this can be undone.
          </p>
          {removalResult.note && <p>{removalResult.note}</p>}
          <div className="flex flex-wrap items-center gap-2">
            <UndoRemovalButton operationId={removalResult.operationId} />
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setRemovalResult(null)}
            >
              Dismiss
            </Button>
          </div>
        </div>
      )}

      {/* Drop Zone */}
      {profileId && (
        <ProfileModInstaller key={profileId} profileId={profileId} />
      )}

      <BulkToggleBar
        selectedIds={[...checked].filter((id) => modIds.has(id))}
        notShown={
          [...checked].filter((id) => modIds.has(id) && !shownSet.has(id))
            .length
        }
        onClear={() => setChecked(new Set())}
      />

      {/* Filter & Search Bar */}
      <div className="flex flex-col @3xl:flex-row items-stretch @3xl:items-center justify-between gap-3">
        <div className="relative flex-1 w-full">
          <Search className="w-4 h-4 text-[var(--fg-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            type="search"
            aria-label="Search mods"
            title="Every word must match the name, author or UniqueID, in any order"
            placeholder="Search name, author, UniqueID"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full pl-9 pr-3 py-2 bg-[var(--bg-surface)] border border-[var(--border)] rounded-lg text-sm text-[var(--fg-primary)] focus:border-[var(--accent-primary)] outline-none"
          />
        </div>

        <div className="flex items-center gap-1 bg-[var(--bg-surface)] border border-[var(--border)] rounded-lg p-1 text-xs">
          <button
            onClick={() => setFilterEnabled("all")}
            className={`px-3 py-1.5 rounded-md font-medium cursor-pointer transition-colors ${
              filterEnabled === "all"
                ? "bg-[var(--accent-primary)] text-[var(--accent-fg)]"
                : "text-[var(--fg-muted)] hover:text-[var(--fg-primary)]"
            }`}
          >
            All ({mods?.length ?? 0})
          </button>
          <button
            onClick={() => setFilterEnabled("enabled")}
            className={`px-3 py-1.5 rounded-md font-medium cursor-pointer transition-colors ${
              filterEnabled === "enabled"
                ? "bg-[var(--accent-primary)] text-[var(--accent-fg)]"
                : "text-[var(--fg-muted)] hover:text-[var(--fg-primary)]"
            }`}
          >
            Enabled ({mods?.filter((m) => m.enabled).length ?? 0})
          </button>
          <button
            onClick={() => setFilterEnabled("disabled")}
            className={`px-3 py-1.5 rounded-md font-medium cursor-pointer transition-colors ${
              filterEnabled === "disabled"
                ? "bg-[var(--accent-primary)] text-[var(--accent-fg)]"
                : "text-[var(--fg-muted)] hover:text-[var(--fg-primary)]"
            }`}
          >
            Disabled ({mods?.filter((m) => !m.enabled).length ?? 0})
          </button>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-3 text-xs">
        <label className="flex items-center gap-2">
          <span className="text-[var(--fg-muted)]">Sort by</span>
          <select
            value={savedPreferences.modSort}
            onChange={(event) => setSort(event.target.value as ModSort)}
            className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
          >
            {MOD_SORTS.map((sort) => (
              <option key={sort} value={sort}>
                {MOD_SORT_LABELS[sort]}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          onClick={() =>
            updatePreferences({
              modSortDescending: !savedPreferences.modSortDescending,
            })
          }
          aria-label={
            savedPreferences.modSortDescending
              ? "Sorted descending; sort ascending"
              : "Sorted ascending; sort descending"
          }
          className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] cursor-pointer"
        >
          {savedPreferences.modSortDescending ? "↓ Descending" : "↑ Ascending"}
        </button>
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            checked={favouritesOnly}
            onChange={(event) => setFavouritesOnly(event.target.checked)}
          />
          <span>Favourites only</span>
        </label>
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            checked={attentionOnly}
            onChange={(event) => setAttentionOnly(event.target.checked)}
          />
          <span>
            Needs attention (
            {
              new Set([
                ...problems.keys(),
                ...skipped.keys(),
                ...(mods ?? [])
                  .filter(
                    (m) =>
                      m.folder_missing || changedFiles.has(m.deployment_id),
                  )
                  .map((m) => m.profile_component_id),
              ]).size
            }
            )
          </span>
        </label>
        {tagOptions.length > 0 && (
          <label className="flex items-center gap-2">
            <span className="text-[var(--fg-muted)]">Tag</span>
            <select
              value={tagFilter}
              onChange={(event) => setTagFilter(event.target.value)}
              className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
            >
              <option value="">Any</option>
              <option value={UNTAGGED}>No tags</option>
              {tagOptions.map((tag) => (
                <option key={tag} value={tag}>
                  {tag}
                </option>
              ))}
            </select>
            {tagFilter && tagFilter !== UNTAGGED && (
              <button
                type="button"
                onClick={() => void renameTag(tagFilter)}
                className="text-[var(--accent-primary)] hover:underline cursor-pointer"
              >
                Rename tag
              </button>
            )}
          </label>
        )}
        <label className="flex items-center gap-2">
          <span className="text-[var(--fg-muted)]">Type</span>
          <select
            value={kindFilter}
            onChange={(event) => setKindFilter(event.target.value)}
            className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
          >
            <option value="">Any</option>
            {Object.entries(KIND_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {tagStatus && (
          <p role="status" className="text-xs">
            {tagStatus}
          </p>
        )}
      </div>

      {(() => {
        // Filters never hide a problem silently: mods that need attention
        // but are filtered out are counted, with a way to show them.
        const shown = new Set(filteredMods.map((m) => m.profile_component_id));
        const hidden = (mods ?? []).filter(
          (m) =>
            !shown.has(m.profile_component_id) &&
            (problems.has(m.profile_component_id) ||
              skipped.has(m.profile_component_id) ||
              m.folder_missing ||
              changedFiles.has(m.deployment_id)),
        ).length;
        return hidden > 0 && filteredMods.length > 0 ? (
          <p role="status" className="text-xs">
            {hidden} mod(s) that need attention are hidden by the current
            filters.{" "}
            <button
              type="button"
              className="underline cursor-pointer"
              onClick={() => {
                setSearch("");
                setFilterEnabled("all");
                setTagFilter("");
                setKindFilter("");
                setFavouritesOnly(false);
                setAttentionOnly(true);
              }}
            >
              Show them
            </button>
          </p>
        ) : null;
      })()}

      {/* Mod Inventory List */}
      {filteredMods.length > 0 ? (
        <Card className="p-0 overflow-hidden border border-[var(--border)]">
          <div className="px-4 py-2 border-b border-[var(--border)] text-xs">
            <label className="inline-flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={shownChecked > 0 && shownChecked === shownIds.length}
                ref={(box) => {
                  if (box)
                    box.indeterminate =
                      shownChecked > 0 && shownChecked < shownIds.length;
                }}
                onChange={() => {
                  // Only the rows shown: a search or filter narrows what
                  // "all" means, and rows it hides keep their ticks.
                  const selectAll = shownChecked < shownIds.length;
                  setChecked((previous) => {
                    const next = new Set(previous);
                    for (const id of shownIds) {
                      if (selectAll) next.add(id);
                      else next.delete(id);
                    }
                    return next;
                  });
                  selectionAnchor.current = null;
                }}
              />
              <span>
                {filteredMods.length === (mods?.length ?? 0)
                  ? `Select all ${shownIds.length}`
                  : `Select all ${shownIds.length} shown`}
              </span>
            </label>
          </div>
          <ul
            aria-label="Mods in this profile"
            onKeyDown={handleRowNavigation}
            className="divide-y divide-[var(--border)]"
          >
            {filteredMods.map((mod, index) => (
              <ModRow
                key={mod.profile_component_id}
                mod={mod}
                index={index}
                checked={checked.has(mod.profile_component_id)}
                annotation={annotationFor(annotations, mod)}
                problem={problems.get(mod.profile_component_id)}
                skipped={skipped.get(mod.profile_component_id)}
                changed={changedFiles.get(mod.deployment_id)}
                toggling={toggling === mod.profile_component_id}
                removing={isRemoving === mod.profile_component_id}
                actions={rowActions}
              />
            ))}
          </ul>
        </Card>
      ) : modsError && !mods ? (
        <LoadFailed
          what="this profile's mods"
          message={errorSummary(modsError)}
          onRetry={() => void refetchMods()}
        />
      ) : !mods ? (
        <Loading what="mods" />
      ) : mods.length === 0 ? (
        <EmptyState
          icon={<Package className="w-6 h-6" />}
          title="No mods in this profile yet"
          description="Mods you install go into this profile only. Choose a mod ZIP you downloaded; you will see what it contains before anything is installed. No account is needed."
          actions={
            <Button
              variant="primary"
              size="sm"
              onClick={() => document.getElementById("choose-mod-zip")?.click()}
            >
              Choose mod ZIP
            </Button>
          }
        />
      ) : (
        <EmptyState
          icon={<Search className="w-6 h-6" />}
          title={
            search
              ? `No mods match "${search}"${filtersOn ? " with these filters" : ""}`
              : filterEnabled === "enabled"
                ? "No enabled mods match"
                : filterEnabled === "disabled"
                  ? "No disabled mods match"
                  : tagFilter
                    ? `No mods are tagged "${tagFilter}"`
                    : attentionOnly && !favouritesOnly
                      ? "No mods need attention"
                      : "No favourites match"
          }
          description={(() => {
            // Name what is hiding them: with filters on, the search alone
            // may match plenty.
            const searchMatches = search
              ? mods.filter((m) =>
                  matchesEveryWord(search, [m.name, m.unique_id, m.author]),
                ).length
              : mods.length;
            const hiding = !search
              ? "the filters hide them"
              : !filtersOn || searchMatches === 0
                ? "the search hides them"
                : `${searchMatches} match the search, and the filters hide them`;
            return `${mods.length} mod(s) are in this profile; ${hiding}.`;
          })()}
          actions={
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                setSearch("");
                setFilterEnabled("all");
                setTagFilter("");
                setKindFilter("");
                setFavouritesOnly(false);
                setAttentionOnly(false);
              }}
            >
              Show all mods
            </Button>
          }
        />
      )}

      {/* Mod Details Drawer */}
      {/* A dialog, so Escape closes it, focus stays inside while it is open
          and goes back to the row's details button afterwards. */}
      {selectedModId && (
        <DetailsDrawer onClose={() => void closeDetails()}>
          <div className="w-full max-w-lg bg-[var(--bg-surface)] border-l border-[var(--border)] h-full shadow-2xl flex flex-col overflow-hidden animate-in slide-in-from-right duration-200">
            {/* Drawer Header */}
            <div className="p-5 border-b border-[var(--border)] flex items-center justify-between">
              <div className="min-w-0 flex-1">
                <RadixDialog.Title className="font-bold text-base truncate">
                  {modDetails?.name || "Mod Details"}
                </RadixDialog.Title>
                <p className="text-xs text-[var(--fg-muted)] font-mono truncate">
                  {modDetails?.unique_id}
                </p>
              </div>
              <button
                type="button"
                onClick={() => void closeDetails()}
                aria-label="Close details"
                className="p-2 hover:bg-[var(--bg-elevated)] rounded-lg text-[var(--fg-muted)] hover:text-[var(--fg-primary)] cursor-pointer"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            {/* Drawer Body */}
            <div className="flex-1 overflow-y-auto p-5 space-y-5">
              {loadingDetails ? (
                <div className="py-12 text-center">
                  <div className="w-6 h-6 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin mx-auto mb-2" />
                  <p className="text-xs text-[var(--fg-muted)]">
                    Loading manifest details...
                  </p>
                </div>
              ) : modDetails ? (
                <>
                  <div className="space-y-2 text-xs">
                    <div className="flex justify-between py-1 border-b border-[var(--border)]">
                      <span className="text-[var(--fg-muted)]">Author:</span>
                      <span className="font-semibold">{modDetails.author}</span>
                    </div>
                    <div className="flex justify-between py-1 border-b border-[var(--border)]">
                      <span className="text-[var(--fg-muted)]">Version:</span>
                      <span className="font-mono">{modDetails.version}</span>
                    </div>
                    {modDetails.entry_dll && (
                      <div className="flex justify-between py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          Entry DLL:
                        </span>
                        <span className="font-mono">
                          {modDetails.entry_dll}
                        </span>
                      </div>
                    )}
                    {modDetails.minimum_api_version && (
                      <div className="flex justify-between py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          Min SMAPI Version:
                        </span>
                        <span className="font-mono">
                          {modDetails.minimum_api_version}
                        </span>
                      </div>
                    )}
                    {modDetails.unique_id && (
                      <div className="flex justify-between gap-2 py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          UniqueID:
                        </span>
                        <span className="font-mono break-all text-right">
                          {modDetails.unique_id}
                        </span>
                      </div>
                    )}
                    {modDetails.minimum_game_version && (
                      <div className="flex justify-between gap-2 py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          Min game version:
                        </span>
                        <span className="font-mono break-all text-right">
                          {modDetails.minimum_game_version}
                        </span>
                      </div>
                    )}
                    {modDetails.content_pack_for && (
                      <div className="flex justify-between gap-2 py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          Content pack for:
                        </span>
                        <span className="font-mono break-all text-right">
                          {`${modDetails.content_pack_for?.unique_id}${modDetails.content_pack_for?.minimum_version ? ` ${modDetails.content_pack_for.minimum_version}+` : ""}`}
                        </span>
                      </div>
                    )}
                    {modDetails.update_keys.length > 0 && (
                      <div className="flex justify-between gap-2 py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          Update keys:
                        </span>
                        <span className="font-mono break-all text-right">
                          {modDetails.update_keys.join(", ")}
                        </span>
                      </div>
                    )}
                    <div className="flex justify-between gap-2 py-1 border-b border-[var(--border)]">
                      <span className="text-[var(--fg-muted)]">
                        Compatibility:
                      </span>
                      <span
                        className="text-right"
                        title="No compatibility list is connected, so whether this version works with your game is unknown. Only the minimum versions in its manifest are checked."
                      >
                        Not assessed (only manifest minimums are checked)
                      </span>
                    </div>
                    <div className="flex justify-between py-1 border-b border-[var(--border)]">
                      <span className="text-[var(--fg-muted)]">
                        Deployment Path:
                      </span>
                      <span className="font-mono truncate max-w-xs">
                        {modDetails.deployment_root_path}
                      </span>
                      <CopyButton
                        value={modDetails.deployment_root_path}
                        label="deployment path"
                      />
                    </div>
                    <div className="flex justify-between py-1 border-b border-[var(--border)]">
                      <span className="text-[var(--fg-muted)]">Source:</span>
                      <span className="text-right">
                        {modDetails.source ?? "Not recorded"}
                        {modDetails.original_filename
                          ? ` (${modDetails.original_filename})`
                          : ""}
                        {modDetails.acquired_at
                          ? `, added ${new Date(modDetails.acquired_at).toLocaleDateString()}`
                          : ""}
                      </span>
                    </div>
                    {modDetails.earlier_versions.length > 0 && (
                      <div className="flex justify-between py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">
                          Earlier versions:
                        </span>
                        <span className="text-right">
                          {modDetails.earlier_versions.join("; ")}
                        </span>
                      </div>
                    )}
                  </div>

                  {selectedModId && (
                    <>
                      <ModNotesPanel
                        profileComponentId={selectedModId}
                        uniqueId={modDetails.unique_id}
                        annotation={annotationFor(annotations, modDetails)}
                        artifactHash={modDetails.artifact_hash}
                        profileId={overview?.profile.id}
                        onUnsavedChange={setNotesState}
                      />
                      {savedPreferences.expertMode && (
                        <PackageFiles profileComponentId={selectedModId} />
                      )}
                      <ModRelationsPanel
                        profileComponentId={selectedModId}
                        profileId={overview?.profile.id}
                        uniqueId={modDetails.unique_id}
                        modName={modDetails.name}
                      />
                    </>
                  )}

                  {modDetails.description && (
                    <div className="space-y-1">
                      <h4 className="text-xs font-bold text-[var(--fg-muted)] uppercase tracking-wider">
                        Description
                      </h4>
                      <p className="text-xs text-[var(--fg-primary)] leading-relaxed">
                        {modDetails.description}
                      </p>
                    </div>
                  )}

                  {/* Raw Manifest */}
                  <div className="space-y-1">
                    <h4 className="text-xs font-bold text-[var(--fg-muted)] uppercase tracking-wider flex items-center gap-1.5">
                      <FileCode className="w-3.5 h-3.5" />
                      <span>Raw manifest.json</span>
                    </h4>
                    <pre className="p-3 rounded-lg bg-[var(--bg-primary)] border border-[var(--border)] text-[11px] font-mono text-[var(--fg-muted)] overflow-x-auto max-h-60">
                      {modDetails.raw_manifest}
                    </pre>
                    {(() => {
                      const unread = unreadManifestFields(
                        modDetails.raw_manifest,
                      );
                      if (unread === null)
                        return (
                          <p className="text-[11px] text-[var(--fg-muted)]">
                            This manifest uses comments or other relaxed JSON,
                            so its field list is not checked here.
                          </p>
                        );
                      return unread.length > 0 ? (
                        <p className="text-[11px] text-[var(--fg-muted)]">
                          Fields this manager does not read:{" "}
                          <span className="font-mono">{unread.join(", ")}</span>
                          . SMAPI or the mod may still use them.
                        </p>
                      ) : null;
                    })()}
                  </div>
                </>
              ) : detailsError ? (
                <p role="alert" className="text-sm text-[var(--danger)]">
                  {detailsError}
                </p>
              ) : null}
            </div>
          </div>
        </DetailsDrawer>
      )}
    </div>
  );
};

/** The mod details panel's dialog shell: a full-height panel on the right. */
const DetailsDrawer: React.FC<{
  onClose: () => void;
  children: React.ReactNode;
}> = ({ onClose, children }) => {
  const [opener] = useState(focusedElement);
  return (
    <RadixDialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-50 bg-black/40 backdrop-blur-xs" />
        <RadixDialog.Content
          aria-describedby={undefined}
          onCloseAutoFocus={(event) => returnFocus(opener, event)}
          className="fixed inset-y-0 right-0 z-50 flex w-full max-w-lg justify-end focus:outline-none"
        >
          {children}
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
};
