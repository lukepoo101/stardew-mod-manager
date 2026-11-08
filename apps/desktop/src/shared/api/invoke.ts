import { normalizeApiError } from "./errors";

/** Whether the frontend is running inside the Tauri desktop runtime. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** The Tauri core module, loaded once on the first command. */
let tauriCore: Promise<typeof import("@tauri-apps/api/core")> | null = null;

/**
 * The commands a second press must not run again: everything that changes
 * the manager's data (the backend's `background::change`), and saving a file
 * the user asked for. A Rust test in `background.rs` keeps this in step with
 * the backend. Reads are left out: a read asked for again after a change has
 * to see that change, so it never joins one that started before it.
 */
const JOINABLE: ReadonlySet<string> = new Set([
  "accept_mod_files",
  "activate_profile",
  "adopt_mods",
  "answer_troubleshoot",
  "apply_shared_settings",
  "archive_profile",
  "associate_save",
  "attach_reference_recipe",
  "backup_save",
  "begin_change_set",
  "bring_back_profile",
  "cancel_active_operation",
  "change_set_part_done",
  "clone_profile",
  "create_profile",
  "create_restore_point",
  "delete_profile",
  "delete_restore_point",
  "detach_reference_recipe",
  "dismiss_finding",
  "execute_operation",
  "export_mods_folder",
  "export_profile_bundle",
  "finish_change_set",
  "finish_profile_copy",
  "forget_known_good",
  "forget_troubleshoot_result",
  "freeze_profile",
  "import_profile_bundle",
  "inspect_package_for_install",
  "install_pinned_smapi",
  "install_stored_package",
  "keep_experiment",
  "launch_active_profile",
  "mark_operation_handled",
  "move_aside_and_set_mod_enabled",
  "narrow_troubleshoot",
  "prepare_mod_update",
  "prepare_remove",
  "publish_collection_revision",
  "put_aside_unfinished_change",
  "recreate_profile_from_point",
  "register_game_installation",
  "reinstall_mod",
  "remove_game_installation",
  "rename_mod_tag",
  "replace_mod_version",
  "restore_config_backup",
  "restore_finding",
  "restore_profile",
  "restore_save_backup",
  "restore_to_point",
  "restore_troubleshoot",
  "retry_recovery",
  "run_cleanup",
  "save_collection_draft",
  "save_json_export",
  "save_recipe_file",
  "set_default_profile",
  "set_game_version_override",
  "set_mod_annotation",
  "set_mod_enabled",
  "set_mod_source_link",
  "set_mods_enabled",
  "set_onboarding_disposition",
  "set_reference_difference_accepted",
  "set_retention_policy",
  "set_smapi_update_settings",
  "start_experiment",
  "start_troubleshoot",
  "terminate_active_launch_session",
  "undo_removal",
  "unfreeze_profile",
  "uninstall_smapi",
  "update_profile_details",
]);

/** Changes still running, by command and arguments. */
const inFlight = new Map<string, Promise<unknown>>();

/** The change asked for last, by command and arguments. */
let lastChange: string | null = null;

/**
 * The single entry point for Tauri commands.
 *
 * Tauri rejects a failed command with the serialized ApiErrorDto produced by the
 * Rust IPC boundary. Nothing else in the frontend may call the Tauri invoke API
 * directly, so this is the one place where that rejection becomes an
 * ApiClientError.
 *
 * Commands run in the background, so the window keeps taking clicks while
 * one works. A change asked for again with the same arguments while it is
 * still running (a double click, a second press while a button was still
 * enabled) joins the one already running instead of doing it twice, but only
 * when no other change was asked for in between: on, off, on must end on.
 * Reads are never joined to one already running; see `read`.
 */
export function invokeApi<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const key = `${command}\u0000${JSON.stringify(args ?? {})}`;
  if (!JOINABLE.has(command)) return read<T>(key, command, args);
  const running = inFlight.get(key);
  if (running && lastChange === key) return running as Promise<T>;
  lastChange = key;
  const call: Promise<T> = send<T>(command, args).finally(() => {
    if (inFlight.get(key) === call) inFlight.delete(key);
  });
  inFlight.set(key, call);
  return call;
}

/** Reads running, and the one asked for again behind each, by key. */
const readsRunning = new Map<string, Promise<unknown>>();
const readsNext = new Map<string, Promise<unknown>>();

/**
 * A read asked for again while the same read runs does not join it (it may
 * have started before a change the caller needs to see), and does not run
 * beside it either: it runs once the running one ends, and every request
 * made meanwhile shares that next run. After every change the window asks
 * for a dozen reads; without this they piled up while a long batch ran.
 */
function read<T>(
  key: string,
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const running = readsRunning.get(key);
  if (!running) return startRead<T>(key, command, args);
  let next = readsNext.get(key);
  if (!next) {
    next = running
      .catch(() => undefined)
      .then(() => {
        readsNext.delete(key);
        return startRead<T>(key, command, args);
      });
    readsNext.set(key, next);
  }
  return next as Promise<T>;
}

function startRead<T>(
  key: string,
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const call: Promise<T> = send<T>(command, args).finally(() => {
    if (readsRunning.get(key) === call) readsRunning.delete(key);
  });
  readsRunning.set(key, call);
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
    const answer = await invoke<T | JobTicket>(command, args);
    return isJob(answer) ? await finished<T>(answer.__job) : answer;
  } catch (error) {
    throw normalizeApiError(error);
  }
}

/**
 * A command that works in the background answers at once with a job, and the
 * outcome is asked for with `job_results` (see `background.rs`: keeping every
 * Tauri object on the main thread is what keeps the app from crashing).
 */
interface JobTicket {
  __job: number;
}

interface JobOutcome {
  job: number;
  ok?: unknown;
  err?: unknown;
}

function isJob(value: unknown): value is JobTicket {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as JobTicket).__job === "number" &&
    Object.keys(value).length === 1
  );
}

/** Jobs waiting for their outcome, by job. */
const waiting = new Map<
  number,
  { resolve: (value: unknown) => void; reject: (error: unknown) => void }
>();
let polling = false;

function finished<T>(job: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    waiting.set(job, {
      resolve: (value) => resolve(value as T),
      reject,
    });
    void poll();
  });
}

/**
 * One loop asks for every waiting job at once: straight away, then less
 * often while nothing finishes (2 ms doubling up to 25 ms), so a quick read
 * costs one more round trip and a long change a few cheap ones a second.
 */
async function poll(): Promise<void> {
  if (polling) return;
  polling = true;
  let pause = 0;
  try {
    while (waiting.size > 0) {
      if (pause > 0) await new Promise((done) => setTimeout(done, pause));
      tauriCore ??= import("@tauri-apps/api/core");
      const { invoke } = await tauriCore;
      let outcomes: JobOutcome[];
      try {
        outcomes = await invoke<JobOutcome[]>("job_results", {
          jobs: [...waiting.keys()],
        });
      } catch (error) {
        // The window lost the backend: every waiting job fails the same way.
        for (const [job, { reject }] of waiting) {
          waiting.delete(job);
          reject(error);
        }
        return;
      }
      for (const outcome of outcomes) {
        const caller = waiting.get(outcome.job);
        if (!caller) continue;
        waiting.delete(outcome.job);
        if (outcome.err !== undefined) caller.reject(outcome.err);
        else caller.resolve(outcome.ok ?? null);
      }
      pause = outcomes.length > 0 ? 0 : Math.min(pause > 0 ? pause * 2 : 2, 25);
    }
  } finally {
    polling = false;
  }
}
