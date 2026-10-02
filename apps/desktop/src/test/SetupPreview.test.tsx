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

async function reachSmapiStep() {
  vi.spyOn(api, "discoverGameInstallations").mockResolvedValue([
    {
      candidate_path: "/games/Stardew Valley",
      storefront: "steam",
      operating_system: "linux",
      detected_version: "1.6.15",
      support_state: "supported_fresh",
      is_usable: true,
      has_existing_smapi: false,
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
    });
    await reachSmapiStep();
    expect(
      await screen.findByText("Will change the game folder"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Will create manager-owned files"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install SMAPI" }));
    await waitFor(() => expect(install).toHaveBeenCalledWith("game", "4.1.10"));
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
