import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { BundleCard } from "@/features/profiles/BundleCard";
import { api } from "@/shared/api/client";
import { normalizeApiError } from "@/shared/api/errors";
import type {
  BundleComponentDto,
  BundlePreviewDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  compareWithRecipe,
  differenceKey,
  parseRecipe,
  type ProfileRecipe,
  supportedOn,
} from "@/shared/recipe/recipe";
import { redactSettings } from "@/shared/recipe/settingsPrivacy";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

const component = (
  unique_id: string,
  extra: Partial<BundleComponentDto> = {},
): BundleComponentDto => ({
  unique_id,
  name: unique_id,
  version: "1.0.0",
  enabled: true,
  package_included: true,
  optional: false,
  platforms: [],
  not_for_this_computer: false,
  shares_package_with: [],
  left_out_reason: null,
  ...extra,
});
const preview = (components: BundleComponentDto[]): BundlePreviewDto => ({
  settings_for: [],
  operating_system: "linux",
  bundle_digest: "f".repeat(64),
  unresolved_requirements: [],
  locally_modified: [],
  frozen_at: null,
  frozen_reason: null,
  profile_name: "Seasons",
  generated_at: "",
  components,
  missing_packages: components
    .filter((c) => c.left_out_reason)
    .map((c) => c.name),
  warnings: [],
});

function renderCard() {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p", name: "Main", revision: 1 },
    game: { id: "g1" },
  } as unknown as ProfileOverviewDto);
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <BundleCard />
    </QueryClientProvider>,
  );
}

describe("bundle packages that hold more than one mod", () => {
  it("says an optional mod comes with a required one in its package, and sends the previewed digest", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue(
      preview([
        component("Cozy.SeasonalA", {
          shares_package_with: ["Cozy.SeasonalB"],
        }),
        component("Cozy.SeasonalB", {
          optional: true,
          shares_package_with: ["Cozy.SeasonalA"],
        }),
      ]),
    );
    const importBundle = vi
      .spyOn(api, "importProfileBundle")
      .mockResolvedValue({
        profile_id: "new",
        profile_name: "Seasons",
        installed: ["Cozy.SeasonalA 1.0.0", "Cozy.SeasonalB 1.0.0"],
        disabled: [],
        failures: [],
        settings_applied: [],
        declined_optional: [],
        reference_attached: true,
        not_for_this_computer: [],
        installed_with_package: ["Cozy.SeasonalB"],
      });
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    const optional = await screen.findByRole("checkbox", {
      name: "Cozy.SeasonalB 1.0.0",
    });
    // It cannot be left out, and the preview says so rather than pretend.
    expect(optional).toBeChecked();
    expect(optional).toBeDisabled();
    expect(
      screen.getByText(/comes in one package with Cozy.SeasonalA/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Create profile and install" }),
    );
    await waitFor(() => expect(importBundle).toHaveBeenCalled());
    expect(importBundle.mock.calls[0][5]).toBe("f".repeat(64));
    expect(
      await screen.findByText(/installed because each comes in one package/),
    ).toBeInTheDocument();
  });

  it("does not say an optional mod comes with a package the bundle does not carry", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue(
      preview([
        component("Cozy.SeasonalA", {
          package_included: false,
          shares_package_with: ["Cozy.SeasonalB"],
        }),
        component("Cozy.SeasonalB", {
          optional: true,
          package_included: false,
          shares_package_with: ["Cozy.SeasonalA"],
        }),
      ]),
    );
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    const optional = await screen.findByRole("checkbox", {
      name: /Cozy.SeasonalB 1.0.0/,
    });
    expect(optional).not.toBeDisabled();
    expect(screen.queryByText(/which is installed/)).toBeNull();
  });

  it("lists a mod sharing a package with one for another system as left out, and asks before going on", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue(
      preview([
        component("P.Win", {
          platforms: ["windows"],
          not_for_this_computer: true,
          shares_package_with: ["P.Any"],
        }),
        component("P.Any", {
          shares_package_with: ["P.Win"],
          left_out_reason:
            "It comes in one package with P.Win, which is not for this computer.",
        }),
      ]),
    );
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    expect(
      await screen.findByText(
        /P.Any 1.0.0: It comes in one package with P.Win/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Create profile and install" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("checkbox", {
        name: /Create the profile without P.Any/,
      }),
    ).toBeInTheDocument();
  });

  it("drops the preview when the file changed since it was read", async () => {
    vi.spyOn(api, "chooseBundleDialog").mockResolvedValue("/b.zip");
    vi.spyOn(api, "inspectProfileBundle").mockResolvedValue(
      preview([component("A.Mod")]),
    );
    vi.spyOn(api, "importProfileBundle").mockRejectedValue(
      normalizeApiError({
        code: "BUNDLE_CHANGED",
        category: "validation",
        summary:
          "The bundle file has changed since it was read, so nothing was imported. Read it again to see what it holds now.",
        technical_details: null,
        context: null,
        recoverability: "terminal",
        operation_id: null,
      }),
    );
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Import bundle..." }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Create profile and install" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /changed since it was read/,
    );
    expect(
      screen.queryByRole("button", { name: "Create profile and install" }),
    ).toBeNull();
  });
});

