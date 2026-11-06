import { describe, expect, it, vi } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { askToConfirm } from "@/shared/platform/confirm";
import capability from "../../src-tauri/capabilities/default.json";

describe("askToConfirm", () => {
  it("waits for the answer the desktop dialog resolves to", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(
      Promise.resolve(false) as unknown as boolean,
    );
    expect(await askToConfirm("Delete?")).toBe(false);
  });

  it("takes the browser's direct answer", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    expect(await askToConfirm("Delete?")).toBe(true);
  });

  it("treats a dialog that could not be shown as no", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(
      Promise.reject(
        new Error("dialog.message not allowed"),
      ) as unknown as boolean,
    );
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(await askToConfirm("Delete?")).toBe(false);
  });
});

describe("confirmations in the desktop app", () => {
  it("are allowed to open the native dialog", () => {
    // Without this the plugin's confirm rejects, so nothing is ever asked.
    expect(capability.permissions).toContain("dialog:allow-message");
  });

  it("never call window.confirm directly", () => {
    const root = path.resolve(__dirname, "..");
    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const file = path.join(dir, name);
        if (statSync(file).isDirectory()) {
          if (name !== "test") walk(file);
        } else if (/\.tsx?$/.test(name) && !file.endsWith("confirm.ts")) {
          if (readFileSync(file, "utf8").includes("window.confirm(")) {
            offenders.push(path.relative(root, file));
          }
        }
      }
    };
    walk(root);
    expect(offenders).toEqual([]);
  });
});
