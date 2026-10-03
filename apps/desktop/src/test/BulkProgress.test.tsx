import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { BulkProgressIndicator } from "@/components/layout/BulkProgressIndicator";
import { bulkProgress, summarise } from "@/shared/progress/bulk";

afterEach(() => bulkProgress.clear());

describe("bulk progress", () => {
  it("tells partial from full success", () => {
    expect(summarise([{ name: "a", state: "done" }]).line).toBe("All 1 done");
    expect(
      summarise([
        { name: "a", state: "done" },
        { name: "b", state: "failed" },
        { name: "c", state: "cancelled" },
      ]).line,
    ).toBe("1 of 3 done; 1 failed, 1 not started");
    expect(
      summarise([
        { name: "a", state: "done" },
        { name: "b", state: "queued" },
      ]).line,
    ).toBe("1 of 2 handled");
  });

  it("shows the job in the sidebar on any page until dismissed", () => {
    render(<BulkProgressIndicator />);
    expect(screen.queryByText(/Installing/)).toBeNull();
    act(() => {
      bulkProgress.start("Installing 2 archive(s)", ["a.zip", "b.zip"]);
      bulkProgress.set(0, "done");
      bulkProgress.set(1, "running");
    });
    expect(screen.getByText("Installing 2 archive(s)")).toBeInTheDocument();
    expect(screen.getByText("1 of 2 handled: now b.zip")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
    act(() => {
      bulkProgress.set(1, "failed", "locked");
      bulkProgress.finish();
    });
    expect(screen.getByText("b.zip: failed (locked)")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText("Installing 2 archive(s)")).toBeNull();
  });
});
