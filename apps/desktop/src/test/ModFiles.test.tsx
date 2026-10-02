import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModFilesCard } from "@/features/diagnostics/ModFilesCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("mod file checks", () => {
  it("lists only folders with something to say, separating normal edits", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    const check = vi.spyOn(api, "checkModFiles").mockResolvedValue([
      {
        deployment_id: "d1",
        mods: ["Clean"],
        status: "unchanged",
        missing: [],
        modified: [],
        added: [],
        config_changed: [],
        accepted: [],
        accepted_at: null,
      },
      {
        deployment_id: "d2",
        mods: ["Patched"],
        status: "changed",
        missing: ["manifest.json"],
        modified: ["Patched.dll"],
        added: ["config.json"],
        config_changed: [],
        accepted: [],
        accepted_at: null,
      },
      {
        deployment_id: "d3",
        mods: ["Old"],
        status: "no_record",
        missing: [],
        modified: [],
        added: [],
        config_changed: [],
        accepted: [],
        accepted_at: null,
      },
    ]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModFilesCard />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Check files" }));
    expect(
      await screen.findByText("1 of 3 mod folder(s) match what was installed."),
    ).toBeInTheDocument();
    expect(check).toHaveBeenCalledWith("p1");
    expect(screen.queryByText("Clean")).toBeNull();
    expect(screen.getByText("Patched.dll")).toBeInTheDocument();
    expect(screen.getByText("manifest.json")).toBeInTheDocument();
    expect(screen.getByText(/nothing to compare with/)).toBeInTheDocument();

    // Accepting records the state and shows the mod as locally modified.
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const accept = vi.spyOn(api, "acceptModFiles").mockResolvedValue({
      deployment_id: "d2",
      mods: ["Patched"],
      status: "locally_modified",
      missing: [],
      modified: [],
      added: ["config.json"],
      config_changed: [],
      accepted: ["Patched.dll", "manifest.json"],
      accepted_at: "2026-10-02T10:00:00Z",
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Accept these changes" }),
    );
    expect(await screen.findByText("Locally modified")).toBeInTheDocument();
    expect(accept).toHaveBeenCalledWith("p1", "d2");
    expect(
      screen.getByText(/no longer matches its original download/),
    ).toBeInTheDocument();
  });
});
