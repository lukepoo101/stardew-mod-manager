import { normalizeApiError } from "./errors";

/** Whether the frontend is running inside the Tauri desktop runtime. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * The single entry point for Tauri commands.
 *
 * Tauri rejects a failed command with the serialized ApiErrorDto produced by the
 * Rust IPC boundary. Nothing else in the frontend may call the Tauri invoke API
 * directly, so this is the one place where that rejection becomes an
 * ApiClientError.
 */
export async function invokeApi<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeApiError(error);
  }
}
