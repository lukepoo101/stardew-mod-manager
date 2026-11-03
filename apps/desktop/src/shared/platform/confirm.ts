/**
 * Asks a yes/no question with the platform's own dialog and waits for it.
 *
 * In the desktop app, tauri-plugin-dialog replaces `window.confirm` with an
 * async function that shows a native dialog and resolves to the answer. Its
 * return value is a Promise, which is always truthy, so a synchronous
 * `if (!window.confirm(...)) return` never stops: the action runs before
 * the user has answered. In a browser and in tests `window.confirm` returns
 * the answer directly. Awaiting the result handles both.
 */
export async function askToConfirm(message: string): Promise<boolean> {
  const answer: boolean | Promise<boolean> = window.confirm(message);
  return Boolean(await answer);
}
