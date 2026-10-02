import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RecoveryScreen } from "@/features/recovery/RecoveryScreen";

describe("startup recovery", () => {
  it("says what was interrupted and which steps finished", () => {
    const retry = vi.fn();
    render(
      <RecoveryScreen
        summary="Operation x (SmapiSetup) requires recovery: RECOVERY_REQUIRED"
        error={null}
        onRetry={retry}
        detail={{
          operation_id: "x",
          kind: "SmapiSetup",
          state: "RecoveryRequired",
          error_code: "RECOVERY_REQUIRED",
          error_message: null,
          started_at: "2026-10-01T10:00:00Z",
          steps: [
            {
              step_index: 0,
              step_kind: "download_installer",
              state: "Completed",
              started_at: null,
              completed_at: null,
            },
            {
              step_index: 1,
              step_kind: "install_smapi_files",
              state: "Running",
              started_at: null,
              completed_at: null,
            },
          ],
        }}
      />,
    );
    expect(screen.getByText(/Setting up SMAPI/)).toBeInTheDocument();
    expect(
      screen.getByText("Download installer: finished"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Install smapi files: was running when it stopped"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry recovery" }));
    expect(retry).toHaveBeenCalled();
  });

  it("lets the user continue while saying what stays blocked", () => {
    const go = vi.fn();
    render(
      <RecoveryScreen
        summary="needs recovery"
        error={null}
        onRetry={vi.fn()}
        onContinue={go}
        detail={null}
      />,
    );
    expect(
      screen.getByText(/refused until recovery finishes/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Continue to the app" }),
    );
    expect(go).toHaveBeenCalled();
  });
});
