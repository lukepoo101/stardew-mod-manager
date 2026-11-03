import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BundleCard } from "@/features/profiles/BundleCard";
import { api } from "@/shared/api/client";
import type {
  BundleComponentDto,
  BundlePreviewDto,
  ProfileOverviewDto,
  ProfileSummaryDto,
} from "@/shared/api/generated";
import { normalizeApiError } from "@/shared/api/errors";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

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
    vi.spyOn(api, "chooseFolderDialog").mockResolvedValue("/out");
    const exportBundle = vi
      .spyOn(api, "exportProfileBundle")
      .mockResolvedValue({
        settings_included: [],
        settings_redacted: [],
        settings_skipped: [],
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
      expect(exportBundle).toHaveBeenCalledWith("/out", [], [], []),
    );
    expect(
      await screen.findByText(/Saved 3 mod\(s\)/, undefined, { timeout: 5000 }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Lost Mod/)).toBeInTheDocument();
  });

  it("shows what a move keeps, and freezes the new profile when asked", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue({
      settings_for: [],
      operating_system: "linux",
      bundle_digest: "d",
      unresolved_requirements: ["Alpha needs Core.Lib"],
      locally_modified: ["Alpha"],
      frozen_at: "2026-10-03T10:00:00Z",
      frozen_reason: "Co-op",
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
          platforms: [],
          not_for_this_computer: false,
          shares_package_with: [],
          left_out_reason: null,
        },
      ],
      missing_packages: [],
      warnings: [],
    });
    vi.spyOn(api, "importProfileBundle").mockResolvedValue({
      settings_applied: [],
      declined_optional: [],
      reference_attached: true,
      not_for_this_computer: [],
      installed_with_package: [],
      profile_id: "new",
      profile_name: "Co-op",
      installed: ["Alpha 1.0"],
      disabled: [],
      failures: [],
    });
    const freeze = vi
      .spyOn(api, "freezeProfile")
      .mockResolvedValue({} as never);
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    expect(await screen.findByText("Alpha needs Core.Lib")).toBeInTheDocument();
    expect(
      screen.getByText(/Changed outside the manager when exported: Alpha/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: /Freeze the new profile too/ }),
    ).toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "Create profile and install" }),
    );
    await waitFor(() => expect(freeze).toHaveBeenCalledWith("new", "Co-op"));
  });

  it("previews a bundle, then imports it under the chosen name", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue({
      settings_for: [],
      operating_system: "linux",
      bundle_digest: "d",
      unresolved_requirements: [],
      locally_modified: [],
      frozen_at: null,
      frozen_reason: null,
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
          platforms: [],
          not_for_this_computer: false,
          shares_package_with: [],
          left_out_reason: null,
        },
        {
          unique_id: "B",
          name: "Beta",
          version: "2.0",
          enabled: false,
          package_included: false,
          optional: false,
          platforms: [],
          not_for_this_computer: false,
          shares_package_with: [],
          left_out_reason: null,
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
        not_for_this_computer: [],
        installed_with_package: [],
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
    // Beta's package is missing, so going ahead needs an explicit yes.
    expect(
      screen.getByRole("button", { name: "Create profile and install" }),
    ).toBeDisabled();
    fireEvent.click(
      screen.getByRole("checkbox", { name: /Create the profile without Beta/ }),
    );
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
        [],
        "d",
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
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue({
      settings_for: [],
      operating_system: "linux",
      bundle_digest: "d",
      unresolved_requirements: [],
      locally_modified: [],
      frozen_at: null,
      frozen_reason: null,
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
          platforms: [],
          not_for_this_computer: false,
          shares_package_with: [],
          left_out_reason: null,
        },
        {
          unique_id: "S",
          name: "Shaders",
          version: "3.0",
          enabled: true,
          package_included: true,
          optional: true,
          platforms: [],
          not_for_this_computer: false,
          shares_package_with: [],
          left_out_reason: null,
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
        not_for_this_computer: [],
        installed_with_package: [],
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
      expect(importBundle).toHaveBeenCalledWith(
        "/b.zip",
        "g1",
        "Co-op",
        ["S"],
        [],
        "d",
      ),
    );
  });

  it("shows an unreadable bundle instead of hiding it", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/bad.zip");
    vi.spyOn(api, "inspectProfileBundle").mockRejectedValue(new Error("nope"));
    renderCard();
    await screen.findByRole("button", { name: "Import bundle..." });
    fireEvent.click(screen.getByRole("button", { name: "Import bundle..." }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  const component = (
    unique_id: string,
    extra: Partial<BundleComponentDto> = {},
  ): BundleComponentDto => ({
    unique_id,
    name: unique_id,
    version: "1.0",
    enabled: true,
    package_included: true,
    optional: false,
    platforms: [],
    not_for_this_computer: false,
    shares_package_with: [],
    left_out_reason: null,
    ...extra,
  });
  const preview = (
    components: BundleComponentDto[],
    extra: Partial<BundlePreviewDto> = {},
  ): BundlePreviewDto => ({
    settings_for: [],
    operating_system: "linux",
    bundle_digest: "d",
    unresolved_requirements: [],
    locally_modified: [],
    frozen_at: null,
    frozen_reason: null,
    profile_name: "Main",
    generated_at: "",
    components,
    missing_packages: [],
    warnings: [],
    ...extra,
  });

  it("clears the previous preview when the next bundle is refused", async () => {
    vi.spyOn(api, "chooseBundleDialog")
      .mockResolvedValueOnce("/good.zip")
      .mockResolvedValueOnce("/wrong-schema.zip");
    vi.spyOn(api, "inspectProfileBundle")
      .mockResolvedValueOnce(preview([component("Alpha")]))
      .mockRejectedValueOnce(
        new Error("This is not a Stardew Mod Manager profile recipe."),
      );
    const importBundle = vi.spyOn(api, "importProfileBundle");
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    expect(
      await screen.findByRole("button", { name: "Create profile and install" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Import bundle..." }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "not a Stardew Mod Manager profile recipe",
    );
    expect(
      screen.queryByRole("button", { name: "Create profile and install" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/Main.*lists/)).not.toBeInTheDocument();
    expect(importBundle).not.toHaveBeenCalled();
  });

  it("reads a bundle and exports to a folder typed by hand, with no dialog", async () => {
    const dialog = vi.spyOn(api, "chooseBundleDialog");
    const folderDialog = vi.spyOn(api, "chooseFolderDialog");
    const inspect = vi
      .spyOn(api, "inspectProfileBundle")
      .mockResolvedValue(preview([component("Alpha")]));
    const exportBundle = vi
      .spyOn(api, "exportProfileBundle")
      .mockResolvedValue({
        path: "/home/me/share/Main-20261102.smm-bundle.zip",
        component_count: 1,
        package_count: 1,
        missing_packages: [],
        settings_included: [],
        settings_redacted: [],
        settings_skipped: [],
      });
    renderCard();
    fireEvent.change(await screen.findByLabelText("Bundle to import"), {
      target: { value: "/home/me/friend.smm-bundle.zip" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Read bundle" }));
    await waitFor(() =>
      expect(inspect).toHaveBeenCalledWith("/home/me/friend.smm-bundle.zip"),
    );
    fireEvent.change(screen.getByLabelText("Save bundles in"), {
      target: { value: "/home/me/share" },
    });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Export bundle..." }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Export bundle..." }));
    await waitFor(() =>
      expect(exportBundle).toHaveBeenCalledWith("/home/me/share", [], [], []),
    );
    expect(dialog).not.toHaveBeenCalled();
    expect(folderDialog).not.toHaveBeenCalled();
  });

  it("shows mods for another system as not for this computer", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue(
      preview([
        component("Alpha"),
        component("Winny.DirectXShader", {
          name: "DirectX Shader",
          platforms: ["windows"],
          not_for_this_computer: true,
        }),
      ]),
    );
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    expect(
      await screen.findByText(/Not for this computer \(linux\)/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("DirectX Shader 1.0 (works on windows)"),
    ).toBeInTheDocument();
  });

  it("suggests a free name and says next to it when a name is taken", async () => {
    vi.spyOn(api, "listProfiles").mockResolvedValue([
      { id: "p", name: "Main" } as ProfileSummaryDto,
    ]);
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue(
      preview([component("Alpha")]),
    );
    vi.spyOn(api, "importProfileBundle").mockRejectedValue(
      normalizeApiError({
        code: "DUPLICATE_PROFILE_NAME",
        category: "validation",
        summary: "A profile named 'Main (from bundle)' already exists",
        technical_details: null,
        context: null,
        recoverability: "terminal",
        operation_id: null,
      }),
    );
    renderCard();
    await screen.findByRole("button", { name: "Import bundle..." });
    await waitFor(() => expect(api.listProfiles).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Import bundle..." }));
    const name = await screen.findByLabelText("New profile name");
    expect(name).toHaveValue("Main (from bundle)");
    fireEvent.click(
      screen.getByRole("button", { name: "Create profile and install" }),
    );
    await waitFor(() => expect(name).toHaveAttribute("aria-invalid", "true"));
    expect(name).toHaveAccessibleDescription(/already exists/);
  });

  it("shares flagged settings fields only for the mods the user says", async () => {
    vi.spyOn(api, "listShareableSettings").mockResolvedValue([
      {
        unique_id: "Leaky.CloudSync",
        name: "Cloud Sync",
        files: ["config.json"],
        warnings: ['config.json: "SaveBackupFolder" contains a path'],
      },
    ]);
    const exportBundle = vi
      .spyOn(api, "exportProfileBundle")
      .mockResolvedValue({
        path: "/out/Main.smm-bundle.zip",
        component_count: 1,
        package_count: 1,
        missing_packages: [],
        settings_included: ["Leaky.CloudSync"],
        settings_redacted: [
          "Cloud Sync config.json: left out ApiToken (never shared); SaveBackupFolder",
        ],
        settings_skipped: [],
      });
    const { container } = renderCard();
    fireEvent.change(await screen.findByLabelText("Save bundles in"), {
      target: { value: "/out" },
    });
    const details = container.querySelector("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    fireEvent.click(
      await screen.findByRole("checkbox", { name: /Cloud Sync/ }),
    );
    // Ticking the mod alone shares its settings with the flagged fields out.
    fireEvent.click(screen.getByRole("button", { name: "Export bundle..." }));
    await waitFor(() =>
      expect(exportBundle).toHaveBeenCalledWith(
        "/out",
        ["Leaky.CloudSync"],
        [],
        [],
      ),
    );
    expect(
      await screen.findByText(/left out ApiToken \(never shared\)/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("checkbox", { name: /Share flagged paths/ }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Export bundle..." }));
    await waitFor(() =>
      expect(exportBundle).toHaveBeenLastCalledWith(
        "/out",
        ["Leaky.CloudSync"],
        [],
        ["Leaky.CloudSync"],
      ),
    );
  });
});
