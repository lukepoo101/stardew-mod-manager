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
