import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StorageUsageCard } from "@/features/settings/StorageUsageCard";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("storage usage", () => {
  it("shows sizes per profile and area, and unreadable ones as unknown", async () => {
    vi.spyOn(api, "getStorageUsage").mockResolvedValue({
      profiles: [
        {
          profile_id: "p1",
          name: "Main",
          archived: false,
          live_bytes: 2048,
          disabled_bytes: 0,
          operations_bytes: null,
        },
      ],
      packages_bytes: 5 * 1024 * 1024,
      installer_cache_bytes: 0,
      save_backups_bytes: 1024,
      trash_bytes: 0,
    });
    render(<StorageUsageCard />);
    fireEvent.click(screen.getByRole("button", { name: "Measure" }));
    expect(await screen.findByText("Main")).toBeInTheDocument();
    expect(screen.getByText("2.0 KB")).toBeInTheDocument();
    expect(screen.getByText("could not be read")).toBeInTheDocument();
    expect(screen.getByText("5.0 MB")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /remove|delete/i })).toBeNull();
  });
});
