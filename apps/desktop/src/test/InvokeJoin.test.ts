import { afterEach, describe, expect, it, vi } from "vitest";

const { pending, invoke } = vi.hoisted(() => {
  const pending: Array<{
    resolve: (value: unknown) => void;
    reject: (error: unknown) => void;
  }> = [];
  const invoke = vi.fn(
    () =>
      new Promise((resolve, reject) => {
        pending.push({ resolve, reject });
      }),
  );
  return { pending, invoke };
});

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { invokeApi } from "@/shared/api/invoke";

/** Waits until the backend has been asked `times` times in all. */
const asked = (times: number) =>
  vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(times));

afterEach(() => {
  pending.length = 0;
  invoke.mockClear();
});

describe("commands asked for twice while running", () => {
  it("joins a second identical request instead of running it again", async () => {
    const first = invokeApi<string>("clone_profile", {
      profileId: "p",
      name: "Copy",
    });
    const second = invokeApi<string>("clone_profile", {
      profileId: "p",
      name: "Copy",
    });
    await asked(1);

    pending[0].resolve("done once");
    await expect(first).resolves.toBe("done once");
    await expect(second).resolves.toBe("done once");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("runs requests with different arguments, and the same one again once it finished", async () => {
    const one = invokeApi("set_mod_enabled", { id: "a", enabled: true });
    const other = invokeApi("set_mod_enabled", { id: "b", enabled: true });
    await asked(2);
    pending[0].resolve(null);
    pending[1].resolve(null);
    await Promise.all([one, other]);

    const again = invokeApi("set_mod_enabled", { id: "a", enabled: true });
    await asked(3);
    pending[2].resolve(null);
    await again;
  });

  it("gives both callers the failure, and runs the request again afterwards", async () => {
    const first = invokeApi("execute_operation", { operationId: "x" });
    const second = invokeApi("execute_operation", { operationId: "x" });
    await asked(1);
    pending[0].reject({
      code: "COMMAND_PANICKED",
      category: "internal",
      summary: "Something went wrong inside the manager, so this was stopped.",
      technical_details: "panic: boom",
      context: null,
      recoverability: "terminal",
      operation_id: null,
    });
    await expect(first).rejects.toMatchObject({ code: "COMMAND_PANICKED" });
    await expect(second).rejects.toMatchObject({ code: "COMMAND_PANICKED" });

    const again = invokeApi("execute_operation", { operationId: "x" });
    await asked(2);
    pending[1].resolve(null);
    await again;
  });
});
