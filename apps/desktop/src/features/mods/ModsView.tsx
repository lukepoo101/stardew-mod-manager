import React, { useState, useMemo } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useActiveProfileOverview,
  useProfileMods,
  useToggleMod,
} from "@/shared/api/hooks";
import { ModListItemDto, ModDetailsDto } from "@/shared/api/generated";
import { api, backend } from "@/shared/api/client";
import { ModDropZone } from "./ModDropZone";
import { ModReviewDialog } from "./ModReviewDialog";
import { ArchiveInspectionResult } from "@/lib/backend/types";
import {
  Search,
  Package,
  Trash2,
  Info,
  X,
  FileCode,
} from "lucide-react";

export const ModsView: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: mods, refetch: refetchMods } = useProfileMods(profileId);
  const toggleMutation = useToggleMod();

  const [search, setSearch] = useState("");
  const [filterEnabled, setFilterEnabled] = useState<"all" | "enabled" | "disabled">("all");
  const [selectedModId, setSelectedModId] = useState<string | null>(null);
  const [modDetails, setModDetails] = useState<ModDetailsDto | null>(null);
  const [loadingDetails, setLoadingDetails] = useState(false);
  const [activeInspection, setActiveInspection] = useState<ArchiveInspectionResult | null>(null);
  const [isRemoving, setIsRemoving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const filteredMods = useMemo(() => {
    if (!mods) return [];
    return mods.filter((m) => {
      const matchesSearch =
        m.name.toLowerCase().includes(search.toLowerCase()) ||
        m.unique_id.toLowerCase().includes(search.toLowerCase()) ||
        m.author.toLowerCase().includes(search.toLowerCase());

      if (!matchesSearch) return false;
      if (filterEnabled === "enabled") return m.enabled;
      if (filterEnabled === "disabled") return !m.enabled;
      return true;
    });
  }, [mods, search, filterEnabled]);

  const handleToggle = async (profileComponentId: string, current: boolean) => {
    try {
      await toggleMutation.mutateAsync({
        profileComponentId,
        enabled: !current,
      });
      refetchMods();
    } catch (e: any) {
      setError(e?.message || "Failed to toggle mod");
    }
  };

  const handleOpenDetails = async (profileComponentId: string) => {
    setSelectedModId(profileComponentId);
    setLoadingDetails(true);
    try {
      const details = await api.getModDetails(profileComponentId);
      setModDetails(details);
    } catch (e: any) {
      setError(e?.message || "Failed to load mod details");
    } finally {
      setLoadingDetails(false);
    }
  };

  const handleRemoveMod = async (mod: ModListItemDto) => {
    if (!window.confirm(`Are you sure you want to remove "${mod.name}"?`)) return;
    setIsRemoving(mod.profile_component_id);
    setError(null);
    try {
      if (profileId) {
        await backend.removeMod(mod.profile_component_id, profileId);
      }
      if (selectedModId === mod.profile_component_id) {
        setSelectedModId(null);
        setModDetails(null);
      }
      refetchMods();
    } catch (e: any) {
      setError(e?.message || "Failed to remove mod");
    } finally {
      setIsRemoving(null);
    }
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 className="text-xl font-bold tracking-tight">Installed Mods</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            {mods?.length ?? 0} mod(s) in active profile ({mods?.filter((m) => m.enabled).length ?? 0} enabled)
          </p>
        </div>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm">
          {error}
        </div>
      )}

      {/* Drop Zone */}
      {profileId && (
        <ModDropZone
          setupId={profileId}
          onInspectionReady={(res) => setActiveInspection(res)}
        />
      )}

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
              {/* Left: Info */}
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 flex-wrap">
                  <h4 className="font-bold text-sm text-[var(--fg-primary)] truncate">
                    {mod.name}
                  </h4>
                  <span className="text-xs px-2 py-0.5 rounded bg-[var(--bg-elevated)] border border-[var(--border)] font-mono text-[var(--fg-muted)]">
                    v{mod.version}
                  </span>
                  <span className="text-xs text-[var(--fg-muted)]">by {mod.author}</span>
                  {mod.enabled ? (
                    <StatusBadge variant="success">Enabled</StatusBadge>
                  ) : (
                    <StatusBadge variant="neutral">Disabled</StatusBadge>
                  )}
                </div>
                <p className="text-xs font-mono text-[var(--fg-muted)] mt-1 truncate">
                  {mod.unique_id}
                </p>
                {mod.description && (
                  <p className="text-xs text-[var(--fg-muted)] mt-1 line-clamp-1">
                    {mod.description}
                  </p>
                )}
              </div>

              {/* Right: Actions */}
              <div className="flex items-center gap-2 shrink-0">
                <Button
                  variant={mod.enabled ? "ghost" : "secondary"}
                  size="sm"
                  onClick={() => handleToggle(mod.profile_component_id, mod.enabled)}
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
      ) : (
        <Card className="text-center py-12 space-y-3">
          <div className="w-12 h-12 rounded-full bg-[var(--bg-elevated)] text-[var(--fg-muted)] flex items-center justify-center mx-auto">
            <Package className="w-6 h-6" />
          </div>
          <p className="text-sm font-semibold text-[var(--fg-primary)]">
            {search ? "No mods match your search" : "No user mods installed yet"}
          </p>
          <p className="text-xs text-[var(--fg-muted)] max-w-sm mx-auto">
            {search
              ? "Try clearing your search query or filter"
              : "Drag and drop a mod ZIP above to install your first mod"}
          </p>
        </Card>
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
                  <p className="text-xs text-[var(--fg-muted)]">Loading manifest details...</p>
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
                        <span className="text-[var(--fg-muted)]">Entry DLL:</span>
                        <span className="font-mono">{modDetails.entry_dll}</span>
                      </div>
                    )}
                    {modDetails.minimum_api_version && (
                      <div className="flex justify-between py-1 border-b border-[var(--border)]">
                        <span className="text-[var(--fg-muted)]">Min SMAPI Version:</span>
                        <span className="font-mono">{modDetails.minimum_api_version}</span>
                      </div>
                    )}
                    <div className="flex justify-between py-1 border-b border-[var(--border)]">
                      <span className="text-[var(--fg-muted)]">Deployment Path:</span>
                      <span className="font-mono truncate max-w-xs">{modDetails.deployment_root_path}</span>
                    </div>
                  </div>

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

                  {/* Dependencies */}
                  <div className="space-y-2">
                    <h4 className="text-xs font-bold text-[var(--fg-muted)] uppercase tracking-wider">
                      Dependencies ({modDetails.dependencies.length})
                    </h4>
                    {modDetails.dependencies.length > 0 ? (
                      <div className="space-y-1">
                        {modDetails.dependencies.map((dep, i) => (
                          <div
                            key={i}
                            className="p-2 rounded bg-[var(--bg-elevated)] border border-[var(--border)] text-xs flex justify-between items-center"
                          >
                            <span className="font-mono">{dep.unique_id}</span>
                            <span className="text-[var(--fg-muted)] text-[10px]">
                              {dep.is_required ? "Required" : "Optional"}
                            </span>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <p className="text-xs text-[var(--fg-muted)]">No external dependencies required.</p>
                    )}
                  </div>

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

      <ModReviewDialog
        inspection={activeInspection}
        onClose={() => setActiveInspection(null)}
        onModInstalled={() => refetchMods()}
      />
    </div>
  );
};
