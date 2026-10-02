import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ExperimentChanges,
  experimentChanges,
} from "@/features/profiles/ExperimentChanges";
import { api } from "@/shared/api/client";
import type { ModListItemDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const mod = (id: string, hash: string, enabled = true, version = "1.0") =>
  ({
    profile_component_id: `c-${id}-${hash}`,
    unique_id: id,
    name: id,
    version,
    enabled,
    artifact_hash: hash,
  }) as ModListItemDto;

describe("experiment changes", () => {
  it("tells added, removed, version and enabled changes apart", () => {
    const changes = experimentChanges(
      [mod("Keep", "a"), mod("Old", "b"), mod("Ver", "c"), mod("Flip", "d")],
      [
        mod("keep", "a"),
        mod("New", "e"),
        mod("Ver", "f", true, "2.0"),
        mod("Flip", "d", false),
      ],
    );
    expect(changes.map((c) => c.kind).sort()).toEqual([
      "added",
      "enabled",
      "removed",
      "version",
    ]);
  });

  it("applies one change to the original only after confirming", async () => {
    vi.spyOn(api, "listProfileMods").mockImplementation(async (id) =>
      id === "src" ? [mod("Ver", "c")] : [mod("Ver", "f", true, "2.0")],
    );
    const replace = vi
      .spyOn(api, "replaceModVersion")
      .mockResolvedValue({} as never);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ExperimentChanges
          sourceId="src"
          sourceName="Main"
          experimentId="exp"
        />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Ver is 2.0, was 1.0")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: 'Apply to "Main"' }));
    expect(confirm.mock.calls[0][0]).toMatch(/from 1.0 to 2.0/);
    await waitFor(() => expect(replace).toHaveBeenCalledWith("src", "f"));
  });
});
