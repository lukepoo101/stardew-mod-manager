import { matchSkipped, skippedByComponent } from "@/shared/diagnostics/skipped";
import { Modal } from "@/components/ui/Modal";
import { CopyButton } from "@/components/ui/CopyButton";
import { usePreferences, type ModFilter } from "@/shared/preferences";
import React, { useState, useMemo } from "react";
import { Card } from "@/components/ui/Card";
import { EmptyState, LoadFailed, Loading } from "@/components/ui/EmptyState";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useActiveProfileOverview,
  useProfileMods,
  useExecuteOperation,
  useModAnnotations,
  useDiagnosticsReport,
  useModProblems,
} from "@/shared/api/hooks";
import {
  ModListItemDto,
  ModDetailsDto,
  ToggleImpactDto,
  OperationPreviewDto,
} from "@/shared/api/generated";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { ProfileModInstaller } from "./ProfileModInstaller";
import { ModNotesPanel } from "./ModNotesPanel";
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
import { ModRelationsPanel } from "./ModRelationsPanel";
import { copyText, downloadText } from "@/shared/support/actions";
import { buildInventory, serializeInventory } from "@/shared/support/inventory";
import { MOD_TRUST_DETAIL, MOD_TRUST_SUMMARY } from "@/shared/security/trust";
import {
  Star,
  Search,
  Package,
  Trash2,
  Info,
  X,
  FileCode,
  Copy,
  Download,
} from "lucide-react";

