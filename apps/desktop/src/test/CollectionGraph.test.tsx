import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CollectionGraph } from "@/features/profiles/CollectionGraph";
import type { DependencyMapEntryDto } from "@/shared/api/generated";
import { brokenWithout, cycles, neighbourhood } from "@/shared/recipe/graph";
import type { ProfileRecipe } from "@/shared/recipe/recipe";

const node = (
  id: string,
  requires: [string, string, string][] = [],
): DependencyMapEntryDto =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    version: "1.0",
    enabled: true,
    required_by: [],
    requires: requires.map(([to, kind, status]) => ({
      unique_id: to,
      name: to,
      installed_version: null,
      minimum_version: null,
      kind,
      status,
    })),
  }) as DependencyMapEntryDto;

const map = [
  node("Top", [
    ["Lib", "required", "satisfied"],
    ["Extra", "optional", "satisfied"],
  ]),
  node("Lib"),
  node("Pack", [["Opt", "content_pack_for", "satisfied"]]),
  node("Opt"),
  node("Gone", [["Missing.Mod", "required", "missing"]]),
  node("LoopA", [["LoopB", "required", "satisfied"]]),
  node("LoopB", [["LoopA", "required", "satisfied"]]),
];

describe("collection dependency graph", () => {
  it("finds loops, neighbourhoods and what declining optional mods breaks", () => {
    expect(cycles(map)).toEqual([["LoopA", "LoopB"]]);
    expect([...neighbourhood(map, "Lib")].sort()).toEqual(["lib", "top"]);
    expect(brokenWithout(map, new Set(["opt"]))).toEqual(["Pack"]);
  });

  it("shows link kinds and problems, filters and simulates declining", () => {
    const recipe = {
      components: [{ unique_id: "Opt", optional: true }],
    } as unknown as ProfileRecipe;
    render(<CollectionGraph recipe={recipe} map={map} />);
    expect(screen.getByText("needs Lib")).toBeInTheDocument();
    expect(screen.getByText("can use Extra")).toBeInTheDocument();
    expect(screen.getByText("is a content pack for Opt")).toBeInTheDocument();
    expect(screen.getByText("needs Missing.Mod (missing)")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("LoopA → LoopB");
    fireEvent.click(
      screen.getByLabelText(/Show what breaks if a recipient declines/),
    );
    expect(
      screen.getByText("These would stop loading: Pack."),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Focus on a mod"), {
      target: { value: "Lib" },
    });
    expect(screen.queryByText("Pack", { selector: "span" })).toBeNull();
    expect(screen.getByText("Top", { selector: "span" })).toBeInTheDocument();
  });
});
