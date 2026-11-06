import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "bun:test";
import { ModNotesPanel } from "@/features/mods/ModNotesPanel";
import type { ModAnnotationDto } from "@/shared/api/generated";

const saved = (note: string) =>
  ({
    unique_id: "Pw.Core",
    tags: ["qol"],
    note,
    favourite: false,
    source_url: null,
  }) as unknown as ModAnnotationDto;

describe("a note being typed", () => {
  it("survives the annotations being read again after a change elsewhere", () => {
    const panel = (annotation: ModAnnotationDto) => (
      <ModNotesPanel
        profileComponentId="c1"
        uniqueId="Pw.Core"
        annotation={annotation}
      />
    );
    const { rerender } = render(panel(saved("old")));
    const note = screen.getByRole("textbox", { name: "Note" });
    fireEvent.change(note, { target: { value: "needs the 1.6 patch" } });

    // The window refetches every query after any change: a new object with
    // the same saved values.
    rerender(panel(saved("old")));
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue(
      "needs the 1.6 patch",
    );

    // A note saved elsewhere does replace it.
    rerender(panel(saved("from the command line")));
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveValue(
      "from the command line",
    );
  });
});
