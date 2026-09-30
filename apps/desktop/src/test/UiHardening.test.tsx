import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  DEFAULT_PREFERENCES,
  loadPreferences,
  PREFERENCES_VERSION,
  savePreferences,
} from "@/shared/preferences";
import { CopyButton } from "@/components/ui/CopyButton";

beforeEach(() => localStorage.clear());

describe("status badges", () => {
  it.each(["success", "warning", "danger", "info", "neutral"] as const)(
    "%s carries a spoken label as well as colour",
    (variant) => {
      const { container } = render(
        <StatusBadge variant={variant}>Text</StatusBadge>,
      );
      expect(container.querySelector("svg")).not.toBeNull();
      expect(container.querySelector(".sr-only")?.textContent).toMatch(/:/);
    },
  );
});

describe("ui preferences", () => {
  it("round-trips valid values", () => {
    const values = {
      uiScale: 130,
      modFilter: "disabled",
      modSort: "newest",
      modSortDescending: true,
      showGuidance: false,
    } as const;
    savePreferences(values);
    expect(loadPreferences()).toEqual(values);
  });

  it("discards another version, bad JSON and bad values", () => {
    localStorage.setItem(
      "smm-ui-preferences",
      JSON.stringify({
        version: PREFERENCES_VERSION + 1,
        values: { uiScale: 150 },
      }),
    );
    expect(loadPreferences()).toEqual(DEFAULT_PREFERENCES);
    localStorage.setItem("smm-ui-preferences", "{not json");
    expect(loadPreferences()).toEqual(DEFAULT_PREFERENCES);
    localStorage.setItem(
      "smm-ui-preferences",
      JSON.stringify({
        version: PREFERENCES_VERSION,
        values: { uiScale: 5000, modFilter: "everything" },
      }),
    );
    expect(loadPreferences()).toEqual(DEFAULT_PREFERENCES);
  });
});

describe("copy button", () => {
  it("copies exactly the value and announces success", async () => {
    let copied = "";
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText: async (text: string) => {
          copied = text;
        },
      },
      configurable: true,
    });
    render(<CopyButton value="/a/b c" label="cache path" />);
    fireEvent.click(screen.getByRole("button", { name: "Copy cache path" }));
    expect(await screen.findByText("cache path copied")).toBeInTheDocument();
    expect(copied).toBe("/a/b c");
  });
});

const sourceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

function productionFiles(directory: string): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const full = path.join(directory, entry);
    if (statSync(full).isDirectory()) {
      return ["generated", "test"].includes(entry) ? [] : productionFiles(full);
    }
    return /\.tsx$/.test(entry) ? [full] : [];
  });
}

describe("safety wording", () => {
  it("never labels a mod as safe or trusted in visible copy", () => {
    // Visible strings only: text between tags or in title/aria attributes.
    const banned =
      /(safe mod|trusted mod|verified safe|guaranteed safe|100% safe)/i;
    for (const file of productionFiles(sourceRoot)) {
      const text = readFileSync(file, "utf8");
      expect(text, file).not.toMatch(banned);
    }
  });
});
