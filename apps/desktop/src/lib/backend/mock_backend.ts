import {
  AppSnapshot,
  ArchiveInspectionResult,
  GameInstallation,
  InstalledMod,
  InstallPlan,
  LaunchSession,
  Operation,
  SmapiStatus,
  SmapiReleaseInfo,
} from "./types";

export class MockBackend {
  private game: GameInstallation | undefined = {
    id: "mock-steam-game",
    canonical_root:
      "/home/user/.local/share/Steam/steamapps/common/Stardew Valley",
    platform_kind: "steam_native",
    detected_version: "1.6.14",
    validated_at: new Date().toISOString(),
    is_fresh: true,
  };

  private setup = {
    id: "setup-default",
    game_id: "mock-steam-game",
    display_name: "Default",
    relative_mods_dir: "setups/setup-default/Mods",
    created_at: new Date().toISOString(),
  };

  private smapiInstalled = false;
  private installedMods: InstalledMod[] = [];
  private activeOperation: Operation | undefined = undefined;
  private activeSession: LaunchSession | undefined = undefined;

  async getAppSnapshot(_gameId?: string): Promise<AppSnapshot> {
    return {
      selected_game: this.game,
      setup: this.game ? this.setup : undefined,
      smapi_installed: this.smapiInstalled,
      smapi_version: this.smapiInstalled ? "4.1.10" : undefined,
      installed_mods: this.installedMods,
      active_operation: this.activeOperation,
      active_session: this.activeSession,
    };
  }

  async discoverGames(): Promise<GameInstallation[]> {
    return this.game ? [this.game] : [];
  }

  async chooseGame(folderPath: string): Promise<GameInstallation> {
    this.game = {
      id: `game-${Date.now()}`,
      canonical_root: folderPath,
      platform_kind: "manual_folder",
      detected_version: "1.6.14",
      validated_at: new Date().toISOString(),
      is_fresh: true,
    };
    return this.game;
  }

  async selectGame(
    candidatePathOrId: string,
    platformKind?: string,
  ): Promise<AppSnapshot> {
    if (!this.game) {
      this.game = {
        id: "mock-steam-game",
        canonical_root: candidatePathOrId,
        platform_kind: (platformKind as any) || "steam_native",
        detected_version: "1.6.14",
        validated_at: new Date().toISOString(),
        is_fresh: true,
      };
      this.setup = {
        id: "setup-default",
        game_id: this.game.id,
        display_name: "Default",
        relative_mods_dir: "setups/setup-default/Mods",
        created_at: new Date().toISOString(),
      };
    }
    return this.getAppSnapshot();
  }

  async prepareSmapi(): Promise<SmapiReleaseInfo> {
    return {
      version: "4.1.10",
      asset_url:
        "https://github.com/Pathoschild/SMAPI/releases/download/4.1.10/SMAPI-4.1.10-installer.zip",
      sha256:
        "8c127148a76c890e485aea73910189dc41c80f822c2a0a5e3b0e762b4ee3a93e",
      tag: "4.1.10",
      commit: "fd73446090cd71f4948f34ba8c428e45aa0a3ebf",
      supported_game_version: "1.6.9+",
      installer_exec_path:
        "SMAPI 4.1.10 installer/internal/linux/SMAPI.Installer",
      launcher_exec_path: "StardewModdingAPI",
    };
  }

  async installSmapi(_gameId: string): Promise<SmapiStatus> {
    this.smapiInstalled = true;
    return {
      is_installed: true,
      observed_version: "4.1.10",
      tested_version: "4.1.10",
      is_compatible: true,
    };
  }

  async pickModFile(): Promise<string | null> {
    return "/home/user/Downloads/ExampleMod.zip";
  }

