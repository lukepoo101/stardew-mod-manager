import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { api } from "@/shared/api/client";
import { normalizeApiError } from "@/shared/api/errors";
import type {
  DownloadedPreviewDto,
  ModUpdateDto,
  ModUpdatesDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const source = {
  repo: "focustense/StardewUI",
  tag: "v0.6.1",
  release_url: "https://github.com/focustense/StardewUI/releases/tag/v0.6.1",
  asset_name: "StardewUI.0.6.1.zip",
  size_bytes: 844205,
  has_checksum: true,
  declared_by_mod: true,
};

const mod = (over: Partial<ModUpdateDto> = {}): ModUpdateDto => ({
  profile_component_id: "c1",
  unique_id: "focustense.StardewUI",
  name: "Stardew UI",
  installed_version: "0.6.0",
  update_version: "0.6.1",
  update_url: "https://www.nexusmods.com/stardewvalley/mods/28870",
  compatibility_status: "Ok",
  compatibility_summary: "use latest version.",
  download: source,
  download_note: null,
  github_repo: "focustense/StardewUI",
  github_unread: false,
  errors: [],
  ...over,
});

const checked = (mods: ModUpdateDto[], error: string | null = null) =>
  ({
    profile_id: "p1",
    checked_at: "2026-11-06T21:00:00Z",
    mods,
    error,
  }) satisfies ModUpdatesDto;

const downloaded: DownloadedPreviewDto = {
  checksum_verified: true,
  source,
  preview: {
    operation_id: "op-1",
    artifact_hash: "hash-new",
    original_filename: "StardewUI.0.6.1.zip",
    byte_size: 844205,
    detected_components: [
      {
        unique_id: "focustense.StardewUI",
        name: "Stardew UI",
        author: "focustense",
        version: "0.6.1",
        description: null,
        relative_root: "StardewUI",
        requirements: [],
      },
    ],
    dependencies_satisfied: false,
    warnings: [],
    blockers: ["A mod with this UniqueID is already installed in this profile"],
    other_blockers: [
      "A mod with this UniqueID is already installed in this profile",
    ],
    affected_profile_component_ids: [],
    expected_profile_revision: 1,
    files: [],
    replaces: [
      {
        profile_component_id: "c1",
        unique_id: "focustense.StardewUI",
        name: "Stardew UI",
        installed_version: "0.6.0",
        incoming_version: "0.6.1",
        direction: "upgrade",
        stops_loading: [],
        also_removed: [],
      },
    ],
  },
};

function renderInstaller() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <MemoryRouter>
        <ProfileModInstaller profileId="p1" />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("mod updates", () => {
  it("checks only when asked, then lists what has an update", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue({
      profile_id: "p1",
      checked_at: null,
      mods: [],
      error: null,
    });
    const check = vi.spyOn(api, "checkModUpdates").mockResolvedValue(
      checked([
        mod(),
        mod({
          profile_component_id: "c2",
          unique_id: "Pathoschild.LookupAnything",
          name: "Lookup Anything",
          installed_version: "1.40.0",
          update_version: "1.55.0",
          update_url: "https://www.nexusmods.com/stardewvalley/mods/541",
          download: null,
        }),
        mod({
          profile_component_id: "c3",
          name: "Old Thing",
          update_version: null,
          download: null,
          compatibility_status: "Broken",
          compatibility_summary: "use Newer Thing instead.",
        }),
      ]),
    );
    renderInstaller();
    expect(await screen.findByText(/Not checked yet/)).toBeInTheDocument();
    expect(check).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await waitFor(() => expect(check).toHaveBeenCalledWith("p1"));
    expect(await screen.findByText(/0.6.0 → 0.6.1/)).toBeInTheDocument();
    expect(screen.getByText(/1.40.0 → 1.55.0/)).toBeInTheDocument();
    // GitHub can be downloaded; Nexus is a page to open.
    expect(screen.getAllByRole("button", { name: /^Download / })).toHaveLength(
      1,
    );
    expect(
      screen.getByRole("button", { name: "Download Stardew UI 0.6.1" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/^Download it from the mod page, then install/),
    ).toBeInTheDocument();
    // Read out as words, not an arrow.
    expect(screen.getByText("from 0.6.0 to 0.6.1")).toBeInTheDocument();
    expect(
      screen.getByText(/Old Thing: Broken — use Newer Thing instead./),
    ).toBeInTheDocument();
  });

  it("opens the mod page for an update it cannot download", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(
      checked([mod({ download: null })]),
    );
    const open = vi.spyOn(api, "openExternalPage").mockResolvedValue();
    renderInstaller();
    fireEvent.click(
      await screen.findByRole("button", { name: "Open Stardew UI's mod page" }),
    );
    expect(open).toHaveBeenCalledWith(
      "https://www.nexusmods.com/stardewvalley/mods/28870",
    );
  });

  it("says why a mod page could not be opened", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(
      checked([mod({ download: null })]),
    );
    vi.spyOn(api, "openExternalPage").mockRejectedValue(
      normalizeApiError({
        code: "PAGE_NOT_OPENED",
        summary: "That page could not be opened in the browser",
        category: "validation",
        recoverability: "not_recoverable",
        technical_details: null,
        context: null,
        operation_id: null,
      }),
    );
    renderInstaller();
    fireEvent.click(
      await screen.findByRole("button", { name: "Open Stardew UI's mod page" }),
    );
    expect(
      await screen.findByText("That page could not be opened in the browser"),
    ).toBeInTheDocument();
  });

  it("offers a download when GitHub could not be read during the check", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(
      checked([
        mod({
          download: null,
          github_unread: true,
          download_note: "GitHub could not be reached.",
        }),
      ]),
    );
    renderInstaller();
    expect(
      await screen.findByRole("button", { name: "Download Stardew UI 0.6.1" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/GitHub could not be reached. Download looks for it/),
    ).toBeInTheDocument();
  });

  it("keeps saying why the last check failed", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue({
      ...checked([]),
      checked_at: null,
      error: "smapi.io, which checks for updates, could not be reached.",
    });
    renderInstaller();
    expect(
      await screen.findByText(/smapi.io, which checks for updates/),
    ).toBeInTheDocument();
  });

  it("says when there is nothing to check, and counts what was", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(checked([]));
    renderInstaller();
    expect(
      await screen.findByText("This profile has no mods to check."),
    ).toBeInTheDocument();
  });

  it("counts mods up to date apart from those it could not check", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(
      checked([
        mod({ update_version: null, download: null }),
        mod({
          profile_component_id: "c2",
          name: "No Keys",
          update_version: null,
          download: null,
          compatibility_status: null,
          errors: ["It lists no update keys."],
        }),
      ]),
    );
    renderInstaller();
    expect(
      await screen.findByText(
        /No updates found. 1 mod is up to date; 1 could not be checked/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/No Keys: It lists no update keys./),
    ).toBeInTheDocument();
  });

  it("reviews a download as a replacement, saying where it came from", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(checked([mod()]));
    const prepare = vi
      .spyOn(api, "prepareModUpdate")
      .mockResolvedValue(downloaded);
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const replace = vi.spyOn(api, "replaceModVersion").mockResolvedValue({
      replaced: downloaded.preview.replaces,
      kept_settings: [],
      left_disabled: false,
      settings_backup: null,
      restore_point: null,
    });
    renderInstaller();
    fireEvent.click(
      await screen.findByRole("button", { name: "Download Stardew UI 0.6.1" }),
    );
    await waitFor(() => expect(prepare).toHaveBeenCalledWith("p1", "c1"));
    await screen.findByText("Review mod installation");
    expect(
      screen.getByText(/downloaded from the GitHub release/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/matches the fingerprint GitHub publishes/),
    ).toBeInTheDocument();
    expect(screen.getByText(/0.6.0 → 0.6.1 \(newer\)/)).toBeInTheDocument();
    expect(screen.queryByText(/will be removed too/)).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Replace installed version" }),
    );
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("op-1"));
    await waitFor(() => expect(replace).toHaveBeenCalledWith("p1", "hash-new"));
  });

  it("shows why a download was refused next to the mod", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(checked([mod()]));
    vi.spyOn(api, "prepareModUpdate").mockRejectedValue(
      normalizeApiError({
        code: "UPDATE_ARCHIVE_MISMATCH",
        summary:
          "The downloaded file StardewUI.0.6.1.zip is not Stardew UI 0.6.1: it holds Stardew UI 0.6.0 (focustense.StardewUI). Nothing was installed",
        category: "validation",
        recoverability: "not_recoverable",
        technical_details: null,
        context: null,
        operation_id: null,
      }),
    );
    renderInstaller();
    fireEvent.click(
      await screen.findByRole("button", { name: "Download Stardew UI 0.6.1" }),
    );
    expect(
      await screen.findByText(/is not Stardew UI 0.6.1/),
    ).toBeInTheDocument();
    expect(screen.queryByText("Review mod installation")).toBeNull();
  });

  it("keeps the last results when a check fails", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(checked([mod()]));
    vi.spyOn(api, "checkModUpdates").mockResolvedValue(
      checked(
        [mod()],
        "The update check at smapi.io could not be reached: offline",
      ),
    );
    renderInstaller();
    await screen.findByText(/0.6.0 → 0.6.1/);
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(
      await screen.findByText(/could not be reached: offline/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/results below are from the last check that worked/),
    ).toBeInTheDocument();
    expect(screen.getByText(/0.6.0 → 0.6.1/)).toBeInTheDocument();
  });

  it("warns when an update removes mods that came with the old archive", async () => {
    vi.spyOn(api, "getModUpdates").mockResolvedValue(checked([mod()]));
    vi.spyOn(api, "prepareModUpdate").mockResolvedValue({
      ...downloaded,
      preview: {
        ...downloaded.preview,
        replaces: [
          { ...downloaded.preview.replaces[0], also_removed: ["Extra 1.0.0"] },
        ],
      },
    });
    renderInstaller();
    fireEvent.click(
      await screen.findByRole("button", { name: "Download Stardew UI 0.6.1" }),
    );
    expect(await screen.findByText(/will be removed too/)).toBeInTheDocument();
    expect(screen.getByText("Extra 1.0.0")).toBeInTheDocument();
  });
});
