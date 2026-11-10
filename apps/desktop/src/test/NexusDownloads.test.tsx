import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { NexusCard } from "@/features/settings/NexusCard";
import { api } from "@/shared/api/client";
import type {
  DownloadedPreviewDto,
  ModUpdateDto,
  NexusStatusDto,
  OperationPreviewDto,
} from "@/shared/api/generated";
import {
  integritySentence,
  matchingUpdates,
  originLine,
  sameVersion,
  sourceSentence,
} from "@/shared/mods/downloadSource";

afterEach(() => vi.restoreAllMocks());

const connected = (premium: boolean | null): NexusStatusDto => ({
  personal_key_allowed: true,
  key_set: true,
  rejected: false,
  keychain_error: null,
  account:
    premium === null
      ? null
      : {
          name: "Farmer",
          is_premium: premium,
          hourly_remaining: 99,
          daily_remaining: null,
        },
});

const notAllowed: NexusStatusDto = {
  personal_key_allowed: false,
  key_set: false,
  rejected: false,
  keychain_error: null,
  account: null,
};

const lookup = (over: Partial<ModUpdateDto> = {}): ModUpdateDto => ({
  profile_component_id: "c1",
  unique_id: "Pathoschild.LookupAnything",
  name: "Lookup Anything",
  installed_version: "1.54.0",
  update_version: "1.55.0",
  update_url: "https://www.nexusmods.com/stardewvalley/mods/541",
  compatibility_status: "Ok",
  compatibility_summary: null,
  download: null,
  download_note: null,
  github_repo: null,
  github_unread: false,
  errors: [],
  nexus_mod_id: 541,
  ...over,
});

const preview = (version: string): OperationPreviewDto => ({
  operation_id: "op-1",
  artifact_hash: "hash",
  original_filename: "Lookup Anything-541-1-55-0.zip",
  byte_size: 1000,
  detected_components: [
    {
      unique_id: "Pathoschild.LookupAnything",
      name: "Lookup Anything",
      author: "Pathoschild",
      version,
      description: null,
      relative_root: "LookupAnything",
      requirements: [],
    },
  ],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  other_blockers: [],
  affected_profile_component_ids: [],
  expected_profile_revision: 1,
  replaces: [],
  files: [],
});

const nexusDownload: DownloadedPreviewDto = {
  preview: preview("1.55.0"),
  checksum_verified: true,
  source: {
    provider: "nexus",
    mod_id: 541,
    file_id: 10,
    file_name: "Lookup Anything-541-1-55-0.zip",
    display_name: "Lookup Anything",
    version: "1.55.0",
    size_bytes: 1000,
    page_url: "https://www.nexusmods.com/stardewvalley/mods/541",
    md5_confirmed: true,
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

const updates = (mods: ModUpdateDto[]) =>
  vi.spyOn(api, "getModUpdates").mockResolvedValue({
    profile_id: "p1",
    checked_at: "2026-11-09T10:00:00Z",
    mods,
    error: null,
    github_token_rejected: false,
  });

describe("describing where a download came from", () => {
  it("names the Nexus file and whether Nexus knew its MD5", () => {
    const { preview: _, ...origin } = nexusDownload;
    expect(sourceSentence(origin)).toBe(
      "downloaded from Nexus Mods: Lookup Anything 1.55.0 (file 10 of mod 541). Who uploaded it is not verified.",
    );
    expect(integritySentence(origin)).toBe(
      "Nexus Mods recognised its MD5 as this file.",
    );
    expect(originLine({ ...origin, checksum_verified: false })).toContain(
      "Nexus did not confirm its MD5",
    );
    expect(integritySentence(null)).toBe(
      "There is no published fingerprint to compare it with.",
    );
  });

  it("matches an archive to the update it is, by UniqueID and version", () => {
    expect(sameVersion("1.55", "1.55.0")).toBe(true);
    expect(sameVersion("1.55.0-beta", "1.55.0")).toBe(false);
    expect(matchingUpdates(preview("1.55.0"), [lookup()])).toHaveLength(1);
    expect(matchingUpdates(preview("1.54.9"), [lookup()])).toHaveLength(0);
    expect(
      matchingUpdates(preview("1.55.0"), [lookup({ update_version: null })]),
    ).toHaveLength(0);
  });
});

describe("updates on Nexus Mods", () => {
  it("a Premium member downloads one into the review", async () => {
    updates([lookup()]);
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(connected(true));
    const prepare = vi
      .spyOn(api, "prepareNexusUpdate")
      .mockResolvedValue(nexusDownload);
    renderInstaller();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Download Lookup Anything 1.55.0 from Nexus Mods",
      }),
    );
    await screen.findByText("Review mod installation");
    expect(prepare).toHaveBeenCalledWith("p1", "c1");
    expect(
      screen.getByText(/downloaded from Nexus Mods: Lookup Anything 1.55.0/),
    ).toBeTruthy();
    expect(
      screen.getByText(/Nexus Mods recognised its MD5 as this file/),
    ).toBeTruthy();
  });

  it("without Premium points to the site's Mod manager download button", async () => {
    updates([lookup()]);
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(connected(false));
    renderInstaller();
    await screen.findByText(/lets only Premium members download directly/);
    expect(
      screen.queryByRole("button", { name: /from Nexus Mods/ }),
    ).toBeNull();
  });

  it("a build that is not connected offers only the mod page", async () => {
    updates([lookup()]);
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(notAllowed);
    renderInstaller();
    await screen.findByRole("button", {
      name: "Open Lookup Anything's mod page",
    });
    expect(
      screen.queryByRole("button", { name: /from Nexus Mods/ }),
    ).toBeNull();
  });

  it("an archive chosen by hand is named as the update it is", async () => {
    updates([lookup()]);
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(notAllowed);
    vi.spyOn(api, "pickArchivesDialog").mockResolvedValue([
      "/downloads/LookupAnything.zip",
    ]);
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(
      preview("1.55.0"),
    );
    renderInstaller();
    await screen.findByText("Lookup Anything");
    fireEvent.click(screen.getByText("Install several archives..."));
    await screen.findByText(
      "This is the update the last check found for Lookup Anything (1.54.0 → 1.55.0).",
    );
  });
});

