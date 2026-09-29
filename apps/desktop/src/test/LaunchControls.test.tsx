import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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

describe("launch controls", () => {
  it("offers modded and vanilla launches and names what each does", () => {
    const onLaunch = vi.fn();
    render(<ContextHeader overview={overview()} onLaunch={onLaunch} />);

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
    render(
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
    render(
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
