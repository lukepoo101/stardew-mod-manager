import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "@/app/App";
import { api } from "@/shared/api/client";

// The backend invalidation event is what refreshes server state after a command.
// Emitting it for real here is what reproduces the interaction this test guards:
// registering a game creates its default profile, so the refreshed bootstrap
// reports an active profile while the user is still inside guided setup.
const tauriEvents = vi.hoisted(() => {
  const listeners = new Map<string, (event: unknown) => void>();
  return {
    listeners,
    listen: vi.fn(async (name: string, handler: (event: unknown) => void) => {
      listeners.set(name, handler);
      return vi.fn();
    }),
  };
});

vi.mock("@tauri-apps/api/event", () => ({ listen: tauriEvents.listen }));

const BACKEND_STATE_CHANGED = "backend-state-changed";

beforeEach(() => {
  window.location.hash = "/";
  localStorage.clear();
  tauriEvents.listeners.clear();
  tauriEvents.listen.mockClear();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.restoreAllMocks();
});

describe("guided setup and backend state refreshes", () => {
  it("keeps the user in setup when registering a game refreshes the bootstrap", async () => {
    // A fresh database: the seeded disposition is "completed", which with no
    // active profile is exactly what "nothing is configured yet" looks like.
    const seeded = {
      onboarding_disposition: "completed",
      active_game_installation_id: null,
      active_profile_id: null,
      recovery_summary: null,
      app_version: "0.1.0",
    };
    const bootstrap = vi.spyOn(api, "bootstrap").mockResolvedValue(seeded);
    vi.spyOn(api, "discoverGameInstallations").mockResolvedValue([
      {
        candidate_path: "/games/Stardew Valley",
        storefront: "steam",
        operating_system: "linux",
        detected_version: "1.6.14",
        support_state: "supported_fresh",
        is_usable: true,
        has_existing_smapi: false,
        has_existing_mods: false,
        is_writable: true,
        evidence: ["StardewValley found"],
      },
    ]);
    vi.spyOn(api, "registerGameInstallation").mockResolvedValue({
      id: "game",
      canonical_root: "/games/Stardew Valley",
      operating_system: "linux",
      storefront: "steam",
      management_mode: "managed",
      created_at: new Date().toISOString(),
    });

    render(<App />);
    expect(
      await screen.findByText("Locate Stardew Valley"),
    ).toBeInTheDocument();

    // Registering the game creates and activates its default profile.
    bootstrap.mockResolvedValue({
      ...seeded,
      active_game_installation_id: "game",
      active_profile_id: "profile",
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "Use this installation" }),
    );
    expect(
      await screen.findByRole("button", { name: "Install SMAPI" }),
    ).toBeInTheDocument();

    await waitFor(() =>
      expect(tauriEvents.listeners.has(BACKEND_STATE_CHANGED)).toBe(true),
    );
    await act(async () => {
      tauriEvents.listeners.get(BACKEND_STATE_CHANGED)?.({});
    });

    // The refreshed bootstrap now describes a configured installation, but the
    // user is still in setup and must stay there: SMAPI is only installable from
    // this flow.
    await waitFor(() => expect(bootstrap).toHaveBeenCalled());
    expect(
      screen.getByRole("button", { name: "Install SMAPI" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Ready to Play")).not.toBeInTheDocument();
  });
});
