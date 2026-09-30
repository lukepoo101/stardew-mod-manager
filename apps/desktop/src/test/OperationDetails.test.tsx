import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OperationDetails } from "@/features/activity/OperationDetails";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("operation history details", () => {
  it("loads what an operation changed when opened", async () => {
    const load = vi.spyOn(api, "getOperationHistoryDetails").mockResolvedValue({
      operation_id: "o1",
      profile_name: "Main",
      original_filename: "Pack.zip",
      package_hash: "abc",
      folder: "Pack",
      changes: [
        { change: "added", name: "Core", unique_id: "A.Core", version: "1.0" },
        { change: "removed", name: null, unique_id: null, version: null },
      ],
    });
    const { container } = render(<OperationDetails operationId="o1" />);
    expect(load).not.toHaveBeenCalled();
    const details = container.querySelector("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    expect(await screen.findByText("From: Pack.zip")).toBeInTheDocument();
    expect(screen.getByText(/Installed: Core 1.0/)).toBeInTheDocument();
    expect(screen.getByText(/Removed: name not recorded/)).toBeInTheDocument();
    expect(load).toHaveBeenCalledWith("o1");
  });
});
