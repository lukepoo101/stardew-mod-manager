import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OperationDetails } from "@/features/activity/OperationDetails";
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
};

describe("undoing a removal", () => {
  it("installs the removed archive again after asking", async () => {
    vi.spyOn(api, "getOperationHistoryDetails").mockResolvedValue(details);
    vi.spyOn(api, "storedPackages").mockResolvedValue(["a".repeat(64)]);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const install = vi.spyOn(api, "installStoredPackage").mockResolvedValue();
    render(<OperationDetails operationId="op" undoRemovalInto="p1" />);
    const summary = screen.getByText("What changed");
    (summary.parentElement as HTMLDetailsElement).open = true;
    fireEvent(summary.parentElement as HTMLElement, new Event("toggle"));
    fireEvent.click(
      await screen.findByRole("button", { name: "Install it again" }),
    );
    await waitFor(() =>
      expect(install).toHaveBeenCalledWith("p1", "a".repeat(64)),
    );
    expect(await screen.findByText("Installed again.")).toBeInTheDocument();
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
});
