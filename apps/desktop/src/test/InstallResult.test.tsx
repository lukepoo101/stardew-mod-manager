import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { api } from "@/shared/api/client";
import type { OperationDto, OperationPreviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const preview = (
  over: Partial<OperationPreviewDto> = {},
): OperationPreviewDto => ({
  operation_id: "operation-1",
  artifact_hash: "hash",
  original_filename: "Pack.zip",
  byte_size: 2048,
  detected_components: [
    {
      unique_id: "A.Core",
      name: "Core",
      author: "A",
      version: "1.2.0",
      description: null,
      relative_root: "Core",
    },
    {
      unique_id: "A.Extra",
      name: "Extra",
      author: "A",
      version: "1.2.0",
      description: null,
      relative_root: "Extra",
    },
  ],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  affected_profile_component_ids: [],
  expected_profile_revision: 1,
  files: [],
  replaces: [],
  ...over,
});

async function install(p: OperationPreviewDto) {
  vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(p);
  vi.spyOn(api, "executeOperation").mockResolvedValue({
    id: "operation-1",
    state: "succeeded",
  } as OperationDto);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <ProfileModInstaller profileId="profile-1" />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  fireEvent.change(screen.getByPlaceholderText(/Or paste path/i), {
    target: { value: "/tmp/Pack.zip" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
  fireEvent.click(await screen.findByRole("button", { name: "Install mod" }));
}

describe("install result", () => {
  it("lists every installed component of a multi-mod package", async () => {
    await install(preview());
    expect(await screen.findByText("Installed 2 mods")).toBeInTheDocument();
    expect(screen.getByText("A.Core")).toBeInTheDocument();
    expect(screen.getByText("A.Extra")).toBeInTheDocument();
    expect(
      screen.getByText(/which contained several mods/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/check these before playing/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByText("Installed 2 mods")).toBeNull();
  });

  it("keeps warnings separate from the success", async () => {
    await install(
      preview({
        warnings: ["Needs SMAPI 4.2 or newer"],
      }),
    );
    expect(await screen.findByText("Installed 2 mods")).toBeInTheDocument();
    expect(screen.getByText(/check these before playing/)).toBeInTheDocument();
    expect(screen.getByText("Needs SMAPI 4.2 or newer")).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: "View in Activity" }),
    ).toHaveAttribute("href", "/app/activity");
  });
});

describe("install review files", () => {
  it("lists every file with its size and whether it is installed", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(
      preview({
        files: [
          { path: "Core/manifest.json", size_bytes: 300, installed: true },
          { path: "README.txt", size_bytes: 2048, installed: false },
        ],
      }),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <ProfileModInstaller profileId="profile-1" />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    fireEvent.change(screen.getByPlaceholderText(/Or paste path/i), {
      target: { value: "/tmp/Pack.zip" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
    expect(
      await screen.findByText(
        /All files \(1 installed, 1 kept only in the archive\)/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("README.txt")).toBeInTheDocument();
    expect(screen.getByText("not installed")).toBeInTheDocument();
  });
});
