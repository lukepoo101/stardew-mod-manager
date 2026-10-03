import React, { useState, useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { GameInspectionDto, SmapiStatusDto } from "@/shared/api/generated";
import { useNavigate } from "react-router-dom";
import { Folder, CheckCircle2, RefreshCw } from "lucide-react";
import { InspectionSummary } from "./InspectionSummary";
import { SmapiSetup } from "@/features/smapi/SmapiSetup";
import { ExistingSmapiChoice } from "./ExistingSmapiChoice";

export const OnboardingView: React.FC<{
  initialGameId?: string;
  /** Opened again from Settings: nothing changes unless the user finishes. */
  rerun?: boolean;
  onComplete?: () => void | Promise<void>;
}> = ({ initialGameId, rerun, onComplete }) => {
  const navigate = useNavigate();
  const [step, setStep] = useState<"discover" | "smapi" | "complete">(
    "discover",
  );
  const [candidates, setCandidates] = useState<GameInspectionDto[]>([]);
  const [selectedGameId, setSelectedGameId] = useState<string | undefined>(
    initialGameId,
  );
  const [manualPath, setManualPath] = useState("");
  const [managed, setManaged] = useState<string[]>([]);
  useEffect(() => {
    if (!rerun) return;
    api
      .listGameInstallations()
      .then((games) => setManaged(games.map((game) => game.canonical_root)))
      .catch(() => setManaged([]));
  }, [rerun]);
  const [inspection, setInspection] = useState<GameInspectionDto | null>(null);
  const [isScanning, setIsScanning] = useState(false);

  /**
   * When setup is run again and a different installation is chosen, says
   * what switching means before anything is registered or changed.
   */
  const confirmTarget = (candidate: GameInspectionDto): boolean => {
    if (!rerun || managed.length === 0) return true;
    if (managed.includes(candidate.candidate_path)) return true;
    const smapi = candidate.has_existing_smapi
      ? "already has SMAPI"
      : "has no SMAPI yet; you can install it in the next step";
    return window.confirm(
      `Use ${candidate.candidate_path} as the active game?\n\nIt ${smapi}. It gets its own profiles (a "Main" profile is made for it). The installations you already manage keep their profiles and mods, unchanged, and you can switch back from Settings. Nothing in any game folder changes until you install SMAPI.`,
    );
  };
  const [unmanaged, setUnmanaged] = useState(false);
  // What SMAPI setup produced, checked from the files afterwards.
  const [installed, setInstalled] = useState<SmapiStatusDto | null>(null);
  // SMAPI found in the folder and left exactly as it was.
  const [keptExisting, setKeptExisting] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
          if (inspected.is_usable) setStep("smapi");
        })
        .catch((error) => setError(errorSummary(error)))
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
    } catch (e: unknown) {
      setError(`Could not scan for games: ${errorSummary(e)}`);
    } finally {
      setIsScanning(false);
    }
  };

  const handleSelectCandidate = async (candidate: GameInspectionDto) => {
    if (!confirmTarget(candidate)) return;
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
      setStep("smapi");
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to register game installation"));
    } finally {
      setIsLoading(false);
    }
  };

  // Adds an already-modded installation without managing it: nothing in it
  // is changed and SMAPI setup is skipped. It is not adoption.
  const handleContinueUnmanaged = async (candidate: GameInspectionDto) => {
    setIsLoading(true);
    setError(null);
    try {
      const game = await api.registerGameInstallation(
        candidate.candidate_path,
        candidate.storefront,
        true,
      );
      setSelectedGameId(game.id);
      setInspection(candidate);
      setUnmanaged(true);
      setStep("complete");
    } catch (e: unknown) {
      setError(errorSummary(e, "The installation was not added"));
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
      if (insp.is_usable && confirmTarget(insp)) {
        setSelectedGameId(
          (
            await api.registerGameInstallation(
              insp.candidate_path,
              insp.storefront,
            )
          ).id,
        );
        setStep("smapi");
      }
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to validate game path"));
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
          if (insp.is_usable && confirmTarget(insp)) {
            setSelectedGameId(
              (
                await api.registerGameInstallation(
                  insp.candidate_path,
                  insp.storefront,
                )
              ).id,
            );
            setStep("smapi");
          }
        } catch (e: unknown) {
          setError(errorSummary(e, "Failed to validate game path"));
        } finally {
          setIsLoading(false);
        }
      }
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to open the native folder picker"));
    } finally {
      setIsLoading(false);
    }
  };

  const handleInstalled = (status: SmapiStatusDto) => {
    setInstalled(status);
    setKeptExisting(false);
    setStep("complete");
  };

  const handleFinish = async () => {
    setIsLoading(true);
    setError(null);
    try {
      await api.completeOnboarding();
      await onComplete?.();
      navigate("/app/overview");
    } catch (error) {
      setError(errorSummary(error));
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="max-w-2xl mx-auto py-8 space-y-6">
      {rerun && (
        <div className="p-3 rounded-lg border border-[var(--border)] text-xs space-y-1">
          <p>
            {managed.length > 0
              ? `You already manage ${managed.join(", ")}. `
              : ""}
            Running setup again adds or reselects a game installation and can
            set up SMAPI. Your profiles and mods stay as they are, and nothing
            changes unless you finish a step.
          </p>
          <button
            type="button"
            onClick={() => navigate("/app/settings")}
            className="text-[var(--accent-primary)] hover:underline cursor-pointer"
          >
            Cancel and go back to Settings
          </button>
        </div>
      )}
      {/* Progress Steps */}
      <div className="flex items-center justify-between px-4 pb-4 border-b border-[var(--border)]">
        <div className="flex items-center gap-2">
          <span
            className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${
              step === "discover"
                ? "bg-[var(--accent-primary)] text-[var(--accent-fg)]"
                : "bg-[var(--success)] text-[var(--accent-fg)]"
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
                ? "bg-[var(--accent-primary)] text-[var(--accent-fg)]"
                : step === "complete"
                  ? "bg-[var(--success)] text-[var(--accent-fg)]"
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
                ? "bg-[var(--accent-primary)] text-[var(--accent-fg)]"
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
              We automatically detect your Steam game installations on this
              computer. Fresh installations with no previous mods are supported.
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
                  <h3 className="font-bold text-base text-[var(--fg-primary)]">
                    Stardew Valley
                  </h3>
                  <InspectionSummary
                    inspection={game}
                    busy={isLoading}
                    onContinueUnmanaged={() => handleContinueUnmanaged(game)}
                  />

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
              <div className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-elevated)]/30 mt-2">
                <InspectionSummary
                  inspection={inspection}
                  busy={isLoading}
                  onContinueUnmanaged={() =>
                    handleContinueUnmanaged(inspection)
                  }
                />
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

          <div className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-elevated)]/30">
            {selectedGameId && inspection?.has_existing_smapi ? (
              <ExistingSmapiChoice
                gameId={selectedGameId}
                onInstalled={handleInstalled}
                onKeep={() => {
                  setKeptExisting(true);
                  setStep("complete");
                }}
                onBack={() => setStep("discover")}
              />
            ) : selectedGameId ? (
              <SmapiSetup
                gameId={selectedGameId}
                onInstalled={handleInstalled}
                onBack={() => setStep("discover")}
              />
            ) : null}
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
              {unmanaged
                ? "The installation was added without being managed. Nothing in its folder was changed and SMAPI was not installed by the manager. A separate default profile is ready for mods you add here."
                : "Stardew Valley and SMAPI are configured with an isolated default profile. Next, install your first mod from the dashboard."}
            </p>
            {keptExisting && (
              <p className="text-xs max-w-md mx-auto mt-3">
                The SMAPI already in the game folder was left as it is. You can
                let the manager look after it, update it or change its version
                later from the SMAPI panel on the dashboard.
              </p>
            )}
            {installed && (
              <div className="text-xs text-left max-w-md mx-auto mt-3 space-y-1">
                <p>
                  {installed.is_installed
                    ? `SMAPI ${installed.observed_version ?? "(version unread)"} was found in the game folder afterwards. This is checked from the files on disk, not from the installer saying it worked.`
                    : "SMAPI was not found in the game folder afterwards."}
                </p>
                <p className="text-[var(--fg-muted)]">
                  Whether mods actually load is only known once the game runs.
                  Your first launch from the dashboard is checked against
                  SMAPI's log and the result is shown there.
                </p>
              </div>
            )}
            {inspection && (
              <p className="text-xs text-[var(--fg-muted)] mt-2 font-mono break-all">
                {inspection.candidate_path}
                {unmanaged ? " (not managed)" : ""}
              </p>
            )}
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