export const ModsView: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const {
    data: mods,
    error: modsError,
    refetch: refetchMods,
  } = useProfileMods(profileId);
  const execute = useExecuteOperation();
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
  const { data: report } = useDiagnosticsReport(overview?.game.id);
  const skipped = useMemo(
    () =>
      skippedByComponent(
        matchSkipped(report?.log_summary.skipped_mods ?? [], mods ?? []),
      ),
    [report, mods],
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
  const toggleChecked = (id: string) => {
    const next = new Set(checked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setChecked(next);
  };
  const [modDetails, setModDetails] = useState<ModDetailsDto | null>(null);
  const [loadingDetails, setLoadingDetails] = useState(false);
  const [isRemoving, setIsRemoving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState<string | null>(null);
  const [togglePlan, setTogglePlan] = useState<{
    mod: ModListItemDto;
    enable: boolean;
    impact: ToggleImpactDto;
  } | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);

  const applyToggle = async (mod: ModListItemDto, enable: boolean) => {
    setToggling(mod.profile_component_id);
    setError(null);
    try {
      await api.setModEnabled(mod.profile_component_id, enable);
      setTogglePlan(null);
      setCopyStatus(`${mod.name} ${enable ? "enabled" : "disabled"}`);
    } catch (toggleError) {
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

  const handleExportInventory = () => {
    if (!overview || !mods) return;
    downloadText(
      "profile-inventory.json",
      serializeInventory(
        buildInventory(overview, mods, {
          generatedAt: new Date().toISOString(),
        }),
      ),
    );
  };

  const filteredMods = useMemo(() => {
    if (!mods) return [];
    const matching = mods.filter((m) => {
      const matchesSearch =
        m.name.toLowerCase().includes(search.toLowerCase()) ||
        m.unique_id.toLowerCase().includes(search.toLowerCase()) ||
        m.author.toLowerCase().includes(search.toLowerCase());

      if (!matchesSearch) return false;
      if (filterEnabled === "enabled" && !m.enabled) return false;
      if (filterEnabled === "disabled" && m.enabled) return false;
      if (tagFilter && !hasTag(annotations, m, tagFilter)) return false;
      if (favouritesOnly && !annotationFor(annotations, m)?.favourite)
        return false;
      if (
        attentionOnly &&
        !problems.has(m.profile_component_id) &&
        !skipped.has(m.profile_component_id)
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
    annotations,
    savedPreferences.modSort,
    savedPreferences.modSortDescending,
    favouritesOnly,
    attentionOnly,
    problems,
    skipped,
  ]);

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

  const handleOpenDetails = async (profileComponentId: string) => {
    setSelectedModId(profileComponentId);
    setLoadingDetails(true);
    try {
      const details = await api.getModDetails(profileComponentId);
      setModDetails(details);
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to load mod details"));
    } finally {
      setLoadingDetails(false);
    }
  };

  const handleRemoveMod = async (mod: ModListItemDto) => {
    setIsRemoving(mod.profile_component_id);
    setError(null);
    try {
      setRemovalPreview(await api.prepareRemoval(mod.profile_component_id));
    } catch (error) {
      setError(errorSummary(error, "Failed to prepare the removal preview"));
    } finally {
      setIsRemoving(null);
    }
  };

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
            Remove {removalPreview.affected_profile_component_ids.length} mod
            component(s) from {removalPreview.original_filename}?
          </p>
          {removalPreview.warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
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
                  await execute.mutateAsync(removalPreview.operation_id);
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
            <Button variant="secondary" onClick={() => setTogglePlan(null)}>
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
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
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
          onClick={handleExportInventory}
          disabled={!overview || !mods}
          className="flex items-center gap-1.5"
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
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm">
          {error}
        </div>
      )}

      {/* Drop Zone */}
      {profileId && (
        <ProfileModInstaller key={profileId} profileId={profileId} />
      )}

      <BulkToggleBar
        selectedIds={[...checked].filter((id) =>
          mods?.some((m) => m.profile_component_id === id),
        )}
        onClear={() => setChecked(new Set())}
      />

      {/* Filter & Search Bar */}
      <div className="flex flex-col sm:flex-row items-center justify-between gap-3">
        <div className="relative flex-1 w-full">
          <Search className="w-4 h-4 text-[var(--fg-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
          <input
            type="text"
            placeholder="Search mods by name, author, or unique ID..."
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
                ? "bg-[var(--accent-primary)] text-white"
                : "text-[var(--fg-muted)] hover:text-[var(--fg-primary)]"
            }`}
          >
            All ({mods?.length ?? 0})
          </button>
          <button
            onClick={() => setFilterEnabled("enabled")}
            className={`px-3 py-1.5 rounded-md font-medium cursor-pointer transition-colors ${
              filterEnabled === "enabled"
                ? "bg-[var(--accent-primary)] text-white"
                : "text-[var(--fg-muted)] hover:text-[var(--fg-primary)]"
            }`}
          >
            Enabled ({mods?.filter((m) => m.enabled).length ?? 0})
          </button>
          <button
            onClick={() => setFilterEnabled("disabled")}
            className={`px-3 py-1.5 rounded-md font-medium cursor-pointer transition-colors ${
              filterEnabled === "disabled"
                ? "bg-[var(--accent-primary)] text-white"
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
            {new Set([...problems.keys(), ...skipped.keys()]).size})
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
              {tagOptions.map((tag) => (
                <option key={tag} value={tag}>
                  {tag}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>

      {/* Mod Inventory List */}
      {filteredMods.length > 0 ? (
        <Card className="p-0 overflow-hidden border border-[var(--border)] divide-y divide-[var(--border)]">
          {filteredMods.map((mod) => (
            <div
              key={mod.profile_component_id}
              className={`p-4 flex items-center justify-between gap-4 hover:bg-[var(--bg-elevated)]/40 transition-colors ${
                !mod.enabled ? "opacity-60 bg-[var(--bg-elevated)]/10" : ""
              }`}
            >
              <input
                type="checkbox"
                aria-label={`Select ${mod.name}`}
                checked={checked.has(mod.profile_component_id)}
                onChange={() => toggleChecked(mod.profile_component_id)}
                className="shrink-0"
              />
              {/* Left: Info */}
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 flex-wrap">
                  {mod.unique_id && (
                    <button
                      type="button"
                      onClick={() => toggleFavourite(mod)}
                      aria-pressed={Boolean(
                        annotationFor(annotations, mod)?.favourite,
                      )}
                      aria-label={`Favourite ${mod.name}`}
                      title="Favourite"
                      className="cursor-pointer text-[var(--fg-muted)] hover:text-amber-500"
                    >
                      <Star
                        className={`w-4 h-4 ${
                          annotationFor(annotations, mod)?.favourite
                            ? "fill-amber-400 text-amber-500"
                            : ""
                        }`}
                      />
                    </button>
                  )}
                  <h4 className="font-bold text-sm text-[var(--fg-primary)] truncate">
                    {mod.name}
                  </h4>
                  <span className="text-xs px-2 py-0.5 rounded bg-[var(--bg-elevated)] border border-[var(--border)] font-mono text-[var(--fg-muted)]">
                    v{mod.version}
                  </span>
                  <span className="text-xs text-[var(--fg-muted)]">
                    by {mod.author}
                  </span>
                  {mod.enabled ? (
                    <StatusBadge variant="success">Enabled</StatusBadge>
                  ) : (
                    <StatusBadge variant="neutral">Disabled</StatusBadge>
                  )}
                  {problems.has(mod.profile_component_id) && (
                    <span
                      title={`Needs ${problems.get(mod.profile_component_id)?.join(", ")}`}
                    >
                      <StatusBadge variant="danger">
                        Missing requirement
                      </StatusBadge>
                    </span>
                  )}
                  {skipped.has(mod.profile_component_id) && (
                    <span
                      title={`${skipped.get(mod.profile_component_id)?.skipped.reason ?? ""} (SMAPI log line ${skipped.get(mod.profile_component_id)?.skipped.line})`}
                    >
                      <StatusBadge variant="warning">
                        {skipped.get(mod.profile_component_id)?.kind === "exact"
                          ? "Skipped last session"
                          : "Possibly skipped last session"}
                      </StatusBadge>
                    </span>
                  )}
                </div>
                <p className="text-xs font-mono text-[var(--fg-muted)] mt-1 truncate">
                  {mod.unique_id || "UniqueID unavailable (manifest invalid)"}
                </p>
                {mod.description && (
                  <p className="text-xs text-[var(--fg-muted)] mt-1 line-clamp-1">
                    {mod.description}
                  </p>
                )}
                {(annotationFor(annotations, mod)?.tags.length ?? 0) > 0 && (
                  <ul className="flex flex-wrap gap-1 mt-1" aria-label="Tags">
                    {annotationFor(annotations, mod)?.tags.map((tag) => (
                      <li
                        key={tag}
                        className="text-[10px] px-1.5 py-0.5 rounded-full bg-[var(--bg-elevated)] border border-[var(--border)]"
                      >
                        {tag}
                      </li>
                    ))}
                  </ul>
                )}
              </div>

              {/* Right: Actions */}
              <div className="flex items-center gap-2 shrink-0">
                {mod.unique_id && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleCopyId(mod.unique_id)}
                    title="Copy UniqueID"
                    aria-label={`Copy UniqueID ${mod.unique_id}`}
                  >
                    <Copy className="w-4 h-4" />
                  </Button>
                )}
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={toggling === mod.profile_component_id}
                  onClick={() => requestToggle(mod)}
                  aria-label={`${mod.enabled ? "Disable" : "Enable"} ${mod.name}`}
                  title={
                    mod.enabled
                      ? "Stop SMAPI from loading this mod"
                      : "Let SMAPI load this mod again"
                  }
                >
                  {mod.enabled ? "Disable" : "Enable"}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => handleOpenDetails(mod.profile_component_id)}
                  title="View manifest details"
                >
                  <Info className="w-4 h-4" />
                </Button>

                <button
                  onClick={() => handleRemoveMod(mod)}
                  disabled={isRemoving === mod.profile_component_id}
                  className="p-2 hover:bg-[var(--danger-surface)] rounded text-[var(--fg-muted)] hover:text-[var(--danger)] cursor-pointer transition-colors"
                  title="Remove mod"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>
          ))}
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
              ? `No mods match "${search}"`
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
          description={`${mods.length} mod(s) are in this profile; the ${search ? "search" : "filter"} hides them.`}
          actions={
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                setSearch("");
                setFilterEnabled("all");
                setTagFilter("");
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
      {selectedModId && (
        <div className="fixed inset-0 z-50 flex justify-end bg-black/40 backdrop-blur-xs">
          <div className="w-full max-w-lg bg-[var(--bg-surface)] border-l border-[var(--border)] h-full shadow-2xl flex flex-col overflow-hidden animate-in slide-in-from-right duration-200">
            {/* Drawer Header */}
            <div className="p-5 border-b border-[var(--border)] flex items-center justify-between">
              <div className="min-w-0 flex-1">
                <h3 className="font-bold text-base truncate">
                  {modDetails?.name || "Mod Details"}
                </h3>
                <p className="text-xs text-[var(--fg-muted)] font-mono truncate">
                  {modDetails?.unique_id}
                </p>
              </div>
              <button
                onClick={() => setSelectedModId(null)}
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
                  </div>

                  {selectedModId && (
                    <>
                      <ModNotesPanel
                        profileComponentId={selectedModId}
                        uniqueId={modDetails.unique_id}
                        annotation={annotationFor(annotations, modDetails)}
                      />
                      <ModRelationsPanel profileComponentId={selectedModId} />
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
                  </div>
                </>
              ) : null}
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
