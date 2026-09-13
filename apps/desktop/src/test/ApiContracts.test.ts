import { afterEach, describe, expect, it, vi } from "vitest";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { api } from "@/shared/api/client";

afterEach(clearMocks);
describe("production IPC request arguments", () => {
  it("sends Rust command argument names while the Tauri runtime is present", async () => {
    const dispatch = vi.fn(() => null);
    mockIPC(dispatch);
    await api.listProfiles();
    await api.createProfile("Seasonal", "game-id");
    await api.duplicateProfile("profile-id", "Copy");
    await api.getSmapiStatus("game-id");
    await api.cancelActiveOperation("operation-id");
    await api.prepareRemoval("component-id");
    expect(dispatch.mock.calls).toEqual([
      ["list_profiles", {}],
      ["create_profile", { name: "Seasonal", gameId: "game-id" }],
      ["duplicate_profile", { profileId: "profile-id", newName: "Copy" }],
      ["get_smapi_status", { gameId: "game-id" }],
      ["cancel_active_operation", { operationId: "operation-id" }],
      ["prepare_remove", { profileComponentId: "component-id" }],
    ]);
  });
});
