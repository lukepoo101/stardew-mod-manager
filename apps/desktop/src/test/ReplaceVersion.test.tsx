import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { api } from "@/shared/api/client";
import type { OperationPreviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const preview = (direction: string, incoming: string): OperationPreviewDto => ({
  operation_id: "op-1",
  artifact_hash: "hash-new",
  original_filename: "V.Mod.zip",
  byte_size: 1,
  detected_components: [
    {
      unique_id: "V.Mod",
      name: "V Mod",
      author: "A",
      version: incoming,
      description: null,
      relative_root: "V.Mod",
    },
  ],
  dependencies_satisfied: false,
  warnings: [],
  blockers: ["A mod with this UniqueID is already installed in this profile"],
  affected_profile_component_ids: [],
  expected_profile_revision: 1,
  replaces: [
    {
      profile_component_id: "c1",
      unique_id: "V.Mod",
      name: "V Mod",
      installed_version: "1.0.0",
      incoming_version: incoming,
      direction,
    },
  ],
});

async function open(p: OperationPreviewDto) {
  vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(p);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <ProfileModInstaller profileId="p1" />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  fireEvent.change(screen.getByPlaceholderText(/Or paste path/i), {
    target: { value: "/tmp/V.Mod.zip" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
  await screen.findByText("Review mod installation");
}

describe("replacing an installed version", () => {
  it("offers an upgrade instead of refusing a duplicate", async () => {
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const replace = vi.spyOn(api, "replaceModVersion").mockResolvedValue({
      replaced: preview("upgrade", "2.0.0").replaces,
      kept_settings: ["config.json"],
      left_disabled: false,
    });
    await open(preview("upgrade", "2.0.0"));
    expect(screen.getByText(/1.0.0 → 2.0.0 \(newer\)/)).toBeInTheDocument();
    expect(screen.queryByText(/already installed/)).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Replace installed version" }),
    );
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("op-1"));
    await waitFor(() => expect(replace).toHaveBeenCalledWith("p1", "hash-new"));
    expect(
      await screen.findByText(
        /Replaced V Mod 1.0.0 with 2.0.0. Your settings were kept./,
      ),
    ).toBeInTheDocument();
  });

  it("needs an explicit confirmation for an older version", async () => {
    await open(preview("downgrade", "0.9.0"));
    const button = screen.getByRole("button", {
      name: "Install older version",
    });
    expect(button).toBeDisabled();
    fireEvent.click(screen.getByLabelText("I want the older version"));
    expect(button).toBeEnabled();
  });
});
