import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { LaunchPanel } from "@/features/launch/LaunchPanel";
import { backend } from "@/lib/backend/client";
import { GameInstallation, InstalledMod, LaunchSession, Setup } from "@/lib/backend/types";

describe("LaunchPanel and Diagnostics", () => {
  const mockGame: GameInstallation = {
    id: "game-1",
    canonical_root: "/home/user/.steam/steamapps/common/Stardew Valley",
    platform_kind: "steam_native",
    detected_version: "1.6.14",
    validated_at: new Date().toISOString(),
    is_fresh: true,
  };

  const mockSetup: Setup = {
    id: "setup-1",
    game_id: "game-1",
    display_name: "Default",
    relative_mods_dir: "setups/setup-1/Mods",
    created_at: new Date().toISOString(),
  };

  const mockMods: InstalledMod[] = [
    {
      id: "mod-1",
      setup_id: "setup-1",
      package_id: "pkg-1",
      unique_id: "Pathoschild.LookupAnything",
      name: "Lookup Anything",
      author: "Pathoschild",
      version: "1.42.0",
      raw_manifest: "{}",
      relative_target_path: "LookupAnything",
      file_inventory: ["LookupAnything.dll"],
      installed_at: new Date().toISOString(),
    },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders ready to play state initially", () => {
    render(
      <LaunchPanel
        game={mockGame}
        setup={mockSetup}
        installedMods={mockMods}
      />
    );

    expect(screen.getByText("Ready to Play")).toBeInTheDocument();
    expect(screen.getByText("1 mod enabled in managed profile")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /▶ Play/i })).toBeEnabled();
    expect(screen.queryByRole("button", { name: /Stop Game/i })).not.toBeInTheDocument();
  });

  it("launches game and polls until mod load is confirmed", async () => {
    const runningSession: LaunchSession = {
      id: "session-123",
      game_id: "game-1",
      setup_id: "setup-1",
      launched_at: new Date().toISOString(),
      pid: 4567,
      state: "running_unverified",
      expected_mod_ids: ["Pathoschild.LookupAnything"],
    };

    const confirmedSession: LaunchSession = {
      ...runningSession,
      state: "mod_load_confirmed",
      verification_result: {
        confirmed_mods: ["Lookup Anything"],
        details: "All 1 expected mod(s) confirmed loaded.",
        timestamp: new Date().toISOString(),
      },
    };

    vi.spyOn(backend, "launchGame").mockResolvedValueOnce(runningSession);
    vi.spyOn(backend, "pollSession").mockResolvedValueOnce(confirmedSession);

    render(
      <LaunchPanel
        game={mockGame}
        setup={mockSetup}
        installedMods={mockMods}
      />
    );

    const playBtn = screen.getByRole("button", { name: /▶ Play/i });
    fireEvent.click(playBtn);

    await waitFor(() => {
      expect(backend.launchGame).toHaveBeenCalledWith("game-1", "setup-1");
      expect(screen.getByText("Game Running")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /Stop Game/i })).toBeInTheDocument();
    });

    // Verify polling transitioned to mod_load_confirmed
    await waitFor(() => {
      expect(screen.getByText("Mod Loaded & Verified")).toBeInTheDocument();
      expect(screen.getByText("All 1 expected mod(s) confirmed loaded.")).toBeInTheDocument();
      expect(screen.getByText(/Confirmed loaded mods: Lookup Anything/i)).toBeInTheDocument();
    });
  });

  it("allows force closing a running game session", async () => {
    const activeSession: LaunchSession = {
      id: "session-456",
      game_id: "game-1",
      setup_id: "setup-1",
      launched_at: new Date().toISOString(),
      pid: 7890,
      state: "running_unverified",
      expected_mod_ids: ["Pathoschild.LookupAnything"],
    };

    vi.spyOn(backend, "terminateGame").mockResolvedValueOnce();

    render(
      <LaunchPanel
        game={mockGame}
        setup={mockSetup}
        installedMods={mockMods}
        initialSession={activeSession}
      />
    );

    expect(screen.getByText("Game Running")).toBeInTheDocument();
    const stopBtn = screen.getByRole("button", { name: /Stop Game/i });
    expect(stopBtn).toBeInTheDocument();

    fireEvent.click(stopBtn);

    await waitFor(() => {
      expect(backend.terminateGame).toHaveBeenCalledWith("session-456");
      expect(screen.getByText("▶ Play")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Stop Game/i })).not.toBeInTheDocument();
    });
  });

  it("toggles troubleshooting section, fetches logs, and copies logs to clipboard", async () => {
    const mockLog = `[SMAPI] SMAPI 4.1.10 with Stardew Valley 1.6.14 on Linux\n[SMAPI] Loaded 1 mods:\n[SMAPI]    Lookup Anything 1.42.0\n[WARN] Sample warning message\n[ERROR] Sample error message`;
    const mockPath = "/home/user/.config/StardewValley/ErrorLogs/SMAPI-latest.txt";

    vi.spyOn(backend, "getSmapiLog").mockResolvedValue(mockLog);
    vi.spyOn(backend, "getSmapiLogPath").mockResolvedValue(mockPath);

    // Mock clipboard API
    const writeTextMock = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, {
      clipboard: {
        writeText: writeTextMock,
      },
    });

    render(
      <LaunchPanel
        game={mockGame}
        setup={mockSetup}
        installedMods={mockMods}
      />
    );

    const toggleLogsBtn = screen.getByText(/Troubleshooting & SMAPI Logs/i);
    fireEvent.click(toggleLogsBtn);

    await waitFor(() => {
      expect(backend.getSmapiLog).toHaveBeenCalled();
      expect(backend.getSmapiLogPath).toHaveBeenCalled();
      expect(screen.getByText(mockPath)).toBeInTheDocument();
      expect(screen.getByText(/SMAPI 4.1.10 with Stardew Valley/i)).toBeInTheDocument();
      expect(screen.getByText(/Sample error message/i)).toBeInTheDocument();
    });

    // Test Copy Log button
    const copyBtn = screen.getByRole("button", { name: /Copy Log/i });
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(writeTextMock).toHaveBeenCalledWith(mockLog);
      expect(screen.getByText(/Copied!/i)).toBeInTheDocument();
    });

    // Test Refresh Logs button
    const refreshBtn = screen.getByRole("button", { name: /Refresh Logs/i });
    fireEvent.click(refreshBtn);

    await waitFor(() => {
      expect(backend.getSmapiLog).toHaveBeenCalledTimes(2);
    });
  });

  it("displays verification unavailable notice when verification times out", () => {
    const timedOutSession: LaunchSession = {
      id: "session-timeout",
      game_id: "game-1",
      setup_id: "setup-1",
      launched_at: new Date().toISOString(),
      pid: 9999,
      state: "verification_unavailable",
      expected_mod_ids: ["Pathoschild.LookupAnything"],
    };

    render(
      <LaunchPanel
        game={mockGame}
        setup={mockSetup}
        installedMods={mockMods}
        initialSession={timedOutSession}
      />
    );

    expect(screen.getByText("Verification Unavailable")).toBeInTheDocument();
    expect(
      screen.getByText(/SMAPI log verification timed out after 60 seconds. The game remains running normally./i)
    ).toBeInTheDocument();
  });
});
