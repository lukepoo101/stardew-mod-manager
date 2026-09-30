import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FreezeCard } from "@/features/profiles/FreezeCard";
import { api } from "@/shared/api/client";
import { freezeDrift } from "@/shared/profiles/freezeDrift";
import type {
  FrozenModDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const frozen = (id: string, enabled = true): FrozenModDto => ({
  unique_id: id,
  name: id,
  version: "1.0",
  artifact_hash: "h",
  enabled,
});
const current = (id: string, enabled = true) =>
  ({ unique_id: id, name: id, enabled }) as ModListItemDto;

describe("profile freeze", () => {
  it("reports drift from the frozen snapshot", () => {
    expect(
      freezeDrift(
        [frozen("A"), frozen("B"), frozen("Gone")],
        [current("a"), current("B", false), current("New")],
      ),
    ).toEqual({
      enabledChanged: [{ name: "B", nowEnabled: false }],
      added: ["New"],
      removed: ["Gone"],
    });
  });

  it("freezes with a reason and shows the frozen state", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([current("A")]);
    const get = vi.spyOn(api, "getProfileFreeze").mockResolvedValue(null);
    const freeze = vi.spyOn(api, "freezeProfile").mockResolvedValue({
      profile_id: "p1",
      frozen_at: "2026-09-01T10:00:00Z",
      reason: "Co-op",
      mods: [frozen("A")],
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <FreezeCard />
      </QueryClientProvider>,
    );
    fireEvent.change(await screen.findByLabelText("Reason (optional)"), {
      target: { value: "Co-op" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Freeze" }));
    await waitFor(() => expect(freeze).toHaveBeenCalledWith("p1", "Co-op"));
    expect(get).toHaveBeenCalled();
  });

  it("shows the freeze, whether anything drifted, and unfreezes", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([current("A", false)]);
    vi.spyOn(api, "getProfileFreeze").mockResolvedValue({
      profile_id: "p1",
      frozen_at: "2026-09-01T10:00:00Z",
      reason: "Co-op",
      mods: [frozen("A")],
    });
    const unfreeze = vi.spyOn(api, "unfreezeProfile").mockResolvedValue();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <FreezeCard />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("A is now disabled")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Unfreeze" }));
    await waitFor(() => expect(unfreeze).toHaveBeenCalledWith("p1"));
  });
});
