import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "@/shared/api/client";
import { ApiClientError } from "@/shared/api/errors";
import { useGuardedLaunch } from "@/shared/launch/useGuardedLaunch";
import type { LaunchSessionDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

function Launcher() {
  const launch = useGuardedLaunch();
  return (
    <>
      <button type="button" onClick={() => void launch.request("Modded")}>
        Play
      </button>
      {launch.dialog}
      {launch.error && <p role="alert">{launch.error}</p>}
    </>
  );
}

const renderLauncher = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Launcher />
    </QueryClientProvider>,
  );

const started = {} as LaunchSessionDto;

describe("starting past warnings", () => {
  it("starts straight away when there are no warnings", async () => {
    vi.spyOn(api, "getLaunchPreflight").mockResolvedValue({
      can_launch: true,
      blockers: [],
      warnings: [],
    });
    const launch = vi
      .spyOn(api, "launchActiveProfile")
      .mockResolvedValue(started);
    renderLauncher();
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    await waitFor(() => expect(launch).toHaveBeenCalledWith("Modded", []));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("asks first, then sends exactly the warnings that were shown", async () => {
    vi.spyOn(api, "getLaunchPreflight").mockResolvedValue({
      can_launch: true,
      blockers: [],
      warnings: ["The last launch was not verified"],
    });
    const launch = vi
      .spyOn(api, "launchActiveProfile")
      .mockResolvedValue(started);
    renderLauncher();
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(
      await screen.findByText("The last launch was not verified"),
    ).toBeInTheDocument();
    expect(launch).not.toHaveBeenCalled();
    // Cancel is first, so the default focus is never the risky action.
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toContain("Cancel");
    fireEvent.click(screen.getByRole("button", { name: "Start anyway" }));
    await waitFor(() =>
      expect(launch).toHaveBeenCalledWith("Modded", [
        "The last launch was not verified",
      ]),
    );
  });

  it("shows changed warnings again instead of starting", async () => {
    const preflight = vi
      .spyOn(api, "getLaunchPreflight")
      .mockResolvedValueOnce({
        can_launch: true,
        blockers: [],
        warnings: ["Old warning"],
      })
      .mockResolvedValueOnce({
        can_launch: true,
        blockers: [],
        warnings: ["Old warning", "New warning"],
      });
    vi.spyOn(api, "launchActiveProfile").mockRejectedValue(
      new ApiClientError({
        code: "LAUNCH_WARNINGS_CHANGED",
        summary: "The warnings changed",
        operation_id: null,
      } as never),
    );
    renderLauncher();
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Start anyway" }),
    );
    expect(await screen.findByText("New warning")).toBeInTheDocument();
    expect(screen.getByText(/changed since you looked/)).toBeInTheDocument();
    expect(preflight).toHaveBeenCalledTimes(2);
  });
});

describe("the last played save", () => {
  it("notes when it belongs to another profile, without sending it as acknowledged", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "solo" },
    } as never);
    vi.spyOn(api, "getLaunchPreflight").mockResolvedValue({
      can_launch: true,
      blockers: [],
      warnings: [],
    });
    vi.spyOn(api, "listSaves").mockResolvedValue({
      saves_dir: "/saves",
      unavailable_links: [],
      saves: [
        {
          id: "Coop_1",
          farm_name: "Sunny",
          farmer_name: null,
          game_version: null,
          modified_at: "2026-09-30T10:00:00Z",
          size_bytes: 1,
          profile_id: "coop",
          profile_name: "Saturday co-op",
          backups: [],
        },
      ],
    });
    const launch = vi
      .spyOn(api, "launchActiveProfile")
      .mockResolvedValue({} as LaunchSessionDto);
    renderLauncher();
    // Let the overview load first.
    await waitFor(() =>
      expect(api.getActiveProfileOverview).toHaveBeenCalled(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(
      await screen.findByText(/Sunny, is linked to "Saturday co-op"/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start anyway" }));
    await waitFor(() => expect(launch).toHaveBeenCalledWith("Modded", []));
  });
});

describe("a save linked to another profile", () => {
  const setup = (revision: number) => {
    localStorage.clear();
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "solo", revision },
    } as never);
    vi.spyOn(api, "getLaunchPreflight").mockResolvedValue({
      can_launch: true,
      blockers: [],
      warnings: [],
    });
    vi.spyOn(api, "listSaves").mockResolvedValue({
      saves_dir: "/saves",
      unavailable_links: [],
      saves: [
        {
          id: "Coop_1",
          farm_name: "Sunny",
          farmer_name: null,
          game_version: null,
          modified_at: "2026-09-30T10:00:00Z",
          size_bytes: 1,
          profile_id: "coop",
          profile_name: "Saturday co-op",
          backups: [],
        },
      ],
    });
  };
  const openReview = async () => {
    await waitFor(() =>
      expect(api.getActiveProfileOverview).toHaveBeenCalled(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
  };

  it("offers switching to the linked profile instead", async () => {
    setup(1);
    const activate = vi.spyOn(api, "activateProfile").mockResolvedValue();
    renderLauncher();
    await openReview();
    fireEvent.click(
      await screen.findByRole("button", {
        name: 'Switch to "Saturday co-op" instead',
      }),
    );
    await waitFor(() => expect(activate).toHaveBeenCalledWith("coop"));
  });

  it("stops warning for an accepted pair until the profile changes", async () => {
    setup(1);
    const { unmount } = renderLauncher();
    await openReview();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Don't warn about this save with this profile until it changes",
      }),
    );
    expect(screen.queryByText(/is linked to "Saturday co-op"/)).toBeNull();
    unmount();
    vi.restoreAllMocks();

    // Same revision: no warning, so the game starts straight away.
    const keep = localStorage.getItem("smm-accepted-save-profile-pairs");
    setup(1);
    localStorage.setItem("smm-accepted-save-profile-pairs", keep ?? "[]");
    const launch = vi
      .spyOn(api, "launchActiveProfile")
      .mockResolvedValue({} as LaunchSessionDto);
    const second = renderLauncher();
    await openReview();
    await waitFor(() => expect(launch).toHaveBeenCalled());
    second.unmount();
    vi.restoreAllMocks();

    // A changed profile warns again.
    setup(2);
    localStorage.setItem("smm-accepted-save-profile-pairs", keep ?? "[]");
    renderLauncher();
    await openReview();
    expect(
      await screen.findByText(/is linked to "Saturday co-op"/),
    ).toBeInTheDocument();
  });
});
