import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BundleCard } from "@/features/profiles/BundleCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

function renderCard() {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p", name: "Main", revision: 1 },
    game: { id: "g1" },
  } as unknown as ProfileOverviewDto);
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <BundleCard />
    </QueryClientProvider>,
  );
}

describe("profile bundles", () => {
  it("exports to a chosen folder and reports what could not be carried", async () => {
    vi.spyOn(api, "pickFolderDialog").mockResolvedValue("/out");
    const exportBundle = vi
      .spyOn(api, "exportProfileBundle")
      .mockResolvedValue({
        settings_included: [],
        path: "/out/Main.smm-bundle.zip",
        component_count: 3,
        package_count: 2,
        missing_packages: ["Lost Mod"],
      });
    renderCard();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Export bundle..." }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Export bundle..." }));
    await waitFor(() =>
      expect(exportBundle).toHaveBeenCalledWith("/out", [], []),
    );
    expect(await screen.findByText(/Saved 3 mod\(s\)/)).toBeInTheDocument();
    expect(screen.getByText(/Lost Mod/)).toBeInTheDocument();
  });

  it("previews a bundle, then imports it under the chosen name", async () => {
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue({
      settings_for: [],
      profile_name: "Co-op",
      generated_at: "",
      components: [
        {
          unique_id: "A",
          name: "Alpha",
          version: "1.0",
          enabled: true,
          package_included: true,
          optional: false,
        },
        {
          unique_id: "B",
          name: "Beta",
          version: "2.0",
          enabled: false,
          package_included: false,
          optional: false,
        },
      ],
      missing_packages: ["Beta"],
      warnings: [
        "1 package(s) in the bundle are not used by the recipe and will be ignored.",
      ],
    });
    const importBundle = vi
      .spyOn(api, "importProfileBundle")
      .mockResolvedValue({
        settings_applied: [],
        declined_optional: [],
        reference_attached: false,
        profile_id: "new",
        profile_name: "Co-op copy",
        installed: ["Alpha 1.0"],
        disabled: [],
        failures: [
          {
            name: "Beta",
            reason: "The bundle does not include this mod's package",
          },
        ],
      });
    renderCard();
    await screen.findByRole("button", { name: "Import bundle..." });
    fireEvent.click(screen.getByRole("button", { name: "Import bundle..." }));
    expect(
      await screen.findByText(/Beta 2.0 \(disabled\)/),
    ).toBeInTheDocument();
    expect(screen.getByText("package not included")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("New profile name"), {
      target: { value: "Co-op copy" },
    });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Create profile and install" }),
      ).toBeEnabled(),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Create profile and install" }),
    );
    await waitFor(() =>
      expect(importBundle).toHaveBeenCalledWith(
        "/b.zip",
        "g1",
        "Co-op copy",
        [],
      ),
    );
    expect(
      await screen.findByText(/Some mods could not be installed/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/does not include this mod's package/),
    ).toBeInTheDocument();
  });

  it("lists optional mods separately and installs only the chosen ones", async () => {
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue({
      settings_for: [],
      profile_name: "Co-op",
      generated_at: "",
      components: [
        {
          unique_id: "A",
          name: "Alpha",
          version: "1.0",
          enabled: true,
          package_included: true,
          optional: false,
        },
        {
          unique_id: "S",
          name: "Shaders",
          version: "3.0",
          enabled: true,
          package_included: true,
          optional: true,
        },
      ],
      missing_packages: [],
      warnings: [],
    });
    const importBundle = vi
      .spyOn(api, "importProfileBundle")
      .mockResolvedValue({
        settings_applied: [],
        declined_optional: [],
        reference_attached: false,
        profile_id: "new",
        profile_name: "Co-op",
        installed: ["Alpha 1.0", "Shaders 3.0"],
        disabled: [],
        failures: [],
      });
    renderCard();
    await screen.findByRole("button", { name: "Import bundle..." });
    fireEvent.click(screen.getByRole("button", { name: "Import bundle..." }));
    const shaders = await screen.findByRole("checkbox", {
      name: "Shaders 3.0",
    });
    // Nothing optional is installed unless chosen.
    expect(shaders).not.toBeChecked();
    fireEvent.click(shaders);
    fireEvent.click(
      screen.getByRole("button", { name: "Create profile and install" }),
    );
    await waitFor(() =>
      expect(importBundle).toHaveBeenCalledWith("/b.zip", "g1", "Co-op", ["S"]),
    );
  });

  it("shows an unreadable bundle instead of hiding it", async () => {
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValue("/bad.zip");
    vi.spyOn(api, "inspectProfileBundle").mockRejectedValue(new Error("nope"));
    renderCard();
    await screen.findByRole("button", { name: "Import bundle..." });
    fireEvent.click(screen.getByRole("button", { name: "Import bundle..." }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});
