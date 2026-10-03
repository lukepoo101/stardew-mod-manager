import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LinkCheck } from "@/features/profiles/CollectionCard";
import { api } from "@/shared/api/client";
import type { ProfileRecipe } from "@/shared/recipe/recipe";

afterEach(() => vi.restoreAllMocks());

describe("checking download links", () => {
  it("checks each link when asked and says when, without condemning it", async () => {
    const check = vi.spyOn(api, "checkDownloadLinks").mockResolvedValue([
      {
        url: "https://forums.example.com/1",
        state: "unreachable",
        status: null,
        checked_at: "2026-10-03T10:00:00Z",
      },
      {
        url: "https://example.com/b",
        state: "reachable",
        status: 200,
        checked_at: "2026-10-03T10:00:00Z",
      },
    ]);
    const recipe = {
      components: [
        {
          name: "A",
          manual: { url: "https://forums.example.com/1", instructions: "" },
        },
        { name: "B", source_url: "https://example.com/b" },
      ],
    } as unknown as ProfileRecipe;
    render(<LinkCheck recipe={recipe} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Check 2 download link(s)" }),
    );
    expect(
      await screen.findByText(
        /A: https:\/\/forums.example.com\/1 did not answer/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/B: https:\/\/example.com\/b answers \(200\)/),
    ).toBeInTheDocument();
    expect(screen.getByText(/may answer later/)).toBeInTheDocument();
    expect(check).toHaveBeenCalledWith([
      "https://forums.example.com/1",
      "https://example.com/b",
    ]);
  });
});
