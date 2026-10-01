import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { handleRowNavigation } from "@/shared/a11y/rowNavigation";

function List() {
  return (
    <ul onKeyDown={handleRowNavigation}>
      {["A", "B", "C"].map((name) => (
        <li data-row key={name}>
          <input type="checkbox" aria-label={`Select ${name}`} />
          <button type="button">Open {name}</button>
          <input type="text" aria-label={`Note ${name}`} />
        </li>
      ))}
    </ul>
  );
}

describe("row navigation", () => {
  it("moves to the same control in the next and previous row", () => {
    render(<List />);
    const openA = screen.getByRole("button", { name: "Open A" });
    openA.focus();
    fireEvent.keyDown(openA, { key: "ArrowDown" });
    expect(screen.getByRole("button", { name: "Open B" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowUp" });
    expect(openA).toHaveFocus();
    // Already at the top: focus stays.
    fireEvent.keyDown(openA, { key: "ArrowUp" });
    expect(openA).toHaveFocus();
  });

  it("jumps to the first and last row with Home and End", () => {
    render(<List />);
    const selectB = screen.getByRole("checkbox", { name: "Select B" });
    selectB.focus();
    fireEvent.keyDown(selectB, { key: "End" });
    expect(screen.getByRole("checkbox", { name: "Select C" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement as Element, { key: "Home" });
    expect(screen.getByRole("checkbox", { name: "Select A" })).toHaveFocus();
  });

  it("leaves arrow keys to text fields", () => {
    render(<List />);
    const note = screen.getByRole("textbox", { name: "Note A" });
    note.focus();
    fireEvent.keyDown(note, { key: "ArrowDown" });
    expect(note).toHaveFocus();
  });
});
