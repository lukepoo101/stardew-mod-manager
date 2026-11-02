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
      steps: [],
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

  it("lists each part of a batch with how it went", async () => {
    vi.spyOn(api, "getOperationHistoryDetails").mockResolvedValue({
      operation_id: "b1",
      profile_name: "Main",
      original_filename: null,
      package_hash: null,
      folder: null,
      changes: [],
      steps: [
        { label: "a.zip", state: "done", error: null },
        { label: "b.zip", state: "failed", error: "Needs Core" },
        { label: "c.zip", state: "not_started", error: null },
      ],
    });
    const { container } = render(<OperationDetails operationId="b1" />);
    const details = container.querySelector("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    expect(await screen.findByText("Each part")).toBeInTheDocument();
    expect(screen.getByText("a.zip: done")).toBeInTheDocument();
    expect(screen.getByText("b.zip: failed (Needs Core)")).toBeInTheDocument();
    expect(screen.getByText("c.zip: not started")).toBeInTheDocument();
    expect(screen.queryByText("No changes were recorded.")).toBeNull();
  });
});
