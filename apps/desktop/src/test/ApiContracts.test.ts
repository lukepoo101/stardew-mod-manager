import { afterEach, describe, expect, it, vi } from "bun:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { api } from "@/shared/api/client";

afterEach(clearMocks);
describe("production IPC request arguments", () => {
  it("sends Rust command argument names while the Tauri runtime is present", async () => {
    // Typed as mockIPC calls it, so mock.calls is typed for toEqual below.
    const dispatch = vi.fn((_cmd: string, _payload?: unknown) => null);
    mockIPC(dispatch);
    await api.listProfiles();
    await api.createProfile("Seasonal", "game-id");
    await api.archiveProfile("profile-id");
    await api.getSmapiStatus("game-id");
    await api.cancelActiveOperation("operation-id");
    await api.prepareRemoval("component-id");
    await api.renameModTag("Farm", "Farming");
    await api.listLaunchSessions(5);
    await api.setDefaultProfile("profile-id");
    await api.setDefaultProfile(null);
    expect(dispatch.mock.calls).toEqual([
      ["list_profiles", {}],
      ["create_profile", { name: "Seasonal", gameId: "game-id" }],
      ["archive_profile", { profileId: "profile-id" }],
      ["get_smapi_status", { gameId: "game-id" }],
      ["cancel_active_operation", { operationId: "operation-id" }],
      ["prepare_remove", { profileComponentId: "component-id" }],
      ["rename_mod_tag", { from: "Farm", to: "Farming" }],
      ["list_launch_sessions", { limit: 5 }],
      ["set_default_profile", { profileId: "profile-id" }],
      ["set_default_profile", { profileId: null }],
    ]);
  });
});
