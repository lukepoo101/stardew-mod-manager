import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { ArchiveInspectionResult, InstalledMod } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

export interface ModReviewDialogProps {
  inspection: ArchiveInspectionResult | null;
  onClose: () => void;
  onModInstalled: (mod: InstalledMod) => void;
}

export const ModReviewDialog: React.FC<ModReviewDialogProps> = ({
  inspection,
  onClose,
  onModInstalled,
}) => {
  const [isInstalling, setIsInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!inspection) return null;

  const { plan } = inspection;
  const { manifest, dependency_report } = plan;

  const handleCancel = async () => {
    if (isInstalling) return;
    try { await backend.cancelInspection(plan.plan_id); onClose(); } catch (e) { setError(String(e)); }
  };

  const handleInstall = async () => {
    setIsInstalling(true);
    setError(null);
    try {
      const installed = await backend.installMod(plan);
      onModInstalled(installed);
      onClose();
    } catch (e: any) {
      setError(e?.toString() || "Failed to commit mod installation");
    } finally {
      setIsInstalling(false);
    }
  };

  return (
    <Dialog
      isOpen={!!inspection}
      onClose={handleCancel}
      title="Review Mod"
      footer={
        <>
          <Button variant="ghost" onClick={handleCancel} disabled={isInstalling}>
            Cancel
          </Button>
          <Button
            variant="primary"
            size="lg"
            isLoading={isInstalling}
            disabled={!dependency_report.is_installable || isInstalling}
            onClick={handleInstall}
          >
            Install Mod
          </Button>
        </>
      }
    >
      <div className="space-y-4 text-sm">
        <div>
          <h3 className="text-xl font-bold text-[var(--fg-primary)]">
            {manifest.name}
          </h3>
          <p className="text-xs text-[var(--fg-muted)]">
            by {manifest.author} • Version {manifest.version}
          </p>
        </div>

        {manifest.description && (
          <p className="text-xs text-[var(--fg-primary)] bg-[var(--bg-elevated)] p-3 rounded-lg border border-[var(--border)] leading-relaxed">
            {manifest.description}
          </p>
        )}

        <div className="space-y-2 border-t border-[var(--border)] pt-3 text-xs">
          <div className="flex justify-between">
            <span className="text-[var(--fg-muted)]">Unique ID:</span>
            <span className="font-mono text-[var(--fg-primary)]">{manifest.unique_id}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-[var(--fg-muted)]">Source File:</span>
            <span className="text-[var(--fg-primary)]">{plan.original_filename}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-[var(--fg-muted)]">Files in Archive:</span>
            <span className="text-[var(--fg-primary)]">{plan.file_inventory.length} files</span>
          </div>
        </div>

        {plan.component_manifests && plan.component_manifests.length > 1 && (
          <div className="space-y-2 border-t border-[var(--border)] pt-3">
            <h4 className="font-bold text-xs text-[var(--fg-primary)]">
              Included Mod Components ({plan.component_manifests.length})
            </h4>
            <div className="space-y-1.5">
              {plan.component_manifests.map((c) => (
                <div
                  key={c.manifest.unique_id}
                  className="p-2 rounded bg-[var(--bg-elevated)] border border-[var(--border)] flex justify-between items-center text-xs"
                >
                  <span className="font-medium text-[var(--fg-primary)]">{c.manifest.name}</span>
                  <span className="font-mono text-[11px] text-[var(--fg-muted)]">{c.manifest.unique_id}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Dependency Evaluation Section */}
        <div className="space-y-2 border-t border-[var(--border)] pt-3">
          <h4 className="font-bold text-xs text-[var(--fg-primary)]">Dependency Findings</h4>
          {dependency_report.findings.length === 0 ? (
            <div className="flex items-center gap-2 text-xs text-[var(--success)]">
              <span>✓</span> No external mod dependencies required.
            </div>
          ) : (
            <div className="space-y-2">
              {dependency_report.findings.map((f) => (
                <div
                  key={f.unique_id}
                  className={`p-2.5 rounded-lg border text-xs flex items-center justify-between ${
                    f.satisfied
                      ? "bg-[var(--success-surface)] border-[var(--success)]/20 text-[var(--success)]"
                      : "bg-[var(--danger-surface)] border-[var(--danger)]/20 text-[var(--danger)]"
                  }`}
                >
                  <div>
                    <span className="font-bold">{f.unique_id}</span>
                    {f.required_version && <span> (&gt;= {f.required_version})</span>}
                    <div className="text-[11px] opacity-90">{f.reason}</div>
                  </div>
                  <StatusBadge variant={f.satisfied ? "success" : "danger"}>
                    {f.satisfied ? "Satisfied" : "Missing"}
                  </StatusBadge>
                </div>
              ))}
            </div>
          )}

          {dependency_report.duplicate_id && (
            <div className="p-2.5 rounded-lg bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-xs text-[var(--danger)]">
              This mod ({manifest.unique_id}) is already installed.
            </div>
          )}
        </div>

        {error && (
          <div className="p-3 bg-[var(--danger-surface)] border border-[var(--danger)]/30 rounded-lg text-xs text-[var(--danger)]">
            {error}
          </div>
        )}
      </div>
    </Dialog>
  );
};
