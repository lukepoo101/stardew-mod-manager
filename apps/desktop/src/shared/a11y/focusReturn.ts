/**
 * Where keyboard focus goes when a dialog closes.
 *
 * Radix returns focus to a dialog's Trigger, but these dialogs are opened by
 * ordinary buttons (often after an async check) and have no Trigger, so focus
 * fell to the page body and a keyboard user on row 120 started again from
 * the top. Dialogs remember what was focused when they opened and go back to
 * it; when that control is gone (its row was removed), the main content gets
 * focus so the next Tab still starts inside the page.
 */

/** The focused control, captured when a dialog opens. */
export function focusedElement(): HTMLElement | null {
  const active = document.activeElement;
  return active instanceof HTMLElement && active !== document.body
    ? active
    : null;
}

/** For a Radix `onCloseAutoFocus`: focuses the opener instead of nothing. */
export function returnFocus(opener: HTMLElement | null, event: Event): void {
  event.preventDefault();
  if (
    opener?.isConnected &&
    !opener.hasAttribute("disabled") &&
    !opener.closest("[inert]")
  ) {
    opener.focus();
    return;
  }
  document.getElementById("main-content")?.focus();
}
