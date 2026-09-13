import React, { useState, useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { GameInspectionDto } from "@/shared/api/generated";
import { useNavigate } from "@/shared/router";
import { useInstallSmapi } from "@/shared/api/hooks";
import { Folder, CheckCircle2, AlertTriangle } from "lucide-react";

export const OnboardingView: React.FC<{ onComplete?: () => void }> = ({ onComplete }) => {
  const navigate = useNavigate();
  const [step, setStep] = useState<"discover" | "smapi" | "complete">("discover");
  const [manualPath, setManualPath] = useState("");
  const [inspection, setInspection] = useState<GameInspectionDto | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const installSmapiMutation = useInstallSmapi();

  useEffect(() => {
    // Auto-discover on mount
    autoDiscover();
  }, []);

  const autoDiscover = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const games = await api.listGameInstallations();
      if (games.length > 0) {
        const insp = await api.validateGameInstallationPath(games[0].canonical_root);
        setInspection(insp);
        setManualPath(games[0].canonical_root);
        if (insp.has_existing_smapi) {
          setStep("complete");
        } else {
          setStep("smapi");
        }
      }
    } catch (e: any) {
      // Fallback
    } finally {
      setIsLoading(false);
    }
  };

  const handleValidate = async () => {
    if (!manualPath.trim()) return;
    setIsLoading(true);
    setError(null);
    try {
      const insp = await api.validateGameInstallationPath(manualPath.trim());
      setInspection(insp);
      if (insp.is_usable) {
        await api.registerGameInstallation(insp.candidate_path, insp.storefront);
        setStep(insp.has_existing_smapi ? "complete" : "smapi");
      }
    } catch (e: any) {
      setError(e?.message || "Failed to validate game path");
    } finally {
      setIsLoading(false);
    }
  };

  const handleBrowse = async () => {
    try {
      const folder = await api.pickFolderDialog();
      if (folder) {
        setManualPath(folder);
        const insp = await api.validateGameInstallationPath(folder);
        setInspection(insp);
        if (insp.is_usable) {
          await api.registerGameInstallation(insp.candidate_path, insp.storefront);
          setStep(insp.has_existing_smapi ? "complete" : "smapi");
        }
      }
    } catch (e: any) {
      setError(e?.message || "Failed to pick folder");
    }
  };

  const handleInstallSmapi = async () => {
    try {
      await installSmapiMutation.mutateAsync(undefined);
      setStep("complete");
    } catch (e: any) {
      setError(e?.message || "Failed to install SMAPI");
    }
  };

  const handleFinish = () => {
    if (onComplete) {
      onComplete();
    } else {
      navigate("/app/overview");
    }
  };

  return (
    <div className="max-w-2xl mx-auto py-8 space-y-6">
      {/* Progress Steps */}
      <div className="flex items-center justify-between px-4 pb-4 border-b border-[var(--border)]">
        <div className="flex items-center gap-2">
          <span
            className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${
              step === "discover"
                ? "bg-[var(--accent-primary)] text-white"
                : "bg-emerald-600 text-white"
            }`}
          >
            1
          </span>
          <span className="text-sm font-medium">Find Game</span>
        </div>
        <div className="h-0.5 flex-1 mx-4 bg-[var(--border)]" />
        <div className="flex items-center gap-2">
          <span
            className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${
              step === "smapi"
                ? "bg-[var(--accent-primary)] text-white"
                : step === "complete"
                ? "bg-emerald-600 text-white"
                : "bg-[var(--bg-elevated)] text-[var(--fg-muted)]"
            }`}
          >
            2
          </span>
          <span className="text-sm font-medium">Mod Loader</span>
        </div>
        <div className="h-0.5 flex-1 mx-4 bg-[var(--border)]" />
        <div className="flex items-center gap-2">
          <span
            className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${
              step === "complete"
                ? "bg-[var(--accent-primary)] text-white"
                : "bg-[var(--bg-elevated)] text-[var(--fg-muted)]"
            }`}
          >
            3
          </span>
          <span className="text-sm font-medium">Ready</span>
        </div>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm">
          {error}
        </div>
      )}

      {/* Step 1: Discover / Select Game */}
      {step === "discover" && (
        <Card className="space-y-6">
          <div>
            <h2 className="text-xl font-bold tracking-tight">Locate Stardew Valley</h2>
            <p className="text-sm text-[var(--fg-muted)] mt-1">
              Select your game installation directory. Fresh installations with no previous mods are supported.
            </p>
          </div>

          <div className="space-y-3">
            <label className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider">
              Installation Folder
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={manualPath}
                onChange={(e) => setManualPath(e.target.value)}
                placeholder="/home/.../steamapps/common/Stardew Valley"
                className="flex-1 px-3 py-2 bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg text-sm text-[var(--fg-primary)] focus:border-[var(--accent-primary)] outline-none font-mono"
              />
              <Button variant="secondary" onClick={handleBrowse} type="button">
                <Folder className="w-4 h-4 mr-1.5" />
                Browse
              </Button>
            </div>
          </div>

          {inspection && (
            <div className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-elevated)]/30 space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-sm">{inspection.candidate_path}</span>
                <StatusBadge variant={inspection.is_usable ? "success" : "danger"}>
                  {inspection.support_state}
                </StatusBadge>
              </div>

              {!inspection.is_usable && (
                <div className="flex items-start gap-2 text-xs text-[var(--danger)] pt-1">
                  <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5" />
                  <span>Existing mods or unsupported configuration detected. Please use a clean game directory.</span>
                </div>
              )}
            </div>
          )}

          <div className="flex justify-end gap-3 pt-2">
            <Button
              variant="primary"
              onClick={handleValidate}
              disabled={!manualPath.trim() || isLoading}
              isLoading={isLoading}
            >
              Validate & Continue
            </Button>
          </div>
        </Card>
      )}

      {/* Step 2: SMAPI Runtime Setup */}
      {step === "smapi" && (
        <Card className="space-y-6">
          <div>
            <h2 className="text-xl font-bold tracking-tight">Set up modding</h2>
            <p className="text-sm text-[var(--fg-muted)] mt-1">
              SMAPI is the open-source mod loader required to load and run mods in Stardew Valley.
            </p>
          </div>

          <div className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-elevated)]/30 space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="font-bold text-sm">SMAPI 4.1.10</h3>
                <p className="text-xs text-[var(--fg-muted)]">Verified pinned release for Stardew Valley 1.6+</p>
              </div>
              <StatusBadge variant="info">Pinned Release</StatusBadge>
            </div>
            <div className="text-xs font-mono text-[var(--fg-muted)] bg-[var(--bg-primary)] p-2.5 rounded border border-[var(--border)]">
              SHA-256: 8c127148a76c890e485aea73910189dc41c80f822c2a0a5e3b0e762b4ee3a93e
            </div>
          </div>

          {installSmapiMutation.isPending && (
            <div className="text-sm text-[var(--fg-muted)] flex items-center gap-2">
              <span className="w-4 h-4 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
              <span>Installing SMAPI and verifying installation files...</span>
            </div>
          )}

          <div className="flex justify-between items-center pt-2">
            <Button variant="ghost" onClick={() => setStep("discover")} disabled={installSmapiMutation.isPending}>
              Back
            </Button>
            <Button
              variant="primary"
              onClick={handleInstallSmapi}
              isLoading={installSmapiMutation.isPending}
              disabled={installSmapiMutation.isPending}
            >
              Install SMAPI
            </Button>
          </div>
        </Card>
      )}

      {/* Step 3: Complete */}
      {step === "complete" && (
        <Card className="text-center py-10 space-y-5">
          <div className="w-12 h-12 rounded-full bg-emerald-500/10 text-emerald-500 flex items-center justify-center mx-auto">
            <CheckCircle2 className="w-8 h-8" />
          </div>
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Ready to Mod!</h2>
            <p className="text-sm text-[var(--fg-muted)] mt-1 max-w-md mx-auto">
              Stardew Valley and SMAPI are configured with an isolated default profile. You can now install and manage mods safely.
            </p>
          </div>
          <div className="pt-2">
            <Button variant="primary" size="lg" onClick={handleFinish}>
              Go to Dashboard
            </Button>
          </div>
        </Card>
      )}
    </div>
  );
};
