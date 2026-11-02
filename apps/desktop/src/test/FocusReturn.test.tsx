import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "@/components/ui/Modal";
import { ContextHeader } from "@/components/layout/ContextHeader";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
  ProfileSummaryDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("focus after a dialog closes", () => {
  it("returns to the control that opened it", async () => {
    const Harness = () => {
      const [open, setOpen] = useState(false);
      return (
        <main id="main-content" tabIndex={-1}>
          <button type="button" onClick={() => setOpen(true)}>
            Disable Power Core
          </button>
          {open && (
            <Modal labelledBy="t" onClose={() => setOpen(false)}>
              <h2 id="t">Disable Power Core?</h2>
              <button type="button" onClick={() => setOpen(false)}>
                Cancel
              </button>
            </Modal>
          )}
        </main>
      );
    };
    render(<Harness />);
    const opener = screen.getByRole("button", { name: "Disable Power Core" });
    opener.focus();
    fireEvent.click(opener);
    await waitFor(() =>
      expect(screen.getByRole("dialog")).toContainElement(
        document.activeElement as HTMLElement,
      ),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(opener));

    // Closed with its own Cancel button as well.
    fireEvent.click(opener);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("falls back to the page when the opener is gone", async () => {
    const Harness = () => {
      const [state, setState] = useState<"row" | "dialog" | "removed">("row");
      return (
        <main id="main-content" tabIndex={-1}>
          {state !== "removed" && (
            <button type="button" onClick={() => setState("dialog")}>
              Remove Power Core
            </button>
          )}
          {state === "dialog" && (
            <Modal labelledBy="t" onClose={() => setState("row")}>
              <h2 id="t">Remove?</h2>
              <button type="button" onClick={() => setState("removed")}>
                Remove mod
              </button>
            </Modal>
          )}
        </main>
      );
    };
    render(<Harness />);
    const opener = screen.getByRole("button", { name: "Remove Power Core" });
    opener.focus();
    fireEvent.click(opener);
    fireEvent.click(await screen.findByRole("button", { name: "Remove mod" }));
    await waitFor(() =>
      expect(document.activeElement?.id).toBe("main-content"),
    );
  });
});

describe("toggling a mod from its row", () => {
  it("keeps the button focusable while the change runs", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Main", revision: 1 },
      game: { id: "g" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      {
        profile_component_id: "c1",
        unique_id: "Pw.Extra",
        name: "Power Extra",
        author: "Pw",
        version: "1.0.0",
        description: null,
        enabled: true,
        installed_reason: "explicit",
        deployment_id: "d1",
        artifact_hash: "h",
        installed_at: "",
      } as unknown as ModListItemDto,
    ]);
    vi.spyOn(api, "getToggleImpact").mockResolvedValue({
      affected_mods: ["Power Extra"],
      dependents: [],
      unmet_requirements: [],
    } as never);
    let finish!: () => void;
    vi.spyOn(api, "setModEnabled").mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModsView />
      </QueryClientProvider>,
    );
    const button = await screen.findByRole("button", {
      name: "Disable Power Extra",
    });
    button.focus();
    fireEvent.click(button);
    await waitFor(() => expect(button).toHaveAttribute("aria-busy", "true"));
    // A disabled button would drop focus to the page body.
    expect(button).not.toBeDisabled();
    expect(document.activeElement).toBe(button);
    await act(async () => finish());
    expect(document.activeElement).toBe(button);
  });
});

describe("the mod details panel", () => {
  it("closes on Escape and gives focus back to the row", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Main", revision: 1 },
      game: { id: "g" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      {
        profile_component_id: "c1",
        unique_id: "Pw.Core",
        name: "Power Core",
        author: "Pw",
        version: "1.0.0",
        description: null,
        enabled: true,
        installed_reason: "explicit",
        deployment_id: "d1",
        artifact_hash: "h",
        installed_at: "",
      } as unknown as ModListItemDto,
    ]);
    vi.spyOn(api, "getModDetails").mockResolvedValue({
      profile_component_id: "c1",
      unique_id: "Pw.Core",
      name: "Power Core",
      author: "Pw",
      version: "1.0.0",
      description: null,
      update_keys: [],
      dependencies: [],
      content_pack_for: null,
      raw_manifest: "{}",
      artifact_hash: "h",
      deployment_root_path: "Pw.Core",
      earlier_versions: [],
    } as never);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModsView />
      </QueryClientProvider>,
    );
    const details = await screen.findByRole("button", {
      name: "Details for Power Core",
    });
    details.focus();
    fireEvent.click(details);
    const panel = await screen.findByRole("dialog", { name: "Power Core" });
    await waitFor(() =>
      expect(panel).toContainElement(document.activeElement as HTMLElement),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(details));
  });
});

describe("the header profile badge", () => {
  const overview = {
    profile: { id: "a", name: "Main", revision: 3 },
    game: { id: "g", canonical_root: "/games/Stardew Valley" },
    smapi_status: { is_installed: true, observed_version: "4.1.10" },
    health_summary: { findings: [] },
  } as unknown as ProfileOverviewDto;
  const profile = (id: string, name: string) =>
    ({
      id,
      name,
      game_installation_id: "g",
      mod_count: 0,
      revision: 1,
    }) as unknown as ProfileSummaryDto;

  function renderHeader() {
    vi.spyOn(api, "listProfiles").mockResolvedValue([
      profile("a", "Main"),
      profile("b", "Expanded"),
    ]);
    return render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <ContextHeader overview={overview} />
        </MemoryRouter>
      </QueryClientProvider>,
    );
  }

  it("switches the active profile in one action", async () => {
    const activate = vi.spyOn(api, "activateProfile").mockResolvedValue();
    renderHeader();
    const picker = await screen.findByRole("combobox", {
      name: "Active profile",
    });
    expect(picker).toHaveValue("a");
    fireEvent.change(picker, { target: { value: "b" } });
    await waitFor(() => expect(activate).toHaveBeenCalledWith("b"));
  });

  it("says the old profile is still active when switching fails", async () => {
    vi.spyOn(api, "activateProfile").mockRejectedValue(
      new Error("Stop the game first"),
    );
    renderHeader();
    fireEvent.change(
      await screen.findByRole("combobox", { name: "Active profile" }),
      { target: { value: "b" } },
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      '"Main" is still active',
    );
  });
});
