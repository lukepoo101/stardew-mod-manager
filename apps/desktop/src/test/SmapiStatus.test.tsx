import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SmapiPanel } from "@/features/smapi/SmapiPanel";
import { SmapiUpdateNotice } from "@/features/smapi/SmapiUpdateNotice";
import { api } from "@/shared/api/client";
import type {
  ProfileOverviewDto,
  SetupPreviewDto,
  SmapiCatalogDto,
  SmapiReleaseDto,
  SmapiStatusDto,
  SmapiUpdateSettingsDto,
} from "@/shared/api/generated";
import { smapiBadge, smapiExplanation } from "@/shared/smapi/status";

const status = (over: Partial<SmapiStatusDto>): SmapiStatusDto => ({
  is_installed: true,
  observed_version: "4.1.10",
  tested_version: "4.1.10",
  is_compatible: true,
  state: "installed",
  comparison: "same",
  evidence: [],
  game_version: "1.6.15",
  recommended_version: "4.1.10",
  installed_compatibility: "compatible",
  update_available: false,
  managed: true,
  kept_versions: [],
  catalog_source: "cached",
  catalog_checked_at: "2026-10-01T10:00:00Z",
  ...over,
});

const preview = (over: Partial<SetupPreviewDto> = {}): SetupPreviewDto => ({
  game_path: "/g",
  smapi_version: "4.1.10",
  smapi_source: "https://example.invalid/smapi.zip",
  smapi_sha256: "abc",
  supported_game_version: "1.6.14 or newer",
  installed_smapi: null,
  checksum_published: true,
  compatibility: "compatible",
  modifies: ["Runs the official SMAPI installer on /g"],
  creates: [],
  reads: [],
  notices: [],
  checks: [],
  can_proceed: true,
  ...over,
});

const release = (over: Partial<SmapiReleaseDto>): SmapiReleaseDto => ({
  version: "4.1.10",
  published_at: null,
  prerelease: false,
  checksum: "published",
  min_game: "1.6.14",
  max_game: null,
  compatibility: "compatible",
  notes_url: "",
  installer_kept: false,
  is_installed: false,
  is_recommended: false,
  ...over,
});

const catalog = (releases: SmapiReleaseDto[]): SmapiCatalogDto => ({
  releases,
  source: "cached",
  checked_at: null,
  error: null,
  game_version: "1.6.15",
  recommended: null,
  installed: null,
});

const settings = (
  over: Partial<SmapiUpdateSettingsDto> = {},
): SmapiUpdateSettingsDto => ({
  mode: "notify",
  first_notice_seen: false,
  skipped_version: null,
  ...over,
});

function mockGame(smapi: SmapiStatusDto, mode = "managed") {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p", name: "Main" },
    game: { id: "g", canonical_root: "/g", management_mode: mode },
    smapi_status: smapi,
  } as unknown as ProfileOverviewDto);
}

const renderUi = (ui: React.ReactNode) =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );

beforeEach(() => {
  vi.spyOn(api, "getActiveLaunchSession").mockResolvedValue(null);
  vi.spyOn(api, "getSmapiCatalog").mockResolvedValue(catalog([]));
});
afterEach(() => vi.restoreAllMocks());

describe("SMAPI status wording", () => {
  it("never shows another version as the installed one", () => {
    expect(smapiBadge(status({ observed_version: null })).label).toBe(
      "SMAPI (version unknown)",
    );
  });

  it("tells fit, updates, partial and absent apart", () => {
    expect(smapiBadge(status({}))).toEqual({
      label: "SMAPI 4.1.10",
      variant: "success",
    });
    expect(smapiBadge(status({ update_available: true })).label).toBe(
      "SMAPI 4.1.10, update available",
    );
    expect(
      smapiBadge(status({ installed_compatibility: "game_too_new" })).variant,
    ).toBe("danger");
    expect(smapiBadge(status({ state: "partial" })).label).toBe(
      "SMAPI incomplete",
    );
    expect(
      smapiBadge(status({ state: "absent", is_installed: false })).label,
    ).toBe("No SMAPI");
  });

  it("explains fit as SMAPI declares it", () => {
    expect(smapiExplanation(status({}))).toMatch(
      /supports Stardew Valley 1.6.15, as SMAPI declares/,
    );
    expect(
      smapiExplanation(status({ installed_compatibility: "game_too_new" })),
    ).toMatch(/Update SMAPI before playing modded/);
  });
});

