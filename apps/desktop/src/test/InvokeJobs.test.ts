import { afterEach, describe, expect, it, vi } from "bun:test";

const { invoke, results } = (() => {
  // What job_results answers, call by call.
  const results: unknown[][] = [];
  const invoke = vi.fn(async (command: string, args?: { jobs?: number[] }) => {
    if (command === "job_results") return results.shift() ?? [];
    if (command === "list_profiles") return { __job: 7 };
    if (command === "set_mod_enabled") return { __job: 8 };
    if (command === "profile_bundle_progress") return { done: 1 };
    throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
  });
  return { invoke, results };
})();

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { invokeApi } from "@/shared/api/invoke";

afterEach(() => {
  results.length = 0;
  invoke.mockClear();
});

describe("commands that answer with a job", () => {
  it("waits for the job's answer", async () => {
    results.push([], [], [{ job: 7, ok: ["Main"] }]);
    await expect(invokeApi("list_profiles")).resolves.toEqual(["Main"]);
    const asked = invoke.mock.calls.filter(([c]) => c === "job_results");
    expect(asked.length).toBe(3);
    expect(asked[0][1]).toEqual({ jobs: [7] });
  });

  it("rejects with the job's error as an API error", async () => {
    results.push([
      {
        job: 8,
        err: {
          code: "COMMAND_PANICKED",
          category: "internal",
          summary:
            "Something went wrong inside the manager, so this was stopped.",
          technical_details: "panic: boom",
          context: null,
          recoverability: "terminal",
          operation_id: null,
        },
      },
    ]);
    await expect(
      invokeApi("set_mod_enabled", { id: "a", enabled: true }),
    ).rejects.toMatchObject({ code: "COMMAND_PANICKED" });
  });

  it("passes a direct answer through", async () => {
    await expect(invokeApi("profile_bundle_progress")).resolves.toEqual({
      done: 1,
    });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
