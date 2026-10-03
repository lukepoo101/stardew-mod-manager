import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReferenceCard } from "@/features/profiles/ReferenceCard";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { buildRecipe, parseRecipe } from "@/shared/recipe/recipe";

afterEach(() => vi.restoreAllMocks());

const overview = {
  profile: { id: "p1", name: "Mine" },
  game: { storefront: "steam" },
  smapi_status: { observed_version: null },
} as unknown as ProfileOverviewDto;

describe("provenance in recipes", () => {
  it("carries update keys, requirements and the user's source link", () => {
    const recipe = buildRecipe(
      overview,
      [
        {
          unique_id: "A.Mod",
          name: "A",
          author: "Ann",
          version: "1.0",
          enabled: true,
          artifact_hash: "a".repeat(64),
          update_keys: ["Nexus:1915"],
          requires: ["Core.Lib"],
        } as ModListItemDto,
      ],
      "now",
      new Map([["a.mod", "https://example.com/a"]]),
    );
    const parsed = parseRecipe(JSON.stringify(recipe));
    expect(parsed.ok && parsed.recipe.components[0]).toMatchObject({
      update_keys: ["Nexus:1915"],
      requires: ["Core.Lib"],
      source_url: "https://example.com/a",
    });
    const bad = parseRecipe(
      JSON.stringify({
        ...recipe,
        components: [{ ...recipe.components[0], source_url: "javascript:x" }],
      }),
    );
    expect(bad.ok).toBe(false);
  });

  it("shows recipients who made a recommended mod, where it is published and what else it needs", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
      recipe_json: JSON.stringify({
        schema: "stardew-mod-manager.profile-recipe",
        schema_version: 1,
        generated_at: "",
        profile_name: "Cozy",
        game: { storefront: "steam", smapi_version: null },
        components: [
          {
            unique_id: "Opt.Mod",
            name: "Opt",
            author: "Ann",
            version: "1.0",
            enabled: true,
            artifact_hash: "a".repeat(64),
            optional: true,
            update_keys: ["Nexus:42"],
            requires: ["Core.Lib"],
            source_url: "https://example.com/opt",
          },
          {
            unique_id: "Anon.Mod",
            name: "Anon",
            author: "",
            version: "1.0",
            enabled: true,
            artifact_hash: "b".repeat(64),
            optional: true,
          },
        ],
      }),
      attached_at: "2026-10-01T00:00:00Z",
      accepted: [],
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ReferenceCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/Also needs Core.Lib \(from its manifest\)/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Published at Nexus:42 \(as its manifest says\)/),
    ).toBeInTheDocument();
    expect(screen.getByText("https://example.com/opt")).toBeInTheDocument();
    expect(screen.getByText(/By an unknown author/)).toBeInTheDocument();
    expect(
      screen.getByText(/Where it is published is not recorded/),
    ).toBeInTheDocument();
  });
});