describe("Mod manager download links", () => {
  it("lists a waiting link and downloads it into the review", async () => {
    updates([]);
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(connected(false));
    vi.spyOn(api, "listNxmLinks").mockResolvedValue({
      links: [
        {
          id: "l1",
          mod_id: 541,
          file_id: 10,
          page_url:
            "https://www.nexusmods.com/stardewvalley/mods/541?tab=files",
          expires_at: null,
          installed_name: "Lookup Anything",
        },
      ],
      refused: [],
    });
    const prepare = vi
      .spyOn(api, "prepareNxmDownload")
      .mockResolvedValue(nexusDownload);
    renderInstaller();
    await screen.findByText(
      "A file for Lookup Anything (Nexus mod 541, file 10)",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Download and review" }),
    );
    await screen.findByText("Review mod installation");
    expect(prepare).toHaveBeenCalledWith("p1", "l1");
  });

  it("says why a link was refused, and that a release build cannot download", async () => {
    updates([]);
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(notAllowed);
    vi.spyOn(api, "listNxmLinks").mockResolvedValue({
      links: [
        {
          id: "l1",
          mod_id: 1,
          file_id: 2,
          page_url: "https://www.nexusmods.com/stardewvalley/mods/1?tab=files",
          expires_at: null,
          installed_name: null,
        },
      ],
      refused: ["That Nexus Mods link is for skyrim, not Stardew Valley."],
    });
    renderInstaller();
    await screen.findByText(/is for skyrim/);
    expect(screen.getByText(/not connected to Nexus Mods yet/)).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Download and review" }),
    ).toBeNull();
  });
});

describe("the Nexus settings card", () => {
  const show = () =>
    render(
      <QueryClientProvider client={new QueryClient()}>
        <NexusCard />
      </QueryClientProvider>,
    );

  it("in a release build says Nexus is not connected and asks for no key", async () => {
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(notAllowed);
    show();
    await screen.findByText(/does not download from Nexus Mods yet/);
    expect(screen.queryByLabelText(/API key/)).toBeNull();
  });

  it("connects a key, then shows whose it is without keeping it", async () => {
    vi.spyOn(api, "getNexusStatus").mockResolvedValue({
      ...notAllowed,
      personal_key_allowed: true,
    });
    vi.spyOn(api, "getNxmHandlerStatus").mockResolvedValue({
      supported: true,
      registered: false,
      error: null,
    });
    const connect = vi
      .spyOn(api, "connectNexus")
      .mockResolvedValue(connected(true));
    show();
    const input = (await screen.findByLabelText(
      "Paste your personal API key",
    )) as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: "KEY" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByText(/Connected as Farmer \(Premium/);
    expect(connect).toHaveBeenCalledWith("KEY");
    expect(
      (screen.getByLabelText("Replace the key") as HTMLInputElement).value,
    ).toBe("");
  });

  it("turns the link handler on when asked", async () => {
    vi.spyOn(api, "getNexusStatus").mockResolvedValue(connected(false));
    vi.spyOn(api, "getNxmHandlerStatus").mockResolvedValue({
      supported: true,
      registered: false,
      error: null,
    });
    const set = vi.spyOn(api, "setNxmHandler").mockResolvedValue({
      supported: true,
      registered: true,
      error: null,
    });
    show();
    const box = (await screen.findByLabelText(
      /Open "Mod manager download" links/,
    )) as HTMLInputElement;
    expect(box.checked).toBe(false);
    fireEvent.click(box);
    await waitFor(() => expect(set).toHaveBeenCalledWith(true));
    await waitFor(() => expect(box.checked).toBe(true));
  });
});
