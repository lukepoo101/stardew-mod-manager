import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { SettingsBackups } from "@/features/mods/SettingsBackups";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("settings backups", () => {
  it("lists backups when opened and restores one after confirming", async () => {
    const list = vi.spyOn(api, "listConfigBackups").mockResolvedValue([
      {
        id: "b.mod/20260901T100000000",
        created_at: "2026-09-01T10:00:00Z",
        files: ["config.json"],
      },
    ]);
    const restore = vi.spyOn(api, "restoreConfigBackup").mockResolvedValue({
      files: ["config.json"],
      saved_current: "b.mod/20260902T100000000",
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const { container } = render(
      <SettingsBackups profileId="p1" uniqueId="B.Mod" />,
    );
    expect(list).not.toHaveBeenCalled();
    const details = container.querySelector("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    const listedBefore = list.mock.calls.length;
    await waitFor(() =>
      expect(restore).toHaveBeenCalledWith(
        "p1",
        "B.Mod",
        "b.mod/20260901T100000000",
      ),
    );
    expect(list).toHaveBeenCalledWith("p1", "B.Mod");
    expect(
      await screen.findByText(
        /The settings they replaced were backed up first/,
      ),
    ).toBeInTheDocument();
    // The list is read again so the new backup shows.
    await waitFor(() =>
      expect(list.mock.calls.length).toBeGreaterThan(listedBefore),
    );
  });

  it("reads the list again each time it is opened", async () => {
    const list = vi.spyOn(api, "listConfigBackups").mockResolvedValue([]);
    const { container } = render(
      <SettingsBackups profileId="p1" uniqueId="B.Mod" />,
    );
    const details = container.querySelector("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    await waitFor(() => expect(list).toHaveBeenCalled());
    const firstOpen = list.mock.calls.length;
    details.open = false;
    fireEvent(details, new Event("toggle"));
    details.open = true;
    fireEvent(details, new Event("toggle"));
    await waitFor(() =>
      expect(list.mock.calls.length).toBeGreaterThan(firstOpen),
    );
  });
});
