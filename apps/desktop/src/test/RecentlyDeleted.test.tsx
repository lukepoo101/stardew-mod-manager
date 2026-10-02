import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RecentlyDeleted } from "@/features/profiles/RecentlyDeleted";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("recently deleted profiles", () => {
  it("brings one back and reports what came back", async () => {
    vi.spyOn(api, "listDeletedProfiles")
      .mockResolvedValueOnce([
        {
          entry: "profile-abc-20261001T100000",
          name: "Old farm",
          deleted_at: "2026-10-01T10:00:00Z",
          mod_count: 3,
        },
      ])
      .mockResolvedValue([]);
    const bring = vi.spyOn(api, "bringBackProfile").mockResolvedValue({
      profile_id: "p9",
      profile_name: "Old farm",
      installed: ["A 1.0", "B 1.0"],
      disabled: [],
      failures: [{ name: "C", reason: "gone" }],
      settings_applied: [],
      declined_optional: [],
      reference_attached: false,
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RecentlyDeleted />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Bring back" }));
    await waitFor(() =>
      expect(bring).toHaveBeenCalledWith("profile-abc-20261001T100000"),
    );
    expect(
      await screen.findByText(
        /Brought back "Old farm" with 2 mod\(s\); not brought back: C/,
      ),
    ).toBeInTheDocument();
  });

  it("is hidden when nothing was deleted", async () => {
    const list = vi.spyOn(api, "listDeletedProfiles").mockResolvedValue([]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RecentlyDeleted />
      </QueryClientProvider>,
    );
    await waitFor(() => expect(list).toHaveBeenCalled());
    expect(screen.queryByText("Recently deleted")).toBeNull();
  });
});
