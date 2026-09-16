import {
  AppSnapshot,
  ArchiveInspectionResult,
  GameInstallation,
  InstalledMod,
  InstallPlan,
  LaunchSession,
  Operation,
  SmapiReleaseInfo,
  SmapiStatus,
} from "./types";
import { MockBackend } from "./mock_backend";

const mock = new MockBackend();

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const backend = {
  async cancelInspection(planId: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("cancel_inspection", { planId });
  },
  async retryRecovery(): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("retry_recovery");
  },
  async getAppSnapshot(gameId?: string): Promise<AppSnapshot> {
    if (!isTauri()) return mock.getAppSnapshot(gameId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_app_snapshot", { gameId });
  },

  async discoverGames(): Promise<GameInstallation[]> {
    if (!isTauri()) return mock.discoverGames();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("discover_games");
  },

  async chooseGame(folderPath: string): Promise<GameInstallation> {
    if (!isTauri()) return mock.chooseGame(folderPath);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("choose_game", { folderPath });
  },

  async selectGame(
    candidatePathOrId: string,
    platformKind?: string,
  ): Promise<AppSnapshot> {
    if (!isTauri()) return mock.selectGame(candidatePathOrId, platformKind);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("select_game", {
      candidatePath: candidatePathOrId,
      candidateId: candidatePathOrId,
      platformKind,
    });
  },

  async prepareSmapi(): Promise<SmapiReleaseInfo> {
    if (!isTauri()) return mock.prepareSmapi();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("prepare_smapi");
  },

  async installSmapi(gameId: string): Promise<SmapiStatus> {
    if (!isTauri()) return mock.installSmapi(gameId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("install_smapi", { gameId });
  },

  async pickModFile(): Promise<string | null> {
    if (!isTauri()) return mock.pickModFile();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("pick_mod_file");
  },

  async inspectMod(
    filePath: string,
    setupId: string,
  ): Promise<ArchiveInspectionResult> {
    if (!isTauri()) return mock.inspectMod(filePath, setupId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("inspect_mod", { filePath, setupId });
  },

  async installMod(planOrId: InstallPlan | string): Promise<InstalledMod> {
    if (!isTauri()) return mock.installMod(planOrId);
    const { invoke } = await import("@tauri-apps/api/core");
    const planId = typeof planOrId === "string" ? planOrId : planOrId.plan_id;
    return invoke("install_mod", { planId });
  },

  async removeMod(installedModId: string, setupId: string): Promise<void> {
    if (!isTauri()) return mock.removeMod(installedModId, setupId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("remove_mod", { installedModId, setupId });
  },

  async launchGame(gameId: string, setupId: string): Promise<LaunchSession> {
    if (!isTauri()) return mock.launchGame(gameId, setupId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("launch_game", { gameId, setupId });
  },

  async getOperation(id: string): Promise<Operation | null> {
    if (!isTauri()) return mock.getOperation(id);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_operation", { id });
  },

  async cancelOperation(id: string): Promise<void> {
    if (!isTauri()) return mock.cancelOperation(id);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("cancel_operation", { id });
  },

  async getSession(id: string): Promise<LaunchSession | null> {
    if (!isTauri()) return mock.getSession(id);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_session", { id });
  },

  async pollSession(sessionId: string): Promise<LaunchSession | null> {
    if (!isTauri()) return mock.pollSession(sessionId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("poll_session", { sessionId });
  },

  async terminateGame(sessionId?: string): Promise<void> {
    if (!isTauri()) return mock.terminateGame(sessionId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("terminate_game", { sessionId });
  },

  async getSmapiLog(): Promise<string> {
    if (!isTauri()) return mock.getSmapiLog();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_smapi_log");
  },

  async getSmapiLogPath(): Promise<string> {
    if (!isTauri()) return mock.getSmapiLogPath();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_smapi_log_path");
  },
};
