import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
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
    const restore = vi.spyOn(api, "restoreConfigBackup").mockResolvedValue();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const { container } = render(<SettingsBackups profileComponentId="c1" />);
    expect(list).not.toHaveBeenCalled();
    const details = container.querySelector("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() =>
      expect(restore).toHaveBeenCalledWith("c1", "b.mod/20260901T100000000"),
    );
    expect(await screen.findByText("Settings restored.")).toBeInTheDocument();
  });
});
