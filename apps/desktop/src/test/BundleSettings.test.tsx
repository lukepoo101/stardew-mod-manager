import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BundleCard } from "@/features/profiles/BundleCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("settings in bundles", () => {
  it("includes only chosen settings and shows what they might reveal", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
      game: { id: "g1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listShareableSettings").mockResolvedValue([
      {
        unique_id: "A.Mod",
        name: "A Mod",
        files: ["config.json"],
        warnings: ['config.json: "ApiKey" may be a key, token or password'],
      },
      {
        unique_id: "B.Mod",
        name: "B Mod",
        files: ["config.json"],
        warnings: [],
      },
    ]);
    vi.spyOn(api, "pickFolderDialog").mockResolvedValue("/tmp/out");
    const exportBundle = vi
      .spyOn(api, "exportProfileBundle")
      .mockResolvedValue({
        path: "/tmp/out/x.smm-bundle.zip",
        component_count: 2,
        package_count: 2,
        missing_packages: [],
        settings_included: ["B.Mod"],
      });
    const { container } = render(
      <QueryClientProvider client={new QueryClient()}>
        <BundleCard />
      </QueryClientProvider>,
    );
    const details = container.querySelector("details") as HTMLDetailsElement;
    await screen.findByText(/Include mod settings \(0 chosen\)/);
    details.open = true;
    fireEvent(details, new Event("toggle"));
    expect(
      await screen.findByText(/"ApiKey" may be a key/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/B Mod/));
    fireEvent.click(screen.getByRole("button", { name: "Export bundle..." }));
    await waitFor(() =>
      expect(exportBundle).toHaveBeenCalledWith("/tmp/out", ["B.Mod"]),
    );
    expect(
      await screen.findByText("Included settings for: B.Mod."),
    ).toBeInTheDocument();
  });
});
