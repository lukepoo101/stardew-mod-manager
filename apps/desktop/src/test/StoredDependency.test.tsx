import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StoredDependencyInstall } from "@/features/mods/StoredDependencyInstall";
import { api } from "@/shared/api/client";
import type { ModRequirementDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const requirement = {
  unique_id: "Z.Lib",
  name: null,
  kind: "required",
  status: "missing",
  minimum_version: "2.0.0",
  installed_version: null,
} as unknown as ModRequirementDto;

describe("installing a missing requirement from storage", () => {
  it("names the stored copy, its version and why, then installs it", async () => {
    const find = vi.spyOn(api, "findStoredMod").mockResolvedValue([
      {
        artifact_hash: "h2",
        name: "Z Lib",
        version: "2.1.0",
        original_filename: "ZLib-2.1.zip",
        meets_minimum: true,
      },
      {
        artifact_hash: "h1",
        name: "Z Lib",
        version: "1.0.0",
        original_filename: "ZLib-1.0.zip",
        meets_minimum: false,
      },
    ]);
    const install = vi.spyOn(api, "installStoredPackage").mockResolvedValue();
    render(
      <StoredDependencyInstall
        requirement={requirement}
        requiredBy="Needy"
        profileId="p1"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Find a stored copy" }));
    expect(
      await screen.findByText(
        /Z Lib 2.1.0 from ZLib-2.1.zip, stored by the manager, required by Needy/,
      ),
    ).toBeInTheDocument();
    expect(find).toHaveBeenCalledWith("Z.Lib", "2.0.0");
    // A version too old for the requirement is not offered.
    expect(screen.queryByText(/ZLib-1.0.zip/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Install it" }));
    await waitFor(() => expect(install).toHaveBeenCalledWith("p1", "h2"));
    expect(
      await screen.findByText("Installed Z Lib 2.1.0."),
    ).toBeInTheDocument();
  });

  it("gives the UniqueID to look for when nothing is stored", async () => {
    vi.spyOn(api, "findStoredMod").mockResolvedValue([]);
    render(
      <StoredDependencyInstall
        requirement={requirement}
        requiredBy="Needy"
        profileId="p1"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Find a stored copy" }));
    expect(await screen.findByText(/No stored copy/)).toBeInTheDocument();
    expect(screen.getByText("Z.Lib")).toBeInTheDocument();
  });
});
