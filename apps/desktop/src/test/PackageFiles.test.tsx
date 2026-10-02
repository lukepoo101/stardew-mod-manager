import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PackageFiles } from "@/features/mods/PackageFiles";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

const renderFiles = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <PackageFiles profileComponentId="c1" />
    </QueryClientProvider>,
  );

describe("package and files", () => {
  it("lists installed files, searchable, and the package's state", async () => {
    vi.spyOn(api, "getModPackageFiles").mockResolvedValue({
      recorded: true,
      files: [
        { path: "manifest.json", size_bytes: 300, installed: true },
        { path: "assets/a.png", size_bytes: 2048, installed: true },
      ],
      package_mods: ["Core 1.0", "Extra 1.0"],
      artifact_hash: "abcdef0123456789",
      package_stored: true,
      package_intact: false,
    });
    renderFiles();
    expect(
      await screen.findByText(/no longer matches its checksum/),
    ).toBeInTheDocument();
    expect(screen.getByText(/Core 1.0, Extra 1.0/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Find"), {
      target: { value: "png" },
    });
    expect(screen.getByText("assets/a.png")).toBeInTheDocument();
    expect(screen.queryByText("manifest.json")).toBeNull();
  });

  it("says when no file list was recorded", async () => {
    vi.spyOn(api, "getModPackageFiles").mockResolvedValue({
      recorded: false,
      files: [],
      package_mods: ["Old 1.0"],
      artifact_hash: "abcdef0123456789",
      package_stored: false,
      package_intact: null,
    });
    renderFiles();
    expect(await screen.findByText(/is no longer stored/)).toBeInTheDocument();
    expect(screen.getByText(/there is no file list/)).toBeInTheDocument();
  });
});
