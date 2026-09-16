import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OnboardingView } from "@/features/onboarding/OnboardingView";
import { api } from "@/shared/api/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HashRouter } from "react-router-dom";

const renderOnboarding = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <HashRouter>
        <OnboardingView />
      </HashRouter>
    </QueryClientProvider>,
  );
afterEach(() => vi.restoreAllMocks());

describe("game discovery", () => {
  it("shows a scan error and allows retrying", async () => {
    vi.spyOn(api, "discoverGameInstallations").mockRejectedValueOnce(
      "Steam library unreadable",
    );
    renderOnboarding();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Steam library unreadable",
    );
    fireEvent.click(screen.getByRole("button", { name: "Scan Again" }));
    expect(
      await screen.findByRole("button", { name: "Use this installation" }),
    ).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does not overwrite a manual path when a slow scan completes", async () => {
    const found = await api.discoverGameInstallations();
    let finish!: (value: typeof found) => void;
    vi.spyOn(api, "discoverGameInstallations").mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    renderOnboarding();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Game installation folder" }),
      { target: { value: "/custom/game" } },
    );
    await act(async () => finish(found));
    expect(
      screen.getByRole("textbox", { name: "Game installation folder" }),
    ).toHaveValue("/custom/game");
  });

  it("keeps cancellation harmless and displays native picker errors", async () => {
    vi.spyOn(api, "pickFolderDialog")
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce("Picker unavailable");
    const register = vi.spyOn(api, "registerGameInstallation");
    renderOnboarding();
    await screen.findByRole("button", { name: "Use this installation" });
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Browse" })),
    );
    expect(register).not.toHaveBeenCalled();
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Browse" })),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Picker unavailable",
    );
  });

  it("refuses to select an unsupported installation and explains why", async () => {
    const found = await api.discoverGameInstallations();
    vi.spyOn(api, "discoverGameInstallations").mockResolvedValueOnce([
      {
        ...found[0],
        support_state: "unsupported_existing_mods",
        is_usable: false,
        evidence: ["Existing unmanaged mods found"],
      },
    ]);
    renderOnboarding();

    expect(await screen.findByText("Existing Mods")).toBeInTheDocument();
    expect(
      screen.getByText(/Existing unmanaged mods found/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Use this installation" }),
    ).toBeDisabled();
  });

  it("accepts an installation that is already managed by this application", async () => {
    const found = await api.discoverGameInstallations();
    vi.spyOn(api, "discoverGameInstallations").mockResolvedValueOnce([
      {
        ...found[0],
        support_state: "supported_managed",
        is_usable: true,
      },
    ]);
    renderOnboarding();

    expect(await screen.findByText("Managed Game")).toBeInTheDocument();
    expect(screen.queryByText("Existing Mods")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Use this installation" }),
    ).toBeEnabled();
  });

  it("shows an empty state when no installations are detected", async () => {
    vi.spyOn(api, "discoverGameInstallations").mockResolvedValueOnce([]);
    renderOnboarding();

    expect(
      await screen.findByText(/No Steam installations detected automatically/i),
    ).toBeInTheDocument();
  });
});
