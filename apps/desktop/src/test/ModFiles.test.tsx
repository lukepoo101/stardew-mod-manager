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
      },
      {
        deployment_id: "d2",
        mods: ["Patched"],
        status: "changed",
        missing: ["manifest.json"],
        modified: ["Patched.dll"],
        added: ["config.json"],
        config_changed: [],
      },
      {
        deployment_id: "d3",
        mods: ["Old"],
        status: "no_record",
        missing: [],
        modified: [],
        added: [],
        config_changed: [],
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
  });
});
