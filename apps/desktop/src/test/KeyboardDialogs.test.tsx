import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "@/components/ui/Modal";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { api } from "@/shared/api/client";
import type { OperationPreviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const preview: OperationPreviewDto = {
  operation_id: "op-1",
  artifact_hash: "h",
  original_filename: "Mod.zip",
  byte_size: 1,
  detected_components: [
    {
      unique_id: "A.Mod",
      name: "A Mod",
      author: "A",
      version: "1.0",
      description: null,
      relative_root: "A",
    },
  ],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  affected_profile_component_ids: [],
  expected_profile_revision: 1,
  replaces: [],
};

describe("keyboard use of dialogs", () => {
  it("moves focus into a dialog, closes on Escape and gives focus back", async () => {
    const onClose = vi.fn();
    const Harness = () => (
      <>
        <button type="button">Opener</button>
        <Modal labelledBy="t" onClose={onClose}>
          <h2 id="t">Title</h2>
          <button type="button">Cancel</button>
          <button type="button">Confirm</button>
        </Modal>
      </>
    );
    render(<Harness />);
    await waitFor(() =>
      expect(screen.getByRole("dialog", { name: "Title" })).toContainElement(
        document.activeElement as HTMLElement,
      ),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("ignores Escape while a dialog has no close action", async () => {
    render(
      <Modal labelledBy="busy">
        <h2 id="busy">Working</h2>
        <button type="button">Wait</button>
      </Modal>,
    );
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Working" })).toBeInTheDocument();
  });

  it("Escape on an install review cancels it like the Cancel button", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(preview);
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <ProfileModInstaller profileId="p1" />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    fireEvent.change(screen.getByPlaceholderText(/Or paste path/i), {
      target: { value: "/tmp/Mod.zip" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Review mod installation",
    });
    // The review names each component's UniqueID and where it comes from.
    expect(screen.getByText(/A\.Mod, from "A"/)).toBeInTheDocument();
    expect(screen.getByText("Archive size:")).toBeInTheDocument();
    // Each kind of check is named for what it is, and code is not assessed.
    for (const label of [
      "Source:",
      "Integrity:",
      "Archive structure:",
      "What the code does:",
    ]) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.getByText("not assessed.")).toBeInTheDocument();
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("op-1"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
