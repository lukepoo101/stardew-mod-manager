import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModNotesPanel } from "@/features/mods/ModNotesPanel";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("reinstalling a mod", () => {
  it("asks first, then reports what was reinstalled and kept", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const reinstall = vi.spyOn(api, "reinstallMod").mockResolvedValue({
      mods: ["Lib 1.0.0"],
      kept_settings: ["config.json"],
      left_disabled: false,
      settings_backup: null,
      restore_point: null,
    });
    render(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={undefined}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Reinstall from archive" }),
    );
    expect(confirm.mock.calls[0][0]).toMatch(/config.json are kept/);
    await waitFor(() => expect(reinstall).toHaveBeenCalledWith("c1"));
    expect(
      await screen.findByText(
        "Reinstalled Lib 1.0.0. Kept settings: config.json.",
      ),
    ).toBeInTheDocument();
  });

  it("does nothing when the user cancels", () => {
    vi.spyOn(window, "confirm").mockReturnValue(false);
    const reinstall = vi.spyOn(api, "reinstallMod");
    render(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={undefined}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Reinstall from archive" }),
    );
    expect(reinstall).not.toHaveBeenCalled();
  });
});

describe("a mod whose archive is gone", () => {
  it("hides the archive and disables reinstall, saying why", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    render(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={undefined}
        artifactHash={"a".repeat(64)}
      />,
    );
    expect(
      await screen.findByText(/no longer stored, so it cannot be/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Show original archive" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "Reinstall from archive" }),
    ).toBeDisabled();
  });

  it("keeps both when the archive is stored", async () => {
    const stored = vi
      .spyOn(api, "storedPackages")
      .mockResolvedValue(["a".repeat(64)]);
    render(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={undefined}
        artifactHash={"a".repeat(64)}
      />,
    );
    await waitFor(() => expect(stored).toHaveBeenCalled());
    expect(
      screen.getByRole("button", { name: "Show original archive" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Reinstall from archive" }),
    ).toBeEnabled();
  });
});