describe("accepted differences and package files", () => {
  const recipe = (hash: string): ProfileRecipe => ({
    schema: "stardew-mod-manager.profile-recipe",
    schema_version: 1,
    generated_at: "",
    profile_name: "Group",
    game: { storefront: "steam", smapi_version: null },
    components: [
      {
        unique_id: "T.Mod",
        name: "T",
        author: "a",
        version: "1.0.0",
        enabled: true,
        artifact_hash: hash,
        optional: false,
      },
    ],
  });
  const mine = [
    {
      profile_component_id: "c1",
      unique_id: "T.Mod",
      name: "T",
      author: "a",
      version: "1.0.0",
      enabled: true,
      artifact_hash: "b".repeat(64),
    } as ModListItemDto,
  ];

  it("names both files, so an acceptance returns when the group's file changes", () => {
    const before = differenceKey(
      compareWithRecipe(mine, recipe("a".repeat(64))).differences[0],
    );
    const after = differenceKey(
      compareWithRecipe(mine, recipe("c".repeat(64))).differences[0],
    );
    expect(before).not.toBe(after);
    expect(
      differenceKey(
        compareWithRecipe([], recipe("A".repeat(64))).differences[0],
      ),
    ).toBe(`missing:T.Mod:1.0.0::${"a".repeat(64)}:`);
  });
});

describe("settings redaction in collections", () => {
  it("never shares a repeated, wrapped or numeric secret, or an AuthKey", () => {
    for (const [file, secret] of [
      ['{"ApiToken": "ghp_secret1", "ApiToken": ""}', "ghp_secret1"],
      ['{"ApiToken": {"Value": "ghp_secret2"}}', "ghp_secret2"],
      ['{"Password": 123456}', "123456"],
      ['{"AuthKey": "ak_secret3"}', "ak_secret3"],
    ] as const)
      expect(redactSettings(file, true).content).not.toContain(secret);
    // Unflagged files are shared exactly as they are.
    const plain = '{\n  "ToggleKey": "F5"\n}';
    expect(redactSettings(plain, false).content).toBe(plain);
  });
});

describe("recipe checks shared with the backend", () => {
  it("refuses a forked_from author that is not text", () => {
    const text = JSON.stringify({
      schema: "stardew-mod-manager.profile-recipe",
      schema_version: 1,
      profile_name: "x",
      components: [],
      collection: {
        id: "c",
        name: "C",
        revision: 1,
        forked_from: { id: "o", revision: 1, name: "O", author: 5 },
      },
    });
    expect(parseRecipe(text).ok).toBe(false);
  });

  it("does not take a mod marked for some systems as working on an unknown one", () => {
    const marked = { platforms: ["windows"] } as ProfileRecipe["components"][0];
    expect(supportedOn(marked, undefined)).toBe(false);
    expect(supportedOn({} as ProfileRecipe["components"][0], undefined)).toBe(
      true,
    );
  });
});
