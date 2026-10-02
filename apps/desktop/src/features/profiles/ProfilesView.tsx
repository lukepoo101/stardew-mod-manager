import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useProfiles,
  useActivateProfile,
  useCreateProfile,
  useArchiveProfile,
  useArchivedProfiles,
  useRestoreProfile,
  useSetDefaultProfile,
  useActiveProfileOverview,
  useSaves,
} from "@/shared/api/hooks";
import { errorSummary } from "@/shared/api/errors";
import type { ProfileSummaryDto } from "@/shared/api/generated";
import { BundleCard } from "./BundleCard";
import { CuratorCard } from "./CuratorCard";
import { CollectionCard } from "./CollectionCard";
import { RecipeCard } from "./RecipeCard";
import { ReferenceCard } from "./ReferenceCard";
import { ProfileCompareCard } from "./ProfileCompareCard";
import { ProfileDetailsForm } from "./ProfileDetailsForm";
import {
  Layers,
  Plus,
  Archive,
  ArchiveRestore,
  Check,
  Pencil,
  Copy,
  Star,
} from "lucide-react";
import { DeleteProfileDialog } from "./DeleteProfileDialog";
import { FreezeCard } from "./FreezeCard";
import { ExperimentCard } from "./ExperimentCard";
import { KnownGoodCard } from "./KnownGoodCard";
import { RestorePointsCard } from "./RestorePointsCard";
import { SavesCard } from "@/features/saves/SavesCard";
import { CloneProfileDialog } from "./CloneProfileDialog";
import { UnfinishedCopies } from "./UnfinishedCopies";
import { RecentlyDeleted } from "./RecentlyDeleted";
import { handleRowNavigation } from "@/shared/a11y/rowNavigation";

