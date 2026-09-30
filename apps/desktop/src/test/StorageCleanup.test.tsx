import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  StorageCleanupCard,
  formatBytes,
} from "@/features/settings/StorageCleanupCard";
import { api } from "@/shared/api/client";
import type { CleanupPreviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const preview: CleanupPreviewDto = {
  items: [
    {
      id: "package:aaa",
      category: "unused_package",
      label: "Package aaa",
      detail: "Not used by any profile.",
      size_bytes: 2048,
      removable: true,
    },
    {
      id: "smapi-cache:extracted_installer",
      category: "cache",
      label: "SMAPI installer files (extracted_installer)",
      detail: "Downloaded again.",
      size_bytes: 1024,
      removable: true,
    },
    {
      id: "package:bbb",
      category: "protected",
      label: "Package bbb",
      detail: "Used by Co-op.",
      size_bytes: 4096,
      removable: false,
    },
  ],
  reclaimable_bytes: 3072,
  protected_bytes: 4096,
  blocked_reason: null,
};

describe("storage cleanup", () => {
  it("formats sizes", () => {
    expect(formatBytes(12)).toBe("12 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(50 * 1024 * 1024)).toBe("50 MB");
  });

  it("shows what is kept and why, and removes only the chosen categories", async () => {
    vi.spyOn(api, "getCleanupPreview").mockResolvedValue(preview);
    const run = vi.spyOn(api, "runCleanup").mockResolvedValue({
      outcomes: [
        {
          id: "package:aaa",
          label: "Package aaa",
          outcome: "removed",
          message: null,
          size_bytes: 2048,
        },
      ],
      reclaimed_bytes: 2048,
      complete: true,
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<StorageCleanupCard />);
    fireEvent.click(screen.getByRole("button", { name: "Check storage" }));
    await screen.findByText("Can be removed: 3.0 KB");
    expect(screen.getByText("Used by Co-op.")).toBeTruthy();

    // Opt out of the installer cache: only the unused package is requested.
    fireEvent.click(screen.getByLabelText("Downloaded installers"));
    fireEvent.click(screen.getByRole("button", { name: /Remove selected/ }));
    await waitFor(() => expect(run).toHaveBeenCalledWith(["package:aaa"]));
    await screen.findByText("Freed 2.0 KB.");
  });

  it("reports items that were not removed", async () => {
    vi.spyOn(api, "getCleanupPreview").mockResolvedValue(preview);
    vi.spyOn(api, "runCleanup").mockResolvedValue({
      outcomes: [
        {
          id: "package:aaa",
          label: "Package aaa",
          outcome: "failed",
          message: "Could not remove the item",
          size_bytes: 2048,
        },
      ],
      reclaimed_bytes: 0,
      complete: false,
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<StorageCleanupCard />);
    fireEvent.click(screen.getByRole("button", { name: "Check storage" }));
    await screen.findByText("Can be removed: 3.0 KB");
    fireEvent.click(screen.getByRole("button", { name: /Remove selected/ }));
    await screen.findByText(/1 item\(s\) were not removed/);
    expect(
      screen.getByText("Package aaa: Could not remove the item"),
    ).toBeTruthy();
  });
});
