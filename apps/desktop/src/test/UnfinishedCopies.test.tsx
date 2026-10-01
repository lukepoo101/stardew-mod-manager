import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { UnfinishedCopies } from "@/features/profiles/UnfinishedCopies";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

const renderPanel = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <UnfinishedCopies />
    </QueryClientProvider>,
  );

describe("unfinished copies", () => {
  it("is hidden when every copy finished", async () => {
    const list = vi.spyOn(api, "listUnfinishedCopies").mockResolvedValue([]);
    renderPanel();
    await waitFor(() => expect(list).toHaveBeenCalled());
    expect(screen.queryByText("Unfinished copies")).toBeNull();
  });

  it("lists an interrupted copy and finishes it", async () => {
    vi.spyOn(api, "listUnfinishedCopies")
      .mockResolvedValueOnce([
        {
          profile_id: "p2",
          profile_name: "Main copy",
          source_name: "Main",
          expected_mods: 12,
        },
      ])
      .mockResolvedValue([]);
    const finish = vi.spyOn(api, "finishProfileCopy").mockResolvedValue({
      profile_id: "p2",
      profile_name: "Main copy",
      installed: ["A 1.0", "B 1.0"],
      disabled: [],
      failures: [],
      settings_applied: [],
      declined_optional: [],
      reference_attached: false,
    });
    renderPanel();
    expect(
      await screen.findByText(/a copy of Main \(12 mod\(s\) expected\)/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Finish copying" }));
    await waitFor(() => expect(finish).toHaveBeenCalledWith("p2"));
    expect(
      await screen.findByText(/Finished "Main copy": 2 mod\(s\) installed now/),
    ).toBeInTheDocument();
  });
});
