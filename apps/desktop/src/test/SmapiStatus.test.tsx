import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SmapiCard } from "@/features/settings/SmapiCard";
import { api } from "@/shared/api/client";
import type {
  ProfileOverviewDto,
  SmapiStatusDto,
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
  ...over,
});

describe("SMAPI status wording", () => {
  it("never shows the tested version as the installed one", () => {
    expect(
      smapiBadge(status({ observed_version: null, comparison: "unknown" }))
        .label,
    ).toBe("SMAPI (version unknown)");
  });

  it("tells newer, older, partial and absent apart", () => {
    expect(
      smapiBadge(status({ observed_version: "4.2.0", comparison: "newer" })),
    ).toEqual({ label: "SMAPI 4.2.0, newer than tested", variant: "info" });
    expect(
      smapiBadge(status({ observed_version: "4.0.0", comparison: "older" }))
        .variant,
    ).toBe("warning");
    expect(smapiBadge(status({ state: "partial" })).label).toBe(
      "SMAPI incomplete",
    );
    expect(
      smapiBadge(status({ state: "absent", is_installed: false })).label,
    ).toBe("No SMAPI");
  });

  it("explains that newer is unverified, not broken, and is not downgraded", () => {
    const text = smapiExplanation(
      status({ observed_version: "4.2.0", comparison: "newer" }),
    );
    expect(text).toMatch(/not been verified with this manager/);
    expect(text).toMatch(/will not be downgraded/);
  });
});

afterEach(() => vi.restoreAllMocks());

describe("removing SMAPI", () => {
  it("states what changes and keeps mods", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p", name: "Main" },
      game: {
        id: "g",
        canonical_root: "/games/Stardew Valley",
        management_mode: "managed",
      },
      smapi_status: status({}),
    } as unknown as ProfileOverviewDto);
    const uninstall = vi
      .spyOn(api, "uninstallSmapi")
      .mockResolvedValue(
        status({ is_installed: false, state: "absent", comparison: "absent" }),
      );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SmapiCard />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Remove SMAPI..." }),
    );
    expect(
      screen.getByText(
        /Your profiles, their mods and stored packages are kept/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/There is no automatic undo/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove SMAPI" }));
    await waitFor(() => expect(uninstall).toHaveBeenCalledWith("g"));
    expect(
      await screen.findByText(
        /SMAPI was removed. Your mods and profiles are unchanged/,
      ),
    ).toBeInTheDocument();
  });

  it("is not offered for an installation left unmanaged", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p", name: "Main" },
      game: {
        id: "g",
        canonical_root: "/g",
        management_mode: "external_unmanaged",
      },
      smapi_status: status({}),
    } as unknown as ProfileOverviewDto);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SmapiCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText("Tested with this manager"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Remove SMAPI..." }),
    ).toBeNull();
  });
});

describe("repairing or updating SMAPI", () => {
  const renderWith = (smapi: SmapiStatusDto) => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p", name: "Main" },
      game: { id: "g", canonical_root: "/g", management_mode: "managed" },
      smapi_status: smapi,
    } as unknown as ProfileOverviewDto);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SmapiCard />
      </QueryClientProvider>,
    );
  };

  it("offers repair for a partial install and runs the previewed release", async () => {
    vi.spyOn(api, "previewSmapiSetup").mockResolvedValue({
      game_path: "/g",
      smapi_version: "4.1.10",
      smapi_source: "",
      smapi_sha256: "",
      supported_game_version: "1.6",
      installed_smapi: null,
      modifies: ["Runs the official SMAPI 4.1.10 installer on /g"],
      creates: [],
      reads: [],
      notices: [],
      checks: [],
      can_proceed: true,
    });
    const install = vi
      .spyOn(api, "installPinnedSmapi")
      .mockResolvedValue(status({}));
    renderWith(status({ state: "partial" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Repair SMAPI..." }),
    );
    expect(
      await screen.findByText("Runs the official SMAPI 4.1.10 installer on /g"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Repair SMAPI" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Repair SMAPI" }));
    await waitFor(() => expect(install).toHaveBeenCalledWith("g", "4.1.10"));
  });

  it("offers an update for an older SMAPI and nothing for a newer one", async () => {
    renderWith(status({ observed_version: "4.0.0", comparison: "older" }));
    expect(
      await screen.findByRole("button", { name: "Update SMAPI to 4.1.10..." }),
    ).toBeInTheDocument();
  });

  it("never offers to replace a newer SMAPI", async () => {
    renderWith(status({ observed_version: "4.2.0", comparison: "newer" }));
    expect(
      await screen.findByText("Tested with this manager"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Install|Update|Reinstall|Repair/ }),
    ).toBeNull();
  });
});
