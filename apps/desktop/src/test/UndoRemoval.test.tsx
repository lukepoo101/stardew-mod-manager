import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OperationDetails } from "@/features/activity/OperationDetails";
import {
  UndoRemovalButton,
  undoOutcome,
} from "@/features/activity/UndoRemoval";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

const details = {
  operation_id: "op",
  profile_name: "Main",
  original_filename: "Lib.zip",
  package_hash: "a".repeat(64),
  folder: "Lib",
  changes: [
    { change: "removed", name: "Lib", unique_id: "Z.Lib", version: "1.0.0" },
  ],
  steps: [],
};

describe("undoing a removal", () => {
  it("installs the removed mod again with its settings after asking", async () => {
    vi.spyOn(api, "getOperationHistoryDetails").mockResolvedValue(details);
    vi.spyOn(api, "storedPackages").mockResolvedValue(["a".repeat(64)]);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const undo = vi.spyOn(api, "undoRemoval").mockResolvedValue({
      installed: ["Lib"],
      settings_restored: 1,
      settings_problem: null,
    });
    render(<OperationDetails operationId="op" undoRemovalInto="p1" />);
    const summary = screen.getByText("What changed");
    (summary.parentElement as HTMLDetailsElement).open = true;
    fireEvent(summary.parentElement as HTMLElement, new Event("toggle"));
    fireEvent.click(
      await screen.findByRole("button", { name: "Install it again" }),
    );
    await waitFor(() => expect(undo).toHaveBeenCalledWith("op"));
    expect(confirm.mock.calls[0][0]).toMatch(/settings it had .* brought back/);
    expect(
      await screen.findByText(/Installed again: Lib, with the settings it had/),
    ).toBeInTheDocument();
  });

  it("says when the archive is gone", async () => {
    vi.spyOn(api, "getOperationHistoryDetails").mockResolvedValue(details);
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    render(<OperationDetails operationId="op" undoRemovalInto="p1" />);
    const summary = screen.getByText("What changed");
    (summary.parentElement as HTMLDetailsElement).open = true;
    fireEvent(summary.parentElement as HTMLElement, new Event("toggle"));
    expect(
      await screen.findByText(
        /no longer stored, so this removal cannot be undone/,
      ),
    ).toBeInTheDocument();
  });

  it("is offered straight from the removal result, without hunting in Activity", async () => {
    const undo = vi.spyOn(api, "undoRemoval").mockResolvedValue({
      installed: ["Lib"],
      settings_restored: 0,
      settings_problem:
        "The mod is back, but its settings were not restored (disk full). They are still in the profile's recovery folder.",
    });
    render(<UndoRemovalButton operationId="op" />);
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(undo).toHaveBeenCalledWith("op"));
    expect(
      await screen.findByText(/settings were not restored \(disk full\)/),
    ).toBeInTheDocument();
  });

  it("describes each outcome plainly", () => {
    expect(
      undoOutcome({
        installed: ["Lib"],
        settings_restored: 0,
        settings_problem: null,
      }),
    ).toBe("Installed again: Lib. It had no settings file to bring back.");
  });
});
