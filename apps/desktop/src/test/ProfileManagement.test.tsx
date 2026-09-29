import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ProfileDetailsForm } from "@/features/profiles/ProfileDetailsForm";
import { api } from "@/shared/api/client";
import { compareProfiles } from "@/shared/profiles/compare";
import { compareVersions } from "@/shared/versions";
import type { ModListItemDto, ProfileSummaryDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const mod = (
  id: string,
  version: string,
  over: Partial<ModListItemDto> = {},
): ModListItemDto =>
  ({
    profile_component_id: `${id}-${version}`,
    unique_id: id,
    name: id,
    author: "a",
    version,
    description: null,
    enabled: true,
    installed_reason: "explicit",
    deployment_id: "d",
    artifact_hash: `hash-${id}`,
    installed_at: "",
    ...over,
  }) as ModListItemDto;

describe("comparing two profiles", () => {
  it.each([
    ["1.0", "1.0.0", 0],
    ["1.2", "1.10", -1],
    ["2.0", "1.9.9", 1],
  ])("orders versions %s and %s", (a, b, expected) => {
    expect(compareVersions(a, b)).toBe(expected);
  });

  it("classifies every kind of difference and counts identical mods", () => {
    const result = compareProfiles(
      [
        mod("Same", "1.0"),
        mod("OnlyA", "1.0"),
        mod("Ver", "1.2"),
        mod("Pkg", "1.0", { artifact_hash: "one" }),
        mod("Toggle", "1.0"),
      ],
      [
        mod("same", "1.0.0", { artifact_hash: "hash-Same" }),
        mod("OnlyB", "2.0"),
        mod("Ver", "1.10"),
        mod("Pkg", "1.0", { artifact_hash: "two" }),
        mod("Toggle", "1.0", { enabled: false }),
      ],
    );
    expect(result.identical).toBe(1);
    expect(result.differences.map((d) => d.kind)).toEqual([
      "only_a",
      "only_b",
      "version",
      "package",
      "enabled",
    ]);
    const version = result.differences.find((d) => d.kind === "version");
    expect(version && "newer" in version && version.newer).toBe("b");
  });
});

describe("renaming a profile", () => {
  const profile = {
    id: "p1",
    name: "Main",
    description: null,
  } as unknown as ProfileSummaryDto;

  function renderForm(onDone = vi.fn()) {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ProfileDetailsForm profile={profile} onDone={onDone} />
      </QueryClientProvider>,
    );
    return onDone;
  }

  it("saves the new name and description", async () => {
    const update = vi
      .spyOn(api, "updateProfileDetails")
      .mockResolvedValue({ ...profile, name: "Co-op" });
    const onDone = renderForm();
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Co-op" },
    });
    fireEvent.change(screen.getByLabelText("Description (optional)"), {
      target: { value: "With Sam" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith("p1", "Co-op", "With Sam"),
    );
    await waitFor(() => expect(onDone).toHaveBeenCalled());
  });

  it("keeps the form open and shows why a save failed", async () => {
    vi.spyOn(api, "updateProfileDetails").mockRejectedValue(
      new Error("A profile named 'Other' already exists"),
    );
    const onDone = renderForm();
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Other" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(onDone).not.toHaveBeenCalled();
  });
});
