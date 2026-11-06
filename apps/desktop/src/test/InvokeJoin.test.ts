import { afterEach, describe, expect, it, vi } from "bun:test";

const { pending, invoke } = (() => {
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
})();

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { invokeApi } from "@/shared/api/invoke";

/** Waits until the backend has been asked `times` times in all. */
const asked = async (times: number) => {
  for (let tries = 0; invoke.mock.calls.length < times && tries < 200; tries++)
    await new Promise((done) => setTimeout(done, 5));
  expect(invoke).toHaveBeenCalledTimes(times);
};

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
  it("does not join a change when another was asked for in between", async () => {
    // On, off, on: the last press is what the user meant.
    const on = invokeApi("set_mod_enabled", { id: "a", enabled: true });
    const off = invokeApi("set_mod_enabled", { id: "a", enabled: false });
    const onAgain = invokeApi("set_mod_enabled", { id: "a", enabled: true });
    await asked(3);
    expect(invoke).toHaveBeenNthCalledWith(3, "set_mod_enabled", {
      id: "a",
      enabled: true,
    });
    for (const call of pending) call.resolve(null);
    await Promise.all([on, off, onAgain]);
  });

  it("never joins a read, which has to see the changes made before it", async () => {
    const before = invokeApi("list_profile_mods", { profileId: "p" });
    const change = invokeApi("set_mod_enabled", { id: "a", enabled: false });
    await asked(2);
    pending[1].resolve(null);
    await change;

    // Asked for again after the change: a fresh answer, not the one that
    // started before it. It runs once the earlier read has ended, and a
    // third request meanwhile shares that run.
    const after = invokeApi("list_profile_mods", { profileId: "p" });
    const alsoAfter = invokeApi("list_profile_mods", { profileId: "p" });
    pending[0].resolve(["a: on"]);
    await asked(3);
    pending[2].resolve(["a: off"]);
    await expect(alsoAfter).resolves.toEqual(["a: off"]);
    expect(invoke).toHaveBeenCalledTimes(3);
    await expect(before).resolves.toEqual(["a: on"]);
    await expect(after).resolves.toEqual(["a: off"]);
  });
});