describe("the SMAPI panel", () => {
  it("removes SMAPI after saying what changes and that mods are kept", async () => {
    mockGame(status({}));
    const uninstall = vi
      .spyOn(api, "uninstallSmapi")
      .mockResolvedValue(
        status({ is_installed: false, state: "absent", comparison: "absent" }),
      );
    renderUi(<SmapiPanel />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Remove SMAPI..." }),
    );
    expect(
      screen.getByText(
        /Your profiles, their mods and stored packages are kept/,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove SMAPI" }));
    await waitFor(() => expect(uninstall).toHaveBeenCalledWith("g"));
    expect(await screen.findByText(/SMAPI was removed/)).toBeInTheDocument();
  });

  it("changes nothing for an installation left unmanaged", async () => {
    mockGame(status({}), "external_unmanaged");
    renderUi(<SmapiPanel />);
    expect(
      await screen.findByText(/This installation is not managed/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /SMAPI/ })).toBeNull();
  });

  it("offers an update, a reinstall, any version and roll back to kept installers", async () => {
    mockGame(
      status({
        update_available: true,
        recommended_version: "4.5.2",
        kept_versions: ["4.1.10", "4.0.8"],
      }),
    );
    renderUi(<SmapiPanel />);
    expect(
      await screen.findByRole("button", { name: "Update SMAPI to 4.5.2..." }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Reinstall SMAPI 4.1.10..." }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Change version..." }),
    ).toBeInTheDocument();
    // The installed version is not a roll back target.
    expect(
      screen.getByRole("button", { name: "Roll back to 4.0.8..." }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Roll back to 4.1.10..." }),
    ).toBeNull();
  });

  it("offers adopting a SMAPI it did not install by reinstalling the same version", async () => {
    mockGame(status({ managed: false }));
    const previewSpy = vi
      .spyOn(api, "previewSmapiSetup")
      .mockResolvedValue(preview());
    const install = vi
      .spyOn(api, "installPinnedSmapi")
      .mockResolvedValue(status({}));
    renderUi(<SmapiPanel />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Let the manager look after SMAPI 4.1.10...",
      }),
    );
    await waitFor(() => expect(previewSpy).toHaveBeenCalledWith("g", "4.1.10"));
    const button = await screen.findByRole("button", {
      name: "Reinstall SMAPI",
    });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() =>
      expect(install).toHaveBeenCalledWith("g", "4.1.10", {
        allowUnverified: false,
      }),
    );
  });

  it("needs an explicit yes for a release without a published checksum", async () => {
    mockGame(status({ kept_versions: ["4.1.10", "3.18.6"] }));
    vi.spyOn(api, "previewSmapiSetup").mockResolvedValue(
      preview({
        smapi_version: "3.18.6",
        checksum_published: false,
        compatibility: "game_too_new",
      }),
    );
    const install = vi
      .spyOn(api, "installPinnedSmapi")
      .mockResolvedValue(status({}));
    renderUi(<SmapiPanel />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Roll back to 3.18.6..." }),
    );
    const button = await screen.findByRole("button", {
      name: "Roll back SMAPI",
    });
    expect(
      await screen.findByText(/older than the SMAPI 4.1.10/),
    ).toBeInTheDocument();
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByLabelText(/without a published checksum/));
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByLabelText(/does not support this game/));
    expect(button).toBeEnabled();
    fireEvent.click(button);
    await waitFor(() =>
      expect(install).toHaveBeenCalledWith("g", "3.18.6", {
        allowUnverified: true,
      }),
    );
  });

  it("lists releases for this game and previews the one picked", async () => {
    mockGame(status({}));
    vi.mocked(api.getSmapiCatalog).mockResolvedValue(
      catalog([
        release({ version: "4.5.2", is_recommended: true }),
        release({ version: "4.1.10", is_installed: true }),
        release({
          version: "3.18.6",
          compatibility: "game_too_new",
          checksum: "none",
          min_game: "1.5.6",
          max_game: "1.5.6",
        }),
      ]),
    );
    const previewSpy = vi
      .spyOn(api, "previewSmapiSetup")
      .mockResolvedValue(preview());
    renderUi(<SmapiPanel />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Change version..." }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Choose another version..." }),
    );
    expect(await screen.findByText("SMAPI 4.5.2")).toBeInTheDocument();
    expect(screen.queryByText("SMAPI 3.18.6")).toBeNull();
    fireEvent.click(
      screen.getByLabelText("Show versions for other game versions"),
    );
    expect(screen.getByText(/Stardew Valley 1.5.6 ·/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /SMAPI 4.5.2/ }));
    await waitFor(() => expect(previewSpy).toHaveBeenCalledWith("g", "4.5.2"));
  });

  it("saves the update setting", async () => {
    mockGame(status({}));
    vi.spyOn(api, "getSmapiUpdateSettings").mockResolvedValue(settings());
    const save = vi
      .spyOn(api, "setSmapiUpdateSettings")
      .mockResolvedValue(undefined);
    renderUi(<SmapiPanel />);
    fireEvent.change(await screen.findByLabelText("SMAPI updates"), {
      target: { value: "auto" },
    });
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        mode: "auto",
        first_notice_seen: true,
        skipped_version: null,
      }),
    );
  });
});

