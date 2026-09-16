import React, { useState, useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { GameInspectionDto } from "@/shared/api/generated";
import { useNavigate } from "react-router-dom";
import { useInstallSmapi } from "@/shared/api/hooks";
import { Folder, CheckCircle2, AlertTriangle, RefreshCw } from "lucide-react";

export const OnboardingView: React.FC<{
  initialGameId?: string;
  onComplete?: () => void | Promise<void>;
}> = ({ initialGameId, onComplete }) => {
  const navigate = useNavigate();
  const [step, setStep] = useState<"discover" | "smapi" | "complete">(
    "discover",
  );
  const [candidates, setCandidates] = useState<GameInspectionDto[]>([]);
  const [selectedGameId, setSelectedGameId] = useState<string | undefined>(
    initialGameId,
  );
  const [manualPath, setManualPath] = useState("");
  const [inspection, setInspection] = useState<GameInspectionDto | null>(null);
  const [isScanning, setIsScanning] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const installSmapiMutation = useInstallSmapi();

  useEffect(() => {
    // Auto-discover game installations on mount
    if (initialGameId) {
      setIsLoading(true);
      api
        .listGameInstallations()
        .then(async (games) => {
          const game = games.find((game) => game.id === initialGameId);
          if (!game)
            throw new Error("Registered game installation is unavailable");
          const inspected = await api.validateGameInstallationPath(
            game.canonical_root,
          );
          setInspection(inspected);
          setManualPath(inspected.candidate_path);
          if (inspected.is_usable)
            setStep(inspected.has_existing_smapi ? "complete" : "smapi");
        })
        .catch((error) => setError(String(error)))
        .finally(() => setIsLoading(false));
    }
    void autoDiscover();
  }, []);

  const autoDiscover = async () => {
    setIsScanning(true);
    setError(null);
    try {
      const found = await api.discoverGameInstallations();
      setCandidates(found);
      if (found.length > 0)
        setManualPath((current) => current || found[0].candidate_path);
    } catch (e: any) {
      setError(`Could not scan for games: ${String(e)}`);
    } finally {
      setIsScanning(false);
    }
  };

  const handleSelectCandidate = async (candidate: GameInspectionDto) => {
    setIsLoading(true);
    setError(null);
    try {
      const game = await api.registerGameInstallation(
        candidate.candidate_path,
        candidate.storefront,
      );
      setSelectedGameId(game.id);
      setInspection(candidate);
      setManualPath(candidate.candidate_path);
      if (candidate.has_existing_smapi) {
        setStep("complete");
      } else {
        setStep("smapi");
      }
    } catch (e: any) {
      setError(
        e?.message || e?.toString() || "Failed to register game installation",
      );
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
        setSelectedGameId(
          (
            await api.registerGameInstallation(
              insp.candidate_path,
              insp.storefront,
            )
          ).id,
        );
        setStep(insp.has_existing_smapi ? "complete" : "smapi");
      }
    } catch (e: any) {
      setError(e?.message || e?.toString() || "Failed to validate game path");
    } finally {
      setIsLoading(false);
    }
  };

  const handleBrowse = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const folder = await api.pickFolderDialog();
      if (folder) {
        setManualPath(folder);
        setIsLoading(true);
        setError(null);
        try {
          const insp = await api.validateGameInstallationPath(folder);
          setInspection(insp);
          if (insp.is_usable) {
            setSelectedGameId(
              (
                await api.registerGameInstallation(
                  insp.candidate_path,
                  insp.storefront,
                )
              ).id,
            );
            setStep(insp.has_existing_smapi ? "complete" : "smapi");
          }
        } catch (e: any) {
          setError(
            e?.message || e?.toString() || "Failed to validate game path",
          );
        } finally {
          setIsLoading(false);
        }
      }
    } catch (e: any) {
      setError(e?.message || String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const handleInstallSmapi = async () => {
    try {
      await installSmapiMutation.mutateAsync(selectedGameId);
      setStep("complete");
    } catch (e: any) {
      setError(e?.message || "Failed to install SMAPI");
    }
  };

  const handleFinish = async () => {
    setIsLoading(true);
    setError(null);
    try {
      await api.completeOnboarding();
      await onComplete?.();
      navigate("/app/overview");
    } catch (error) {
      setError(String(error));
    } finally {
      setIsLoading(false);
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
        <div
          role="alert"
          className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm select-text"
        >
          {error}
        </div>
      )}

      {/* Step 1: Discover / Select Game */}
      {step === "discover" && (
        <div className="space-y-6">
          <div className="space-y-1">
            <h2 className="text-xl font-bold tracking-tight">
              Locate Stardew Valley
            </h2>
            <p className="text-sm text-[var(--fg-muted)]">
              We automatically detect your native Steam game installation. Fresh
              installations with no previous mods are supported.
            </p>
          </div>

          {/* Scanning indicator */}
          {isScanning ? (
            <Card className="text-center py-10 space-y-3">
              <div className="inline-block w-8 h-8 border-3 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
              <p className="text-sm font-medium text-[var(--fg-primary)]">
                Scanning for Stardew Valley installations...
              </p>
              <p className="text-xs text-[var(--fg-muted)]">
                Checking standard Steam libraries
              </p>
            </Card>
          ) : candidates.length > 0 ? (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h3 className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider">
                  Discovered Installations ({candidates.length})
                </h3>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={autoDiscover}
                  disabled={isScanning}
                >
                  <RefreshCw className="w-3.5 h-3.5 mr-1.5" />
                  Scan Again
                </Button>
              </div>

              {candidates.map((game) => (
                <Card
                  key={game.candidate_path}
                  className="space-y-4 border-2 hover:border-[var(--border-focus)] transition-all"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2 mb-1.5 flex-wrap">
                        <h3 className="font-bold text-base text-[var(--fg-primary)]">
                          Stardew Valley
                        </h3>
                        <StatusBadge variant="info">
                          {game.storefront === "steam"
                            ? "Steam Native"
                            : game.storefront === "gog"
                              ? "GOG"
                              : "Manual Folder"}
                        </StatusBadge>
                        {game.detected_version && (
                          <span className="text-xs px-2 py-0.5 rounded-md bg-[var(--bg-elevated)] border border-[var(--border)] font-mono text-[var(--fg-primary)]">
                            v{game.detected_version}
                          </span>
                        )}
                        {game.support_state === "supported_managed" ? (
                          <StatusBadge variant="success">
                            Managed Game
                          </StatusBadge>
                        ) : game.is_usable ? (
                          <StatusBadge variant="success">Ready</StatusBadge>
                        ) : (
                          <StatusBadge variant="danger">
                            Existing Mods
                          </StatusBadge>
                        )}
                        {game.has_existing_smapi && (
                          <StatusBadge variant="info">
                            SMAPI Detected
                          </StatusBadge>
                        )}
                      </div>
                      <p className="text-xs text-[var(--fg-muted)] font-mono break-all select-text">
                        {game.candidate_path}
                      </p>
                    </div>
                  </div>

                  {!game.is_usable && (
                    <div className="p-3 bg-[var(--danger-surface)] border border-[var(--danger)]/20 rounded-lg text-xs text-[var(--danger)] leading-relaxed select-text">
                      <strong>Unsupported:</strong>{" "}
                      {game.evidence.join(", ") ||
                        "Existing unmanaged mods or unsupported configuration detected."}
                    </div>
                  )}

                  <div className="flex items-center justify-end gap-3 pt-1">
                    <Button
                      variant="primary"
                      size="md"
                      disabled={!game.is_usable || isLoading}
                      onClick={() => handleSelectCandidate(game)}
                    >
                      Use this installation
                    </Button>
                  </div>
                </Card>
              ))}
            </div>
          ) : (
            <Card className="text-center py-6 space-y-2 bg-[var(--bg-elevated)]/20 border-dashed">
              <p className="text-[var(--fg-primary)] font-medium text-sm">
                No Steam installations detected automatically
              </p>
              <p className="text-xs text-[var(--fg-muted)]">
                You can manually choose or enter the directory where Stardew
                Valley is installed below.
              </p>
              <div className="pt-2">
                <Button variant="ghost" size="sm" onClick={autoDiscover}>
                  <RefreshCw className="w-3.5 h-3.5 mr-1.5" />
                  Scan Again
                </Button>
              </div>
            </Card>
          )}

          {/* Manual selection card */}
          <Card className="space-y-4">
            <div>
              <h3 className="font-bold text-sm text-[var(--fg-primary)]">
                Choose game folder manually
              </h3>
              <p className="text-xs text-[var(--fg-muted)] mt-0.5">
                Browse to or paste the directory path if your game is installed
                in a custom location.
              </p>
            </div>

            <div className="flex gap-2">
              <input
                type="text"
                value={manualPath}
                onChange={(e) => setManualPath(e.target.value)}
                aria-label="Game installation folder"
                placeholder="Select or paste your Stardew Valley folder"
                className="flex-1 px-3 py-2 bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg text-sm text-[var(--fg-primary)] focus:border-[var(--accent-primary)] outline-none font-mono"
              />
              <Button
                variant="secondary"
                onClick={handleBrowse}
                disabled={isLoading}
                type="button"
              >
                <Folder className="w-4 h-4 mr-1.5" />
                Browse
              </Button>
              <Button
                variant="primary"
                onClick={handleValidate}
                disabled={!manualPath.trim() || isLoading}
                isLoading={isLoading}
              >
                Validate & Continue
              </Button>
            </div>

            {inspection && (
              <div className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-elevated)]/30 space-y-2 mt-2">
                <div className="flex items-center justify-between">
                  <span className="font-semibold text-sm font-mono truncate mr-2">
                    {inspection.candidate_path}
                  </span>
                  <StatusBadge
                    variant={inspection.is_usable ? "success" : "danger"}
                  >
                    {inspection.support_state}
                  </StatusBadge>
                </div>

                {!inspection.is_usable && (
                  <div className="flex items-start gap-2 text-xs text-[var(--danger)] pt-1 select-text">
                    <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5" />
                    <span>
                      {inspection.evidence.join(", ") ||
                        "Existing mods or unsupported configuration detected. Please use a clean game directory."}
                    </span>
                  </div>
                )}
              </div>
            )}
          </Card>
        </div>
      )}

      {/* Step 2: SMAPI Runtime Setup */}
      {step === "smapi" && (
        <Card className="space-y-6">
          <div>
            <h2 className="text-xl font-bold tracking-tight">Set up modding</h2>
            <p className="text-sm text-[var(--fg-muted)] mt-1">
              SMAPI is the open-source mod loader required to load and run mods
              in Stardew Valley.
            </p>
          </div>

          <div className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-elevated)]/30 space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="font-bold text-sm">SMAPI 4.1.10</h3>
                <p className="text-xs text-[var(--fg-muted)]">
                  Verified pinned release for Stardew Valley 1.6+
                </p>
              </div>
              <StatusBadge variant="info">Pinned Release</StatusBadge>
            </div>
          </div>

          {installSmapiMutation.isPending && (
            <div className="text-sm text-[var(--fg-muted)] flex items-center gap-2">
              <span className="w-4 h-4 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
              <span>Installing SMAPI and verifying installation files...</span>
            </div>
          )}

          <div className="flex justify-between items-center pt-2">
            <Button
              variant="ghost"
              onClick={() => setStep("discover")}
              disabled={installSmapiMutation.isPending}
            >
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
              Stardew Valley and SMAPI are configured with an isolated default
              profile. You can now install and manage mods safely.
            </p>
          </div>
          <div className="pt-2">
            <Button
              variant="primary"
              size="lg"
              onClick={handleFinish}
              disabled={isLoading}
            >
              Go to Dashboard
            </Button>
          </div>
        </Card>
      )}
    </div>
  );
};
