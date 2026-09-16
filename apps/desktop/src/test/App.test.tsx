import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { beforeEach, afterEach, describe, it, expect, vi } from "vitest";
import { App } from "@/app/App";
import { api } from "@/shared/api/client";

beforeEach(() => {
  window.location.hash = "/";
  localStorage.clear();
});
afterEach(() => vi.restoreAllMocks());

describe("modern application startup", () => {
  it("discovers and registers a game, installs SMAPI for that game and completes onboarding", async () => {
    const boot = {
      onboarding_disposition: "not_started",
      active_game_installation_id: null,
      active_profile_id: null,
      recovery_summary: null,
      app_version: "0.1.0",
    };
    const bootstrap = vi.spyOn(api, "bootstrap").mockResolvedValue(boot);
    const register = vi.spyOn(api, "registerGameInstallation");
    const install = vi.spyOn(api, "installPinnedSmapi");
    vi.spyOn(api, "completeOnboarding").mockImplementation(async () => {
      bootstrap.mockResolvedValue({
        ...boot,
        onboarding_disposition: "completed",
        active_game_installation_id: "game",
        active_profile_id: "profile",
      });
    });
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Use this installation" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Install SMAPI" }),
    );
    expect(register).toHaveBeenCalledWith(
      expect.stringContaining("Stardew Valley"),
      "steam",
    );
    await screen.findByText("Ready to Mod!");
    expect(install).toHaveBeenCalledWith(
      (await register.mock.results[0].value).id,
    );
    fireEvent.click(screen.getByRole("button", { name: "Go to Dashboard" }));
    expect(await screen.findByText("Ready to Play")).toBeInTheDocument();
    expect(api.completeOnboarding).toHaveBeenCalledOnce();
  });

  it("shows startup errors and retries without pretending setup is empty", async () => {
    const bootstrap = vi
      .spyOn(api, "bootstrap")
      .mockRejectedValueOnce(new Error("Database unavailable"));
    render(<App />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Database unavailable",
    );
    expect(screen.queryByText("Locate Stardew Valley")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByText("Ready to Play");
    expect(bootstrap).toHaveBeenCalledTimes(2);
  });

  it("toggles light and dark themes", async () => {
    render(<App />);
    await screen.findByText("Ready to Play");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Play" })).toBeEnabled(),
    );
    const toggle = screen.getByLabelText("Toggle theme");
    await act(async () => fireEvent.click(toggle));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    await act(async () => fireEvent.click(toggle));
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});