describe("SMAPI update notices", () => {
  const updatable = status({
    update_available: true,
    recommended_version: "4.5.2",
  });

  it("first offers updating automatically or never being told", async () => {
    mockGame(updatable);
    vi.spyOn(api, "getSmapiUpdateSettings").mockResolvedValue(settings());
    const save = vi
      .spyOn(api, "setSmapiUpdateSettings")
      .mockResolvedValue(undefined);
    renderUi(<SmapiUpdateNotice />);
    expect(
      await screen.findByText("SMAPI 4.5.2 is available."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Update now..." }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Don't show this again" }),
    );
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        mode: "off",
        first_notice_seen: true,
        skipped_version: null,
      }),
    );
    expect(screen.queryByText("SMAPI 4.5.2 is available.")).toBeNull();
  });

  it("later only offers updating or skipping the version", async () => {
    mockGame(updatable);
    vi.spyOn(api, "getSmapiUpdateSettings").mockResolvedValue(
      settings({ first_notice_seen: true }),
    );
    const save = vi
      .spyOn(api, "setSmapiUpdateSettings")
      .mockResolvedValue(undefined);
    renderUi(<SmapiUpdateNotice />);
    await screen.findByText("SMAPI 4.5.2 is available.");
    expect(
      screen.queryByRole("button", { name: "Always update automatically" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Skip this version" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        mode: "notify",
        first_notice_seen: true,
        skipped_version: "4.5.2",
      }),
    );
  });

  it("says nothing about a skipped version", async () => {
    mockGame(updatable);
    vi.spyOn(api, "getSmapiUpdateSettings").mockResolvedValue(
      settings({ first_notice_seen: true, skipped_version: "4.5.2" }),
    );
    renderUi(<SmapiUpdateNotice />);
    await waitFor(() => expect(api.getSmapiCatalog).toHaveBeenCalled());
    expect(screen.queryByText(/is available/)).toBeNull();
  });

  it("updates by itself when asked to, and says so", async () => {
    mockGame(updatable);
    vi.spyOn(api, "getSmapiUpdateSettings").mockResolvedValue(
      settings({ mode: "auto", first_notice_seen: true }),
    );
    const install = vi
      .spyOn(api, "installPinnedSmapi")
      .mockResolvedValue(status({ observed_version: "4.5.2" }));
    renderUi(<SmapiUpdateNotice />);
    await waitFor(() => expect(install).toHaveBeenCalledWith("g", "4.5.2"));
    expect(
      await screen.findByText(/updated automatically from 4.1.10 to 4.5.2/),
    ).toBeInTheDocument();
  });

  it("does not update automatically while the game is running", async () => {
    mockGame(updatable);
    vi.mocked(api.getActiveLaunchSession).mockResolvedValue({
      id: "s",
      state: "running_unverified",
    } as never);
    vi.spyOn(api, "getSmapiUpdateSettings").mockResolvedValue(
      settings({ mode: "auto", first_notice_seen: true }),
    );
    const install = vi.spyOn(api, "installPinnedSmapi");
    renderUi(<SmapiUpdateNotice />);
    await waitFor(() => expect(api.getSmapiCatalog).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 50));
    expect(install).not.toHaveBeenCalled();
  });
});