export const ProfilesView: React.FC = () => {
  const { data: profiles, refetch } = useProfiles();
  const { data: overview } = useActiveProfileOverview();
  const { data: saves } = useSaves();
  /** The farms linked to a profile, with links to missing saves marked. */
  const linkedSaves = (profileId: string): string[] => [
    ...(saves?.saves ?? [])
      .filter((save) => save.profile_id === profileId)
      .map((save) => save.farm_name ?? save.id),
    ...(saves?.unavailable_links ?? [])
      .filter((link) => link.profile_id === profileId)
      .map((link) => `${link.save_id} (not found)`),
  ];

  const activateMutation = useActivateProfile();
  const createMutation = useCreateProfile();
  const archiveMutation = useArchiveProfile();
  const restoreMutation = useRestoreProfile();
  const defaultMutation = useSetDefaultProfile();
  const { data: archivedProfiles, refetch: refetchArchived } =
    useArchivedProfiles();

  const [isCreating, setIsCreating] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [cloning, setCloning] = useState<ProfileSummaryDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  const activeProfileId = overview?.profile.id;

  const handleActivate = async (profileId: string) => {
    try {
      await activateMutation.mutateAsync(profileId);
      refetch();
    } catch (e: unknown) {
      setError(
        `${errorSummary(e, "The profile was not switched")} "${
          overview?.profile.name ?? "The current profile"
        }" is still active.`,
      );
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
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to create profile"));
    }
  };

  const handleArchive = async (profileId: string) => {
    if (
      !window.confirm(
        "Archive this profile? Its mods stay on disk and it can be restored later.",
      )
    )
      return;
    try {
      await archiveMutation.mutateAsync(profileId);
      refetch();
      refetchArchived();
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to archive profile"));
    }
  };

  const handleToggleDefault = async (profile: ProfileSummaryDto) => {
    try {
      await defaultMutation.mutateAsync(profile.is_default ? null : profile.id);
      refetch();
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to change the default profile"));
    }
  };

  const handleRestore = async (profileId: string) => {
    try {
      await restoreMutation.mutateAsync(profileId);
      refetch();
      refetchArchived();
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to restore profile"));
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-bold tracking-tight">Profiles</h2>
          <p className="text-sm text-[var(--fg-muted)]">
            Manage isolated setups with independent mods, revisions, and
            configs.
          </p>
          <p className="text-xs text-[var(--fg-muted)]">
            The active profile is the one you are using now. The default profile
            is only picked when no profile is active, for example after adding
            the game again; it never switches you away from your choice.
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

      <UnfinishedCopies />

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
      <ul
        aria-label="Profiles"
        onKeyDown={handleRowNavigation}
        className="grid grid-cols-1 md:grid-cols-2 gap-4"
      >
        {profiles?.map((profile) => {
          const isActive = profile.id === activeProfileId;
          return (
            <li key={profile.id} aria-label={profile.name} data-row>
              <Card
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
                      {isActive && (
                        <StatusBadge variant="success">Active</StatusBadge>
                      )}
                      {profile.is_default && (
                        <StatusBadge variant="info">Default</StatusBadge>
                      )}
                    </div>
                    <p className="text-xs text-[var(--fg-muted)] font-mono">
                      Revision {profile.revision.toString()} •{" "}
                      {profile.mod_count} mod(s)
                    </p>
                    {linkedSaves(profile.id).length > 0 && (
                      <p className="text-xs text-[var(--fg-muted)]">
                        Saves: {linkedSaves(profile.id).join(", ")}
                      </p>
                    )}
                    {profile.description && (
                      <p className="text-xs text-[var(--fg-muted)] whitespace-pre-line">
                        {profile.description}
                      </p>
                    )}
                  </div>
                  <div className="p-2 rounded-lg bg-[var(--bg-elevated)]">
                    <Layers className="w-4 h-4 text-[var(--accent-primary)]" />
                  </div>
                </div>

                {editingId === profile.id && (
                  <ProfileDetailsForm
                    profile={profile}
                    onDone={() => setEditingId(null)}
                  />
                )}

                <div className="flex items-center justify-between pt-2 border-t border-[var(--border)] text-xs text-[var(--fg-muted)]">
                  <span>
                    Created {new Date(profile.created_at).toLocaleDateString()}
                  </span>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => handleToggleDefault(profile)}
                      disabled={defaultMutation.isPending}
                      aria-pressed={profile.is_default}
                      className="p-1.5 hover:bg-[var(--bg-elevated)] rounded text-[var(--fg-muted)] hover:text-[var(--fg-primary)] cursor-pointer"
                      title={
                        profile.is_default
                          ? "Stop using this as the default profile"
                          : "Make this the default profile"
                      }
                      aria-label={
                        profile.is_default
                          ? `Clear ${profile.name} as default`
                          : `Make ${profile.name} the default`
                      }
                    >
                      <Star
                        className={`w-3.5 h-3.5 ${profile.is_default ? "fill-current text-[var(--accent-primary)]" : ""}`}
                      />
                    </button>
                    <button
                      type="button"
                      onClick={() => setCloning(profile)}
                      className="p-1.5 hover:bg-[var(--bg-elevated)] rounded text-[var(--fg-muted)] hover:text-[var(--fg-primary)] cursor-pointer"
                      title="Duplicate"
                      aria-label={`Duplicate ${profile.name}`}
                    >
                      <Copy className="w-3.5 h-3.5" />
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        setEditingId(
                          editingId === profile.id ? null : profile.id,
                        )
                      }
                      className="p-1.5 hover:bg-[var(--bg-elevated)] rounded text-[var(--fg-muted)] hover:text-[var(--fg-primary)] cursor-pointer"
                      title="Rename or describe"
                      aria-label={`Rename or describe ${profile.name}`}
                    >
                      <Pencil className="w-3.5 h-3.5" />
                    </button>
                    {!isActive && (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => handleActivate(profile.id)}
                        isLoading={activateMutation.isPending}
                        className="flex items-center gap-1"
                        title={`Switch from "${overview?.profile.name ?? "the current profile"}" to "${profile.name}"`}
                        aria-label={`Switch from ${overview?.profile.name ?? "the current profile"} to ${profile.name}`}
                      >
                        <Check className="w-3.5 h-3.5" />
                        <span>Activate</span>
                      </Button>
                    )}
                    {!isActive && !profile.is_default && (
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
            </li>
          );
        })}
      </ul>

      <RecentlyDeleted />

      {archivedProfiles && archivedProfiles.length > 0 && (
        <div className="space-y-3">
          <div>
            <h3 className="text-sm font-bold text-[var(--fg-primary)]">
              Archived
            </h3>
            <p className="text-xs text-[var(--fg-muted)]">
              Archived profiles keep their mods on disk and must be restored
              before they can be activated.
            </p>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {archivedProfiles.map((profile) => (
              <Card
                key={profile.id}
                className="p-5 border border-[var(--border)] opacity-80"
              >
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <h3 className="font-bold text-base text-[var(--fg-primary)]">
                        {profile.name}
                      </h3>
                      <StatusBadge variant="warning">Archived</StatusBadge>
                    </div>
                    <p className="text-xs text-[var(--fg-muted)] font-mono">
                      Revision {profile.revision.toString()} •{" "}
                      {profile.mod_count} mod(s)
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => handleRestore(profile.id)}
                      isLoading={restoreMutation.isPending}
                      className="flex items-center gap-1"
                    >
                      <ArchiveRestore className="w-3.5 h-3.5" />
                      <span>Restore</span>
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setDeletingId(profile.id)}
                    >
                      Delete...
                    </Button>
                  </div>
                </div>
              </Card>
            ))}
          </div>
        </div>
      )}

      {cloning && (
        <CloneProfileDialog
          profile={cloning}
          onClose={() => {
            setCloning(null);
            refetch();
          }}
        />
      )}

      {deletingId && (
        <DeleteProfileDialog
          profileId={deletingId}
          onClose={() => {
            setDeletingId(null);
            refetchArchived();
          }}
        />
      )}

      <BundleCard />
      <ProfileCompareCard />
      <KnownGoodCard />
      <RestorePointsCard />
      <ExperimentCard />
      <SavesCard />
      <FreezeCard />
      <ReferenceCard />
      <RecipeCard />
      <CuratorCard />
      <CollectionCard />
    </div>
  );
};
