import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModNotesPanel } from "@/features/mods/ModNotesPanel";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("reinstalling a mod", () => {
  it("shows added files first, then reinstalls keeping the chosen ones", async () => {
    vi.spyOn(api, "previewReinstall").mockResolvedValue({
      added: [
        { path: "notes.txt", size_bytes: 9, keep_by_default: true },
        { path: "Newer.dll", size_bytes: 2048, keep_by_default: false },
      ],
      no_install_record: false,
    });
    const reinstall = vi.spyOn(api, "reinstallMod").mockResolvedValue({
      mods: ["Lib 1.0.0"],
      kept_settings: ["config.json"],
      kept_added: ["notes.txt"],
      left_out: ["Newer.dll"],
      removed_copy: "/data/.recovery/op/Lib",
      no_install_record: false,
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
    // Nothing happens until the list has been seen and confirmed.
    const notes = await screen.findByRole("checkbox", { name: /notes\.txt/ });
    const dll = screen.getByRole("checkbox", { name: /Newer\.dll/ });
    expect(notes).toBeChecked();
    expect(dll).not.toBeChecked();
    expect(reinstall).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Reinstall" }));
    await waitFor(() =>
      expect(reinstall).toHaveBeenCalledWith("c1", ["notes.txt"]),
    );
    expect(
      await screen.findByText(
        /Kept files added since install: notes\.txt\. Not kept: Newer\.dll, still in \/data\/\.recovery\/op\/Lib\./,
      ),
    ).toBeInTheDocument();
  });

  it("does nothing when the user cancels", async () => {
    vi.spyOn(api, "previewReinstall").mockResolvedValue({
      added: [],
      no_install_record: true,
    });
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
    expect(
      await screen.findByText(/There is no record of what was installed/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(reinstall).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Reinstall" })).toBeNull();
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

describe("a mod's size", () => {
  it("shows the folder and stored archive sizes", async () => {
    vi.spyOn(api, "getModSize").mockResolvedValue({
      folder_bytes: 2048,
      archive_bytes: 1024,
    });
    render(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={undefined}
      />,
    );
    expect(
      await screen.findByText(/Folder 2\.0 KB · stored archive 1\.0 KB/),
    ).toBeInTheDocument();
  });
});

describe("a mod's own source link", () => {
  it("saves a link marked as the user's and can remove it", async () => {
    const save = vi.spyOn(api, "setModSourceLink").mockResolvedValue({
      unique_id: "Z.Lib",
      favourite: false,
      tags: [],
      note: "",
      source_url: "https://forums.example.com/t/1",
      source_added_at: "2026-10-02T10:00:00Z",
    });
    const { rerender } = render(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={undefined}
      />,
    );
    fireEvent.change(screen.getByLabelText(/Where you got it/), {
      target: { value: "https://forums.example.com/t/1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save link" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith(
        "Z.Lib",
        "https://forums.example.com/t/1",
      ),
    );
    rerender(
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Z.Lib"
        annotation={await save.mock.results[0].value}
      />,
    );
    expect(screen.getByText(/Added by you/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    await waitFor(() => expect(save).toHaveBeenLastCalledWith("Z.Lib", null));
  });
});
