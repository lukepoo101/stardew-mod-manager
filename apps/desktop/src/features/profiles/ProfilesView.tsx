import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useProfiles,
  useActivateProfile,
  useCreateProfile,
  useArchiveProfile,
  useActiveProfileOverview,
} from "@/shared/api/hooks";
import { Layers, Plus, Archive, Check } from "lucide-react";

export const ProfilesView: React.FC = () => {
  const { data: profiles, refetch } = useProfiles();
  const { data: overview } = useActiveProfileOverview();

  const activateMutation = useActivateProfile();
  const createMutation = useCreateProfile();
  const archiveMutation = useArchiveProfile();

  const [isCreating, setIsCreating] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const activeProfileId = overview?.profile.id;

  const handleActivate = async (profileId: string) => {
    try {
      await activateMutation.mutateAsync(profileId);
      refetch();
    } catch (e: any) {
      setError(e?.message || "Failed to activate profile");
    }
  };

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newProfileName.trim() || !overview?.game.id) return;
    try {
      await createMutation.mutateAsync({
        name: newProfileName.trim(),
        gameInstallationId: overview.game.id,
      });
      setNewProfileName("");
      setIsCreating(false);
      refetch();
    } catch (e: any) {
      setError(e?.message || "Failed to create profile");
    }
  };

  const handleArchive = async (profileId: string) => {
    if (
      !window.confirm(
        "Archive this profile? Its mods stay on disk and it can be restored later."
      )
    )
      return;
    try {
      await archiveMutation.mutateAsync(profileId);
      refetch();
    } catch (e: any) {
      setError(e?.message || "Failed to archive profile");
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-bold tracking-tight">Profiles</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            Manage isolated setups with independent mods, revisions, and configs.
          </p>
        </div>
        <Button
          variant="primary"
          onClick={() => setIsCreating(true)}
          className="flex items-center gap-1.5"
        >
          <Plus className="w-4 h-4" />
          <span>New Profile</span>
        </Button>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm">
          {error}
        </div>
      )}

      {/* Create Modal Form */}
      {isCreating && (
        <Card className="p-5 border-2 border-[var(--accent-primary)]/40 bg-[var(--bg-elevated)]/20 space-y-4">
          <h3 className="text-sm font-bold">Create New Profile</h3>
          <form onSubmit={handleCreate} className="flex gap-2">
            <input
              type="text"
              placeholder="Profile name (e.g. Multiplayer, SVE, Vanilla+)"
              value={newProfileName}
              onChange={(e) => setNewProfileName(e.target.value)}
              className="flex-1 px-3 py-2 bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg text-sm text-[var(--fg-primary)] focus:border-[var(--accent-primary)] outline-none"
              autoFocus
            />
            <Button
              variant="primary"
              type="submit"
              disabled={!newProfileName.trim() || createMutation.isPending}
              isLoading={createMutation.isPending}
            >
              Create
            </Button>
            <Button
              variant="ghost"
              type="button"
              onClick={() => {
                setIsCreating(false);
                setNewProfileName("");
              }}
            >
              Cancel
            </Button>
          </form>
        </Card>
      )}

      {/* Profile Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {profiles?.map((profile) => {
          const isActive = profile.id === activeProfileId;
          return (
            <Card
              key={profile.id}
              className={`p-5 space-y-4 transition-all ${
                isActive
                  ? "border-2 border-[var(--accent-primary)] shadow-sm bg-[var(--accent-primary)]/[0.02]"
                  : "border border-[var(--border)] hover:border-[var(--border-focus)]"
              }`}
            >
              <div className="flex items-start justify-between">
                <div className="space-y-1">
                  <div className="flex items-center gap-2">
                    <h3 className="font-bold text-base text-[var(--fg-primary)]">
                      {profile.name}
                    </h3>
                    {isActive && <StatusBadge variant="success">Active</StatusBadge>}
                  </div>
                  <p className="text-xs text-[var(--fg-muted)] font-mono">
                    Revision {profile.revision.toString()} • {profile.mod_count} mod(s)
                  </p>
                </div>
                <div className="p-2 rounded-lg bg-[var(--bg-elevated)]">
                  <Layers className="w-4 h-4 text-[var(--accent-primary)]" />
                </div>
              </div>

              <div className="flex items-center justify-between pt-2 border-t border-[var(--border)] text-xs text-[var(--fg-muted)]">
                <span>Created {new Date(profile.created_at).toLocaleDateString()}</span>
                <div className="flex items-center gap-2">
                  {!isActive && (
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => handleActivate(profile.id)}
                      isLoading={activateMutation.isPending}
                      className="flex items-center gap-1"
                    >
                      <Check className="w-3.5 h-3.5" />
                      <span>Activate</span>
                    </Button>
                  )}
                  {!isActive && (
                    <button
                      onClick={() => handleArchive(profile.id)}
                      className="p-1.5 hover:bg-[var(--bg-elevated)] rounded text-[var(--fg-muted)] hover:text-[var(--fg-primary)] cursor-pointer"
                      title="Archive Profile"
                    >
                      <Archive className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              </div>
            </Card>
          );
        })}
      </div>
    </div>
  );
};
