import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Dialog } from "@/components/ui/Dialog";
import { InstalledMod } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

export interface ModListProps {
  mods: InstalledMod[];
  setupId: string;
  onModRemoved: (id: string) => void;
}

export const ModList: React.FC<ModListProps> = ({
  mods,
  setupId,
  onModRemoved,
}) => {
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [confirmRemoval, setConfirmRemoval] = useState<{
    mod: InstalledMod;
    companions: InstalledMod[];
  } | null>(null);

  const getCompanions = (targetMod: InstalledMod) => {
    return mods.filter(
      (m) =>
        m.id !== targetMod.id &&
        ((m.relative_target_path && m.relative_target_path === targetMod.relative_target_path) ||
          (m.package_id && m.package_id === targetMod.package_id))
    );
  };

  const handleInitiateRemove = (mod: InstalledMod) => {
    const companions = getCompanions(mod);
    if (companions.length > 0) {
      setConfirmRemoval({ mod, companions });
    } else {
      handleRemove(mod.id);
    }
  };

  const handleRemove = async (modId: string) => {
    setRemovingId(modId);
    try {
      await backend.removeMod(modId, setupId);
      onModRemoved(modId);
      setConfirmRemoval(null);
    } catch (e: any) {
      alert(`Failed to remove mod: ${e?.toString()}`);
    } finally {
      setRemovingId(null);
    }
  };

  if (mods.length === 0) {
    return (
      <Card className="text-center py-8 text-[var(--fg-muted)] space-y-1">
        <p className="font-semibold text-[var(--fg-primary)]">No user mods installed yet</p>
        <p className="text-xs">Drag and drop a mod ZIP above to install your first mod.</p>
      </Card>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between text-xs font-bold text-[var(--fg-muted)] px-2">
        <span>INSTALLED MODS ({mods.length})</span>
      </div>

      <div className="space-y-2">
        {mods.map((mod) => (
          <Card
            key={mod.id}
            className="flex items-center justify-between p-4 hover:border-[var(--border-focus)] transition-colors"
          >
            <div>
              <h4 className="font-bold text-sm text-[var(--fg-primary)]">
                {mod.name}
              </h4>
              <p className="text-xs text-[var(--fg-muted)]">
                v{mod.version} by {mod.author} • <span className="font-mono">{mod.unique_id}</span>
              </p>
            </div>

            <Button
              variant="danger"
              size="md"
              isLoading={removingId === mod.id}
              disabled={removingId !== null}
              onClick={() => handleInitiateRemove(mod)}
            >
              Remove
            </Button>
          </Card>
        ))}
      </div>

      {confirmRemoval && (
        <Dialog
          isOpen={true}
          onClose={() => setConfirmRemoval(null)}
          title="Remove Mod Bundle"
          footer={
            <>
              <Button
                variant="ghost"
                onClick={() => setConfirmRemoval(null)}
                disabled={removingId !== null}
              >
                Cancel
              </Button>
              <Button
                variant="danger"
                isLoading={removingId !== null}
                onClick={() => handleRemove(confirmRemoval.mod.id)}
              >
                Remove All ({confirmRemoval.companions.length + 1} Mods)
              </Button>
            </>
          }
        >
          <div className="space-y-3 text-sm">
            <p className="text-[var(--fg-primary)] leading-relaxed">
              <strong>{confirmRemoval.mod.name}</strong> was installed as part of a multi-mod bundle.
              Removing it will delete the shared folder on disk and remove the following companion mod(s):
            </p>
            <div className="p-3 bg-[var(--bg-elevated)] border border-[var(--border)] rounded-xl space-y-2 max-h-48 overflow-y-auto">
              {confirmRemoval.companions.map((comp) => (
                <div key={comp.id} className="text-xs">
                  <span className="font-semibold text-[var(--fg-primary)]">{comp.name}</span>
                  <span className="text-[var(--fg-muted)]"> (v{comp.version})</span>
                  <div className="font-mono text-[11px] text-[var(--fg-muted)]">{comp.unique_id}</div>
                </div>
              ))}
            </div>
            <p className="text-xs text-[var(--danger)]">
              This action cannot be undone without reinstalling the mod archive.
            </p>
          </div>
        </Dialog>
      )}
    </div>
  );
};
