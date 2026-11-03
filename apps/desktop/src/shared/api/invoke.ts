import { normalizeApiError } from "./errors";

/** Whether the frontend is running inside the Tauri desktop runtime. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** The Tauri core module, loaded once on the first command. */
let tauriCore: Promise<typeof import("@tauri-apps/api/core")> | null = null;

/** Commands still running, by command and arguments. */
const inFlight = new Map<string, Promise<unknown>>();

/**
 * The single entry point for Tauri commands.
 *
 * Tauri rejects a failed command with the serialized ApiErrorDto produced by the
 * Rust IPC boundary. Nothing else in the frontend may call the Tauri invoke API
 * directly, so this is the one place where that rejection becomes an
 * ApiClientError.
 *
 * Commands run in the background, so the window keeps taking clicks while
 * one works. The same command with the same arguments asked for again while
 * it is still running (a double click, a second press while a button was
 * still enabled) joins the one already running instead of doing it twice.
 */
export function invokeApi<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const key = `${command}\u0000${JSON.stringify(args ?? {})}`;
  const running = inFlight.get(key);
  if (running) return running as Promise<T>;
  const call = send<T>(command, args).finally(() => inFlight.delete(key));
  inFlight.set(key, call);
  return call;
}

async function send<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    // The dynamic import is inside the boundary too: failing to load the Tauri
    // module is exactly the unexpected desktop-communication failure this
    // wrapper promises to normalize.
    tauriCore ??= import("@tauri-apps/api/core");
    const { invoke } = await tauriCore;
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeApiError(error);
  }
}
