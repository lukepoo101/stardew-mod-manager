import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ModList } from "@/features/mods/ModList";
import { backend } from "@/lib/backend/client";
import { InstalledMod } from "@/lib/backend/types";

describe("ModList Component", () => {
  const mockOnModRemoved = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  const mockMods: InstalledMod[] = [
    {
      id: "mod-1",
      setup_id: "setup-123",
      package_id: "pkg-1",
      unique_id: "Author.ModOne",
      name: "Mod One",
      author: "AuthorOne",
      version: "1.0.0",
      raw_manifest: "{}",
      relative_target_path: "ModOne",
      file_inventory: ["manifest.json"],
      installed_at: new Date().toISOString(),
    },
    {
      id: "mod-2",
      setup_id: "setup-123",
      package_id: "pkg-2",
      unique_id: "Author.ModTwo",
      name: "Mod Two",
      author: "AuthorTwo",
      version: "2.1.0",
      raw_manifest: "{}",
      relative_target_path: "ModTwo",
      file_inventory: ["manifest.json"],
      installed_at: new Date().toISOString(),
    },
  ];

  it("renders empty state when no mods are installed", () => {
    render(
      <ModList mods={[]} setupId="setup-123" onModRemoved={mockOnModRemoved} />,
    );

    expect(screen.getByText(/No user mods installed yet/i)).toBeInTheDocument();
    expect(
      screen.getByText(
        /Drag and drop a mod ZIP above to install your first mod/i,
      ),
    ).toBeInTheDocument();
  });

  it("renders installed mods with their manifest information", () => {
    render(
      <ModList
        mods={mockMods}
        setupId="setup-123"
        onModRemoved={mockOnModRemoved}
      />,
    );

    expect(screen.getByText("INSTALLED MODS (2)")).toBeInTheDocument();
    expect(screen.getByText("Mod One")).toBeInTheDocument();
    expect(screen.getByText(/v1.0.0 by AuthorOne/i)).toBeInTheDocument();
    expect(screen.getByText("Author.ModOne")).toBeInTheDocument();

    expect(screen.getByText("Mod Two")).toBeInTheDocument();
    expect(screen.getByText(/v2.1.0 by AuthorTwo/i)).toBeInTheDocument();
    expect(screen.getByText("Author.ModTwo")).toBeInTheDocument();
  });

  it("removes a mod successfully when user clicks Remove", async () => {
    vi.spyOn(backend, "removeMod").mockResolvedValueOnce();

    render(
      <ModList
        mods={mockMods}
        setupId="setup-123"
        onModRemoved={mockOnModRemoved}
      />,
    );

    const removeButtons = screen.getAllByRole("button", { name: /Remove/i });
    expect(removeButtons.length).toBe(2);

    fireEvent.click(removeButtons[0]);

    await waitFor(() => {
      expect(backend.removeMod).toHaveBeenCalledWith("mod-1", "setup-123");
      expect(mockOnModRemoved).toHaveBeenCalledWith("mod-1");
    });
  });

  it("handles remove error gracefully with alert", async () => {
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});
    vi.spyOn(backend, "removeMod").mockRejectedValueOnce(
      new Error("Cannot remove mod: Stardew Valley is currently running"),
    );

    render(
      <ModList
        mods={mockMods}
        setupId="setup-123"
        onModRemoved={mockOnModRemoved}
      />,
    );

    const removeButtons = screen.getAllByRole("button", { name: /Remove/i });
    fireEvent.click(removeButtons[0]);

    await waitFor(() => {
      expect(backend.removeMod).toHaveBeenCalledWith("mod-1", "setup-123");
      expect(alertSpy).toHaveBeenCalledWith(
        expect.stringContaining(
          "Cannot remove mod: Stardew Valley is currently running",
        ),
      );
      expect(mockOnModRemoved).not.toHaveBeenCalled();
    });
  });
});
