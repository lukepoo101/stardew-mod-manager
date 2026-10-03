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
      accepted_notes: {},
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

describe("exporting an incomplete profile", () => {
  it("names required mods its own reference asks for but it lacks", async () => {
    const { incompleteItems } = await import("@/shared/recipe/incomplete");
    const reference = {
      recipe_json: JSON.stringify({
        schema: "stardew-mod-manager.profile-recipe",
        schema_version: 1,
        generated_at: "",
        profile_name: "Group",
        game: { storefront: "steam", smapi_version: null },
        components: [
          {
            unique_id: "Need",
            name: "Need",
            author: "a",
            version: "1.0",
            enabled: true,
            artifact_hash: "a".repeat(64),
            optional: false,
          },
          {
            unique_id: "Opt",
            name: "Opt",
            author: "a",
            version: "1.0",
            enabled: true,
            artifact_hash: "b".repeat(64),
            optional: true,
          },
          {
            unique_id: "Left",
            name: "Left",
            author: "a",
            version: "1.0",
            enabled: true,
            artifact_hash: "c".repeat(64),
            optional: false,
          },
        ],
      }),
      attached_at: "",
      accepted: [`missing:Left:1.0:`],
      accepted_notes: {},
    };
    expect(incompleteItems(reference, []).map((i) => i.unique_id)).toEqual([
      "Need",
    ]);
  });
});

describe("a requirement that accepts newer versions", () => {
  it("offers the newest stored version that meets it, naming it first", async () => {
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
            unique_id: "Flex.Mod",
            name: "Flex",
            author: "a",
            version: "1.0",
            enabled: true,
            artifact_hash: "a".repeat(64),
            optional: false,
            version_rule: "at_least",
          },
        ],
      }),
      attached_at: "",
      accepted: [],
      accepted_notes: {},
    });
    vi.spyOn(api, "findStoredMod").mockResolvedValue([
      {
        artifact_hash: "h11",
        name: "Flex",
        version: "1.1",
        original_filename: "",
        meets_minimum: true,
      },
      {
        artifact_hash: "h12",
        name: "Flex",
        version: "1.2",
        original_filename: "",
        meets_minimum: true,
      },
      {
        artifact_hash: "h09",
        name: "Flex",
        version: "0.9",
        original_filename: "",
        meets_minimum: false,
      },
    ]);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const install = vi.spyOn(api, "installStoredPackage").mockResolvedValue();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ReferenceCard />
      </QueryClientProvider>,
    );
    const { fireEvent, waitFor } = await import("@testing-library/react");
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Install stored 1.2 (newer is allowed)",
      }),
    );
    await waitFor(() => expect(install).toHaveBeenCalledWith("p1", "h12"));
    expect(confirm.mock.calls[0][0]).toMatch(
      /Flex 1.2 .*asks for 1.0 or newer/,
    );
  });
});