  async inspectMod(
    filePath: string,
    setupId: string,
  ): Promise<ArchiveInspectionResult> {
    const filename = filePath.split("/").pop() || "mod.zip";
    const modName = filename.replace(".zip", "");
    const uniqueId = `Author.${modName.replace(/\s+/g, "")}`;

    const result: ArchiveInspectionResult = {
      selection_id: `sel-${Date.now()}`,
      package_hash: "a1b2c3d4e5f67890",
      original_filename: filename,
      byte_size: 1024 * 512,
      plan: {
        plan_id: `plan-${Date.now()}`,
        setup_id: setupId,
        package_hash: "a1b2c3d4e5f67890",
        original_filename: filename,
        mod_folder_name: uniqueId,
        manifest: {
          unique_id: uniqueId,
          name: modName,
          author: "ModAuthor",
          version: "1.0.0",
          description: "A wonderful Stardew Valley mod",
          entry_dll: `${modName}.dll`,
          dependencies: [],
        },
        raw_manifest: "{}",
        file_inventory: [`${modName}.dll`, "manifest.json"],
        dependency_report: {
          is_installable: true,
          smapi_compatible: true,
          duplicate_id: false,
          findings: [],
        },
      },
    };
    this.lastPlan = result.plan;
    return result;
  }

  private lastPlan?: InstallPlan;

  async installMod(planOrId: InstallPlan | string): Promise<InstalledMod> {
    const plan: InstallPlan =
      typeof planOrId === "string"
        ? this.lastPlan || {
            plan_id: planOrId,
            setup_id: "setup-default",
            package_hash: "mock-hash",
            original_filename: "mock.zip",
            mod_folder_name: "MockMod",
            manifest: {
              unique_id: "Author.MockMod",
              name: "Mock Mod",
              author: "Author",
              version: "1.0.0",
              description: "Mock",
              dependencies: [],
            },
            raw_manifest: "{}",
            file_inventory: [],
            dependency_report: {
              is_installable: true,
              smapi_compatible: true,
              duplicate_id: false,
              findings: [],
            },
          }
        : planOrId;

    const modItem: InstalledMod = {
      id: `mod-${Date.now()}`,
      setup_id: plan.setup_id,
      package_id: plan.package_hash,
      unique_id: plan.manifest.unique_id,
      name: plan.manifest.name,
      author: plan.manifest.author,
      version: plan.manifest.version,
      description: plan.manifest.description,
      raw_manifest: plan.raw_manifest,
      relative_target_path: plan.mod_folder_name,
      file_inventory: plan.file_inventory,
      installed_at: new Date().toISOString(),
    };

    this.installedMods.push(modItem);
    return modItem;
  }

  async removeMod(installedModId: string, _setupId: string): Promise<void> {
    this.installedMods = this.installedMods.filter(
      (m) => m.id !== installedModId,
    );
  }

  async launchGame(gameId: string, setupId: string): Promise<LaunchSession> {
    const session: LaunchSession = {
      id: `session-${Date.now()}`,
      game_id: gameId,
      setup_id: setupId,
      launched_at: new Date().toISOString(),
      pid: 12345,
      state: "mod_load_confirmed",
      expected_mod_ids: this.installedMods.map((m) => m.unique_id),
      verification_result: {
        confirmed_mods: this.installedMods.map((m) => m.name),
        details: "All mods verified in SMAPI session",
        timestamp: new Date().toISOString(),
      },
    };
    this.activeSession = session;
    return session;
  }

  async getOperation(_id: string): Promise<Operation | null> {
    return this.activeOperation || null;
  }

  async cancelOperation(_id: string): Promise<void> {
    this.activeOperation = undefined;
  }

  async getSession(_id: string): Promise<LaunchSession | null> {
    return this.activeSession || null;
  }

  async pollSession(_id: string): Promise<LaunchSession | null> {
    return this.activeSession || null;
  }

  async terminateGame(_id?: string): Promise<void> {
    if (this.activeSession) {
      this.activeSession = {
        ...this.activeSession,
        state: "exited",
      };
    }
  }

  async getSmapiLog(): Promise<string> {
    return `[SMAPI] SMAPI 4.1.10 with Stardew Valley 1.6.15 on Linux
[SMAPI] Mods go here: ~/.config/stardew-mod-manager/setups/default/Mods
[SMAPI] Loaded 1 mods:
[SMAPI]    ExampleMod 1.0.0 by ModAuthor | A wonderful Stardew Valley mod
[SMAPI] Type 'help' for help, or 'help <cmd>' for a full description of any command.`;
  }

  async getSmapiLogPath(): Promise<string> {
    return "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt";
  }
}
