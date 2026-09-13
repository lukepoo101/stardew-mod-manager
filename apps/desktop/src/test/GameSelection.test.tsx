import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GameSelectionScreen } from "@/features/setup/GameSelectionScreen";
import { backend } from "@/lib/backend/client";
import { GameInstallation } from "@/lib/backend/types";

describe("GameSelectionScreen", () => {
  const mockOnGameSelected = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("discovers games and allows selecting a fresh installation", async () => {
    const mockGame: GameInstallation = {
      id: "steam-stardew",
      canonical_root: "/home/user/.steam/steam/steamapps/common/Stardew Valley",
      platform_kind: "steam_native",
      detected_version: "1.6.14",
      validated_at: new Date().toISOString(),
      is_fresh: true,
    };

    vi.spyOn(backend, "discoverGames").mockResolvedValueOnce([mockGame]);

    render(<GameSelectionScreen onGameSelected={mockOnGameSelected} />);

    // Initially shows scanning
    expect(screen.getByText(/Scanning for Stardew Valley.../i)).toBeInTheDocument();

    // After loading, displays game candidate
    await waitFor(() => {
      expect(screen.getByText(/Fresh Game/i)).toBeInTheDocument();
    });

    expect(screen.getByText(mockGame.canonical_root)).toBeInTheDocument();

    const selectBtn = screen.getByRole("button", { name: /Use this game/i });
    expect(selectBtn).toBeEnabled();

    fireEvent.click(selectBtn);
    expect(mockOnGameSelected).toHaveBeenCalledWith(mockGame);
  });

  it("disables selection and shows warning for non-fresh installations", async () => {
    const nonFreshGame: GameInstallation = {
      id: "steam-modded",
      canonical_root: "/home/user/.steam/steam/steamapps/common/Stardew Valley",
      platform_kind: "steam_native",
      detected_version: "1.6.14",
      validated_at: new Date().toISOString(),
      is_fresh: false,
      validation_error: "Existing SMAPI executable found.",
    };

    vi.spyOn(backend, "discoverGames").mockResolvedValueOnce([nonFreshGame]);

    render(<GameSelectionScreen onGameSelected={mockOnGameSelected} />);

    await waitFor(() => {
      expect(screen.getByText(/Existing Mods/i)).toBeInTheDocument();
    });

    expect(screen.getByText(/Existing SMAPI executable found/i)).toBeInTheDocument();

    const selectBtn = screen.getByRole("button", { name: /Use this game/i });
    expect(selectBtn).toBeDisabled();
  });

  it("displays empty state when no games are discovered", async () => {
    vi.spyOn(backend, "discoverGames").mockResolvedValueOnce([]);

    render(<GameSelectionScreen onGameSelected={mockOnGameSelected} />);

    await waitFor(() => {
      expect(
        screen.getByText(/No Steam installations detected automatically/i)
      ).toBeInTheDocument();
    });
  });

  it("handles manual path entry and validation successfully", async () => {
    vi.spyOn(backend, "discoverGames").mockResolvedValueOnce([]);
    const chosenGame: GameInstallation = {
      id: "manual-game",
      canonical_root: "/opt/stardew",
      platform_kind: "manual_folder",
      detected_version: "1.6.14",
      validated_at: new Date().toISOString(),
      is_fresh: true,
    };
    vi.spyOn(backend, "chooseGame").mockResolvedValueOnce(chosenGame);

    render(<GameSelectionScreen onGameSelected={mockOnGameSelected} />);

    await waitFor(() => {
      expect(screen.getByPlaceholderText(/steamapps\/common\/Stardew Valley/i)).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText(/steamapps\/common\/Stardew Valley/i);
    const validateBtn = screen.getByRole("button", { name: /Validate folder/i });

    expect(validateBtn).toBeDisabled();

    fireEvent.change(input, { target: { value: "/opt/stardew" } });
    expect(validateBtn).toBeEnabled();

    fireEvent.click(validateBtn);

    await waitFor(() => {
      expect(backend.chooseGame).toHaveBeenCalledWith("/opt/stardew");
      expect(mockOnGameSelected).toHaveBeenCalledWith(chosenGame);
    });
  });

  it("displays error message when manual validation fails", async () => {
    vi.spyOn(backend, "discoverGames").mockResolvedValueOnce([]);
    vi.spyOn(backend, "chooseGame").mockRejectedValueOnce(new Error("Invalid game folder: Stardew Valley.dll missing"));

    render(<GameSelectionScreen onGameSelected={mockOnGameSelected} />);

    await waitFor(() => {
      expect(screen.getByPlaceholderText(/steamapps\/common\/Stardew Valley/i)).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText(/steamapps\/common\/Stardew Valley/i);
    const validateBtn = screen.getByRole("button", { name: /Validate folder/i });

    fireEvent.change(input, { target: { value: "/invalid/path" } });
    fireEvent.click(validateBtn);

    await waitFor(() => {
      expect(screen.getByText(/Invalid game folder: Stardew Valley.dll missing/i)).toBeInTheDocument();
    });
  });

  it("allows selecting an already-managed installation even if not fresh", async () => {
    const managedGame: GameInstallation = {
      id: "steam-managed",
      canonical_root: "/home/user/.local/share/Steam/steamapps/common/Stardew Valley",
      platform_kind: "steam_native",
      detected_version: "1.6.15",
      validated_at: new Date().toISOString(),
      is_fresh: false,
      is_managed: true,
    };

    vi.spyOn(backend, "discoverGames").mockResolvedValueOnce([managedGame]);

    render(<GameSelectionScreen onGameSelected={mockOnGameSelected} />);

    await waitFor(() => {
      expect(screen.getByText(/Managed Game/i)).toBeInTheDocument();
    });

    expect(screen.queryByText(/Non-fresh install/i)).not.toBeInTheDocument();

    const selectBtn = screen.getByRole("button", { name: /Use this game/i });
    expect(selectBtn).toBeEnabled();

    fireEvent.click(selectBtn);
    expect(mockOnGameSelected).toHaveBeenCalledWith(managedGame);
  });
});
