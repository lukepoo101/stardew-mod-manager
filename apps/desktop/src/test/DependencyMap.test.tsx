import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DependencyMapCard } from "@/features/diagnostics/DependencyMapCard";
import { api } from "@/shared/api/client";
import type {
  DependencyMapEntryDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const req = (unique_id: string, status = "satisfied", kind = "required") => ({
  unique_id,
  name: unique_id,
  installed_version: status === "missing" ? null : "1.0",
  minimum_version: null,
  kind,
  status,
});

const entry = (
  unique_id: string,
  requires: ReturnType<typeof req>[],
  required_by: string[] = [],
): DependencyMapEntryDto => ({
  profile_component_id: `c-${unique_id}`,
  name: unique_id,
  unique_id,
  version: "1.0",
  enabled: true,
  requires,
  required_by,
});

function renderMap(entries: DependencyMapEntryDto[]) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "getDependencyMap").mockResolvedValue(entries);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DependencyMapCard />
    </QueryClientProvider>,
  );
}

describe("dependency map", () => {
  it("shows chains from top mods to frameworks and marks loops", async () => {
    renderMap([
      entry("Top", [req("Middle"), req("Nice", "missing", "optional")]),
      entry("Middle", [req("Base")], ["Top"]),
      entry("Base", [req("Middle")], ["Middle"]),
    ]);
    expect(await screen.findByText("Top 1.0")).toBeInTheDocument();
    expect(screen.getByText(/loops back to a mod above/)).toBeInTheDocument();
    // The optional link is shown, but not as a problem.
    expect(screen.getByText("Nice")).toBeInTheDocument();
  });

  it("can show only mods with an unmet requirement somewhere below", async () => {
    renderMap([
      entry("Fine", [req("Lib")]),
      entry("Lib", [], ["Fine"]),
      entry("Broken", [req("Middle")]),
      entry("Middle", [req("Gone", "missing")], ["Broken"]),
    ]);
    expect(await screen.findByText("Fine 1.0")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Only mods with a problem"));
    expect(screen.queryByText("Fine 1.0")).toBeNull();
    expect(screen.getByText("Broken 1.0")).toBeInTheDocument();
    expect(screen.getByText("Not installed")).toBeInTheDocument();
  });
});
