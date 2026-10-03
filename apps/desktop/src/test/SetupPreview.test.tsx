import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HashRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OnboardingView } from "@/features/onboarding/OnboardingView";
import { api } from "@/shared/api/client";
import type { SetupPreviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const preview = (over: Partial<SetupPreviewDto> = {}): SetupPreviewDto => ({
  game_path: "/games/Stardew Valley",
  smapi_version: "4.1.10",
  smapi_source: "https://example.invalid/smapi.zip",
  smapi_sha256: "abc",
  supported_game_version: "1.6.15",
  installed_smapi: null,
  checksum_published: true,
  compatibility: "compatible",
  modifies: [
    "Runs the official SMAPI 4.1.10 installer on /games/Stardew Valley",
  ],
  creates: ["Manager data: /data"],
  reads: ["The game's files"],
  notices: [],
  checks: [
    {
      label: "Game folder",
      path: "/games/Stardew Valley",
      needs: "read and write, to install SMAPI",
      ok: true,
      problem: null,
      remedy: null,
    },
  ],
  can_proceed: true,
  ...over,
});

async function reachSmapiStep(existingSmapi = false) {
  vi.spyOn(api, "discoverGameInstallations").mockResolvedValue([
    {
      candidate_path: "/games/Stardew Valley",
      storefront: "steam",
      operating_system: "linux",
      detected_version: "1.6.15",
      support_state: "supported_fresh",
      is_usable: true,
      has_existing_smapi: existingSmapi,
      has_existing_mods: false,
      is_writable: true,
      evidence: [],
    },
  ]);
  vi.spyOn(api, "registerGameInstallation").mockResolvedValue({
    id: "game",
    canonical_root: "/games/Stardew Valley",
    operating_system: "linux",
    storefront: "steam",
    management_mode: "managed",
    created_at: "",
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <HashRouter>
        <OnboardingView />
      </HashRouter>
    </QueryClientProvider>,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Use this installation" }),
  );
}

describe("setup preview", () => {
  it("shows what will change and installs the release that was shown", async () => {
    vi.spyOn(api, "previewSmapiSetup").mockResolvedValue(preview());
    const install = vi.spyOn(api, "installPinnedSmapi").mockResolvedValue({
      is_installed: true,
      observed_version: "4.1.10",
      tested_version: "4.1.10",
      is_compatible: true,
      state: "installed",
      comparison: "same",
      game_version: null,
      recommended_version: "4.1.10",
      installed_compatibility: "compatible",
      update_available: false,
      managed: true,
      kept_versions: [],
      catalog_source: "builtin",
      catalog_checked_at: null,
      evidence: [],
    });
    await reachSmapiStep();
    expect(
      await screen.findByText("Will change the game folder"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Will create manager-owned files"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install SMAPI" }));
    await waitFor(() =>
      expect(install).toHaveBeenCalledWith("game", "4.1.10", {
        allowUnverified: false,
      }),
    );
  });

  it("does not start when a location cannot be used", async () => {
    vi.spyOn(api, "previewSmapiSetup").mockResolvedValue(
      preview({
        can_proceed: false,
        checks: [
          {
            label: "Game folder",
            path: "/games/Stardew Valley",
            needs: "read and write, to install SMAPI",
            ok: false,
            problem: "/games/Stardew Valley cannot be written",
            remedy: "Make the game folder writable for your user.",
          },
        ],
      }),
    );
    const install = vi.spyOn(api, "installPinnedSmapi");
    await reachSmapiStep();
    expect(
      await screen.findByText("/games/Stardew Valley cannot be written"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Install SMAPI" }),
    ).toBeDisabled();
    expect(install).not.toHaveBeenCalled();
  });
});

describe("a game that already has SMAPI", () => {
  const found = {
    is_installed: true,
    observed_version: "4.1.10",
    tested_version: "4.5.2",
    is_compatible: true,
    state: "installed",
    comparison: "older",
    evidence: [],
    game_version: "1.6.15",
    recommended_version: "4.5.2",
    installed_compatibility: "compatible",
    update_available: true,
    managed: false,
    kept_versions: [],
    catalog_source: "online",
    catalog_checked_at: null,
  };

  it("reinstalls the same version by default so the manager looks after it", async () => {
    vi.spyOn(api, "getSmapiStatus").mockResolvedValue(found);
    const previewSpy = vi
      .spyOn(api, "previewSmapiSetup")
      .mockResolvedValue(preview());
    const install = vi
      .spyOn(api, "installPinnedSmapi")
      .mockResolvedValue({ ...found, managed: true });
    await reachSmapiStep(true);
    expect(
      await screen.findByRole("radio", {
        name: /Let the manager look after SMAPI 4.1.10/,
      }),
    ).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /Update to SMAPI 4.5.2/ }),
    ).not.toBeChecked();
    await waitFor(() =>
      expect(previewSpy).toHaveBeenCalledWith("game", "4.1.10"),
    );
    const button = screen.getByRole("button", { name: "Reinstall SMAPI" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() =>
      expect(install).toHaveBeenCalledWith("game", "4.1.10", {
        allowUnverified: false,
      }),
    );
    expect(await screen.findByText("Ready to Mod!")).toBeInTheDocument();
  });

  it("can update instead, or leave SMAPI exactly as it is", async () => {
    vi.spyOn(api, "getSmapiStatus").mockResolvedValue(found);
    const previewSpy = vi
      .spyOn(api, "previewSmapiSetup")
      .mockResolvedValue(preview({ smapi_version: "4.5.2" }));
    const install = vi.spyOn(api, "installPinnedSmapi");
    await reachSmapiStep(true);
    fireEvent.click(
      await screen.findByRole("radio", { name: /Update to SMAPI 4.5.2/ }),
    );
    await waitFor(() =>
      expect(previewSpy).toHaveBeenCalledWith("game", "4.5.2"),
    );
    expect(
      screen.getByRole("button", { name: "Update SMAPI" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Leave it as it is/ }));
    fireEvent.click(
      screen.getByRole("button", { name: "Continue without changes" }),
    );
    expect(await screen.findByText(/left as it is/)).toBeInTheDocument();
    expect(install).not.toHaveBeenCalled();
  });
});
