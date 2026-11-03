/**
 * Asks a yes/no question with the platform's own dialog and waits for it.
 *
 * In the desktop app, tauri-plugin-dialog replaces `window.confirm` with an
 * async function that shows a native dialog and resolves to the answer. Its
 * return value is a Promise, which is always truthy, so a synchronous
 * `if (!window.confirm(...)) return` never stops: the action runs before
 * the user has answered. In a browser and in tests `window.confirm` returns
 * the answer directly. Awaiting the result handles both.
 *
 * If the dialog cannot be shown at all (the plugin's permission missing from
 * the window's capability, for example), the answer is "no": a question
 * nobody saw is never taken as a yes.
 */
export async function askToConfirm(message: string): Promise<boolean> {
  try {
    const answer: boolean | Promise<boolean> = window.confirm(message);
    return Boolean(await answer);
  } catch (error) {
    console.error("Could not ask for confirmation", error);
    return false;
  }
}
