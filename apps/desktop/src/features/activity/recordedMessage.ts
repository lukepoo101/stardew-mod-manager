/**
 * The message an operation recorded. Recovery evidence is stored as
 * `{"code","message"}`; anything else is already a sentence.
 */
export function recordedMessage(raw: string | null): string | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as { message?: unknown };
    if (parsed && typeof parsed.message === "string") return parsed.message;
  } catch {
    // Not JSON: the message itself.
  }
  return raw;
}
