import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OnboardingView } from "@/features/onboarding/OnboardingView";
import { api } from "@/shared/api/client";
import { ApiClientError } from "@/shared/api/errors";
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
  it("shows the structured scan error and allows retrying", async () => {
    vi.spyOn(api, "discoverGameInstallations").mockRejectedValueOnce(
      new ApiClientError({
        code: "FILESYSTEM_ERROR",
        category: "filesystem",
        summary: "Steam library unreadable",
        technical_details: "permission denied for /home/user/.steam",
        context: null,
        recoverability: "terminal",
        operation_id: null,
      }),
    );
    renderOnboarding();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Steam library unreadable");
    // Technical details are diagnostics, not the user-facing message.
    expect(alert).not.toHaveTextContent("permission denied");
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
      .mockRejectedValueOnce(
        new ApiClientError({
          code: "NATIVE_DIALOG_FAILED",
          category: "internal",
          summary: "Picker unavailable",
          technical_details: "dialog channel closed",
          context: null,
          recoverability: "terminal",
          operation_id: null,
        }),
      );
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
        support_state: "existing_modded_unmanaged",
        is_usable: false,
        evidence: ["Existing unmanaged mods found"],
      },
    ]);
    renderOnboarding();

    expect(await screen.findByText("Already modded")).toBeInTheDocument();
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

    expect(await screen.findByText("Already set up")).toBeInTheDocument();
    expect(screen.queryByText("Already modded")).not.toBeInTheDocument();
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

describe("running setup again", () => {
  it("names what is already managed and changes nothing on its own", async () => {
    vi.spyOn(api, "discoverGameInstallations").mockResolvedValue([]);
    vi.spyOn(api, "listGameInstallations").mockResolvedValue([
      { id: "g1", canonical_root: "/games/Stardew Valley" } as never,
    ]);
    const register = vi.spyOn(api, "registerGameInstallation");
    render(
      <QueryClientProvider client={new QueryClient()}>
        <HashRouter>
          <OnboardingView rerun />
        </HashRouter>
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/You already manage \/games\/Stardew Valley/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Cancel and go back to Settings" }),
    ).toBeInTheDocument();
    expect(register).not.toHaveBeenCalled();
  });
});
