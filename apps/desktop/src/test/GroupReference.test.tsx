import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReferenceCard } from "@/features/profiles/ReferenceCard";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const recipe = JSON.stringify({
  schema: "stardew-mod-manager.profile-recipe",
  schema_version: 1,
  generated_at: "2026-09-01T00:00:00Z",
  profile_name: "Saturday co-op",
  game: { storefront: "Steam", smapi_version: null },
  components: [
    {
      unique_id: "A.Mod",
      name: "A Mod",
      author: "a",
      version: "1.0.0",
      enabled: true,
      artifact_hash: "a".repeat(64),
      optional: false,
    },
  ],
});

function renderCard(accepted: string[]) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listProfileMods").mockResolvedValue([
    {
      profile_component_id: "c1",
      unique_id: "A.Mod",
      name: "A Mod",
      author: "a",
      version: "1.1.0",
      enabled: true,
      artifact_hash: "b".repeat(64),
    } as ModListItemDto,
  ]);
  vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
    recipe_json: recipe,
    attached_at: "2026-09-02T00:00:00Z",
    accepted,
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ReferenceCard />
    </QueryClientProvider>,
  );
}

describe("group reference", () => {
  it("shows differences from the reference and can accept one", async () => {
    const accept = vi
      .spyOn(api, "setReferenceDifferenceAccepted")
      .mockResolvedValue({
        recipe_json: recipe,
        attached_at: "",
        accepted: [],
      });
    renderCard([]);
    expect(await screen.findByText("1 difference(s)")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(accept).toHaveBeenCalled());
    expect(accept.mock.calls[0][0]).toBe("p1");
    expect(accept.mock.calls[0][1]).toContain("A.Mod");
    expect(accept.mock.calls[0][2]).toBe(true);
  });

  it("treats accepted differences as in step until they change", async () => {
    renderCard(["version:A.Mod:1.0.0:1.1.0"]);
    expect(await screen.findByText("In step")).toBeInTheDocument();
    expect(screen.getByText("Accepted for this group (1)")).toBeInTheDocument();
  });
});
