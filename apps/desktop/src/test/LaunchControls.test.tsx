import { MemoryRouter } from "react-router-dom";
import type React from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ContextHeader } from "@/components/layout/ContextHeader";
import type { ProfileOverviewDto } from "@/shared/api/generated";

const overview = (
  findings: { code: string; severity: string }[] = [],
): ProfileOverviewDto =>
  ({
    profile: { id: "p", name: "Co-op", revision: 2 },
    game: { canonical_root: "/g" },
    smapi_status: { is_installed: true, observed_version: "4.1.10" },
    health_summary: {
      status: "healthy",
      warning_count: 0,
      error_count: 0,
      info_count: 0,
      findings,
    },
  }) as unknown as ProfileOverviewDto;

const renderHeader = (ui: React.ReactElement) =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );

describe("launch controls", () => {
  it("offers modded and vanilla launches and names what each does", () => {
    const onLaunch = vi.fn();
    renderHeader(<ContextHeader overview={overview()} onLaunch={onLaunch} />);

    fireEvent.click(screen.getByRole("button", { name: /launch modded/i }));
    expect(onLaunch).toHaveBeenLastCalledWith("Modded");
    expect(
      screen.getByRole("button", { name: /launch modded/i }),
    ).toHaveAttribute("title", expect.stringContaining("Co-op"));

    fireEvent.click(screen.getByRole("button", { name: /^vanilla$/i }));
    expect(onLaunch).toHaveBeenLastCalledWith("Vanilla");
  });

  it("blocks launching while recovery is required", () => {
    const onLaunch = vi.fn();
    renderHeader(
      <ContextHeader
        overview={overview([
          { code: "RECOVERY_REQUIRED", severity: "critical" },
        ])}
        onLaunch={onLaunch}
      />,
    );
    expect(
      screen.getByRole("button", { name: /launch modded/i }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: /^vanilla$/i })).toBeDisabled();
  });

  it("labels the running session's mode", () => {
    renderHeader(
      <ContextHeader
        overview={overview()}
        activeSession={
          {
            state: "running_unverified",
            launch_mode: "vanilla",
          } as never
        }
      />,
    );
    expect(
      screen.getByRole("button", { name: /stop vanilla game/i }),
    ).toBeInTheDocument();
  });
});
