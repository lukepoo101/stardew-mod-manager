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

describe("resolving reference differences", () => {
  it("replaces a version with the group's stored package after confirming", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue(["a".repeat(64)]);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const replace = vi.spyOn(api, "replaceModVersion").mockResolvedValue({
      replaced: [],
      kept_settings: [],
      left_disabled: false,
      settings_backup: null,
      restore_point: null,
    });
    renderCard([]);
    fireEvent.click(
      await screen.findByRole("button", { name: "Use the group's version" }),
    );
    await waitFor(() =>
      expect(replace).toHaveBeenCalledWith("p1", "a".repeat(64)),
    );
  });

  it("says to get the file when the group's package is not stored", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    renderCard([]);
    expect(
      await screen.findByText(/Get A Mod 1\.0\.0 to match/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Use the group's version" }),
    ).toBeNull();
  });

  it("does nothing when the change is not confirmed", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue(["a".repeat(64)]);
    vi.spyOn(window, "confirm").mockReturnValue(false);
    const replace = vi.spyOn(api, "replaceModVersion");
    renderCard([]);
    fireEvent.click(
      await screen.findByRole("button", { name: "Use the group's version" }),
    );
    expect(replace).not.toHaveBeenCalled();
  });
});

describe("supplying a downloaded file for a reference difference", () => {
  const inspected = (hash: string, version: string, uniqueId = "A.Mod") => ({
    operation_id: "op1",
    artifact_hash: hash,
    original_filename: "a.zip",
    byte_size: 1,
    detected_components: [
      {
        unique_id: uniqueId,
        name: "A Mod",
        author: "a",
        version,
        description: null,
        relative_root: "A",
      },
    ],
    dependencies_satisfied: true,
    warnings: [],
    blockers: [],
    affected_profile_component_ids: [],
    expected_profile_revision: null,
    replaces: [],
  });

  it("uses the exact file without asking", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValue("/dl/a.zip");
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(
      inspected("a".repeat(64), "1.0.0"),
    );
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const confirm = vi.spyOn(window, "confirm");
    const replace = vi.spyOn(api, "replaceModVersion").mockResolvedValue({
      replaced: [],
      kept_settings: [],
      left_disabled: false,
      settings_backup: null,
      restore_point: null,
    });
    renderCard([]);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Choose the downloaded file...",
      }),
    );
    await waitFor(() =>
      expect(replace).toHaveBeenCalledWith("p1", "a".repeat(64)),
    );
    expect(confirm).not.toHaveBeenCalled();
    expect(cancel).toHaveBeenCalledWith("op1");
  });

  it("asks before using a different version, and stops if declined", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValue("/dl/a.zip");
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(
      inspected("c".repeat(64), "1.2.0"),
    );
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const replace = vi.spyOn(api, "replaceModVersion");
    renderCard([]);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Choose the downloaded file...",
      }),
    );
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("op1"));
    expect(confirm.mock.calls[0][0]).toMatch(
      /has A Mod 1\.2\.0; the group uses 1\.0\.0/,
    );
    expect(replace).not.toHaveBeenCalled();
  });

  it("refuses a file that does not contain the mod", async () => {
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValue("/dl/other.zip");
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(
      inspected("d".repeat(64), "1.0.0", "Other.Mod"),
    );
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    renderCard([]);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Choose the downloaded file...",
      }),
    );
    expect(
      await screen.findByText("That file does not contain A.Mod."),
    ).toBeInTheDocument();
    expect(cancel).toHaveBeenCalledWith("op1");
  });
});
