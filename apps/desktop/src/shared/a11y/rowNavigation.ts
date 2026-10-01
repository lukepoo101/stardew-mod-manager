import type React from "react";

const FOCUSABLE =
  'button, input, select, textarea, a[href], [tabindex]:not([tabindex="-1"])';

/** Controls that use the arrow keys themselves keep them. */
const OWNS_ARROWS =
  'input:not([type="checkbox"]):not([type="radio"]), select, textarea, [contenteditable="true"], [role="combobox"], [role="slider"]';

function focusables(row: HTMLElement): HTMLElement[] {
  return Array.from(row.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => !el.hasAttribute("disabled"),
  );
}

/**
 * Arrow-key movement between the rows of a list, for its container's
 * `onKeyDown`. Rows are the descendants marked `data-row`. Up and Down move
 * to the same control in the previous or next row, Home and End to the first
 * or last row. Tab order is untouched, and controls that use the arrows
 * themselves (text fields, selects) keep them.
 */
export function handleRowNavigation(
  event: React.KeyboardEvent<HTMLElement>,
): void {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  const target = event.target as HTMLElement;
  if (target.closest(OWNS_ARROWS)) return;
  const rows = Array.from(
    event.currentTarget.querySelectorAll<HTMLElement>("[data-row]"),
  );
  const current = rows.findIndex((row) => row.contains(target));
  if (current < 0) return;
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? rows.length - 1
        : event.key === "ArrowDown"
          ? Math.min(current + 1, rows.length - 1)
          : Math.max(current - 1, 0);
  event.preventDefault();
  if (next === current) return;
  const column = Math.max(focusables(rows[current]).indexOf(target), 0);
  const candidates = focusables(rows[next]);
  const destination =
    candidates[Math.min(column, candidates.length - 1)] ?? rows[next];
  destination.focus();
}
