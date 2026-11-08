import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "bun:test";
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

function OpenerHarness() {
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
}

function RemovedOpenerHarness() {
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
}

describe("focus after a dialog closes", () => {
  it("returns to the control that opened it", async () => {
    render(<OpenerHarness />);
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
    render(<RemovedOpenerHarness />);
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

  async function openDetails(load: () => Promise<unknown>) {
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
    vi.spyOn(api, "getModDetails").mockImplementation(load as never);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModsView />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Details for Power Core" }),
    );
  }
  const details = {
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
  };

  it("asks before Escape throws away a half-typed note", async () => {
    await openDetails(async () => details);
    const note = await screen.findByRole("textbox", { name: "Note" });
    // The panel can still be settling as the textbox appears (the saved
    // note is read in again), which may put back the empty note; type again
    // until what was typed stays, as a user would see it does.
    await waitFor(() => {
      if ((note as HTMLTextAreaElement).value !== "needs the 1.6 patch") {
        fireEvent.change(note, { target: { value: "needs the 1.6 patch" } });
      }
      expect(note).toHaveValue("needs the 1.6 patch");
    });

    // In the desktop app tauri-plugin-dialog makes window.confirm async: it
    // returns a Promise of the answer, which is truthy before it resolves.
    const answer = (value: boolean) =>
      Promise.resolve(value) as unknown as boolean;
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(answer(false));
    await act(async () => {
      fireEvent.keyDown(note, { key: "Escape" });
    });
    expect(confirm).toHaveBeenCalledOnce();
    // Kept open, with what was typed.
    expect(screen.getByRole("dialog", { name: "Power Core" })).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue(
      "needs the 1.6 patch",
    );

    confirm.mockReturnValue(answer(true));
    fireEvent.click(screen.getByRole("button", { name: "Close details" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("closes without asking when nothing was typed", async () => {
    await openDetails(async () => details);
    await screen.findByRole("textbox", { name: "Note" });
    const confirm = vi.spyOn(window, "confirm");
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(confirm).not.toHaveBeenCalled();
  });

  it("shows a failed load inside the panel", async () => {
    await openDetails(async () => {
      throw new Error("Database unavailable");
    });
    const panel = await screen.findByRole("dialog");
    expect(await within(panel).findByRole("alert")).toHaveTextContent(
      "Database unavailable",
    );
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

  it("switches only on a confirmed choice and keeps focus while it runs", async () => {
    let finish!: () => void;
    const activate = vi.spyOn(api, "activateProfile").mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    renderHeader();
    const picker = await screen.findByRole("combobox", {
      name: "Active profile",
    });
    expect(picker).toHaveValue("a");
    // Moving through the list (as arrow keys do) only picks.
    picker.focus();
    fireEvent.change(picker, { target: { value: "b" } });
    fireEvent.change(picker, { target: { value: "a" } });
    fireEvent.change(picker, { target: { value: "b" } });
    expect(activate).not.toHaveBeenCalled();

    // Enter on the badge (or the Switch button) applies it, once.
    fireEvent.keyDown(picker, { key: "Enter" });
    await waitFor(() => expect(activate).toHaveBeenCalledTimes(1));
    expect(activate).toHaveBeenCalledWith("b");
    // Nothing focused is disabled while it runs.
    expect(picker).not.toBeDisabled();
    expect(document.activeElement).toBe(picker);
    const busy = screen.getByRole("button", { name: /Switching/ });
    expect(busy).not.toBeDisabled();
    fireEvent.click(busy);
    expect(activate).toHaveBeenCalledTimes(1);
    await act(async () => finish());
    expect(document.activeElement).toBe(picker);
  });

  it("forgets an unconfirmed choice on Escape", async () => {
    const activate = vi.spyOn(api, "activateProfile").mockResolvedValue();
    renderHeader();
    const picker = await screen.findByRole("combobox", {
      name: "Active profile",
    });
    fireEvent.change(picker, { target: { value: "b" } });
    expect(screen.getByRole("button", { name: "Switch" })).toBeInTheDocument();
    fireEvent.keyDown(picker, { key: "Escape" });
    expect(picker).toHaveValue("a");
    expect(screen.queryByRole("button", { name: "Switch" })).toBeNull();
    expect(activate).not.toHaveBeenCalled();
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
    fireEvent.click(screen.getByRole("button", { name: "Switch" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      '"Main" is still active',
    );
  });
});
