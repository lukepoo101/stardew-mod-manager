// Windows packaged-application smoke test.
//
// Windows drives a WebView2 application through Microsoft Edge Driver rather
// than WebKitWebDriver, so the driver process, its capabilities and the
// fixture layout all differ. The user journey itself is shared with the Linux
// runner.
//
// The Tauri context macro embeds the frontend into the executable, so the
// runtime contract is a single self-contained binary: there is no sibling
// dist directory to check for, and asserting one would fail a perfectly good
// installation.
//
// Three levels are supported, and they make different claims:
//
// * `--direct-start` launches the packaged application the way a user does
//   and requires it to stay alive, which is the one claim that always has to
//   hold.
// * `--require-webview2` fails when Edge Driver is unavailable, and is what
//   the release pipeline uses.
// * Without either, the run reports `skipped` and still verifies the
//   application on disk, so a developer machine without the driver gets a
//   useful answer rather than a false failure.
//
// (As in the Python original, `--require-webview2` is accepted but not read,
// and no `skipped` result is produced: a missing provider is `blocked`.)
//
// Each launch gets its own WebView2 user-data directory. Two launches sharing
// one is not a detail: the runtime keeps a lock in that directory, so a second
// process can fail to start for no reason that has anything to do with the
// application.

import {
  closeSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  normalize,
  relative,
  resolve,
  sep,
} from "node:path";
import {
  buildModArchive,
  captureFailure,
  onInterrupt,
  type Result,
  removeTree,
  runUserJourney,
  Session,
} from "./common.ts";
import {
  type ChildProcess,
  hasExited,
  returnCode,
  stopProcess,
  waitForExit,
} from "./process.ts";
import {
  AssertionError,
  environ,
  IS_WINDOWS,
  PyFloat,
  print,
  pyG,
  pySplitlines,
  sleep,
  sortedKeys,
  writeText,
} from "./py.ts";
import { CREATE_NEW_PROCESS_GROUP, spawnWithFlags } from "./win32-process.ts";
import {
  classifyFailure,
  EMBEDDED_AUTODETECT_TIMEOUT_SECONDS,
  EMBEDDED_READY_TIMEOUT_SECONDS,
  EmbeddedApplication,
  PROVIDER_EMBEDDED,
  PROVIDER_EXTERNAL,
  PROVIDERS,
  ProviderUnavailable,
  reservePort,
} from "./windows-driver.ts";

// The Rust package name, which is what cargo and the bundler actually produce.
// The product name is display text and never appears in a file name.
export const BINARY_NAME = "stardew-mod-manager.exe";
export const DEFAULT_BINARY = join("target", "release", BINARY_NAME);

const NATIVE_DRIVER_NAMES = ["msedgedriver.exe", "msedgedriver"];

// The WebView2 user-data directory name inside a launch's isolated scratch
// directory. Each launch gets a scratch directory of its own, so this name
// repeating across launches is not a shared location.
const DIRECT_START_DIRECTORY = "webview2-data";

// Starting a WebView2 application means launching a process, waiting for the
// runtime to initialise and attaching to it, which is slower than any other
// WebDriver session.
const SESSION_TIMEOUT_SECONDS = 120;

// WebView2 applications are driven through tauri-driver, which translates the
// tauri:options capability into the ms:edgeOptions the native driver expects.
// Talking to msedgedriver directly cannot work: it does not know what a
// "tauri:options" capability is, so the session never starts.
const TAURI_DRIVER_NAME = "tauri-driver";

// How long the directly launched application has to stay alive before it is
// considered healthy. A packaged Tauri application that cannot create its
// webview exits within a second or two, so this is far beyond the failure
// window while staying short enough for CI.
export const DIRECT_START_SECONDS = 15.0;

const DIAGNOSTIC_PREFIXES = [
  "WEBVIEW2",
  "TAURI_",
  "APPDATA",
  "LOCALAPPDATA",
  "RUST_",
];

// Every process this module starts, so Ctrl+C can stop them (Python's
// KeyboardInterrupt ran the `finally` blocks that did).
const started = new Set<ChildProcess>();

function track<T extends ChildProcess>(process: T): T {
  started.add(process);
  return process;
}

function killStarted(): void {
  for (const process of started) {
    if (!hasExited(process)) process.kill();
  }
}

/**
 * Starts a helper process with its output redirected to log files.
 *
 * On Windows this is Popen(stdout=..., stderr=...,
 * creationflags=CREATE_NEW_PROCESS_GROUP), through CreateProcessW: Bun.spawn
 * has no creation flags. The process group only keeps a console Ctrl+C from
 * reaching the child. Elsewhere, which is only ever a test of this module,
 * it is Bun.spawn with the same files.
 */
async function startLogged(
  argv: string[],
  environment: Record<string, string>,
  stdoutPath: string,
  stderrPath: string,
): Promise<ChildProcess> {
  if (IS_WINDOWS) {
    return spawnWithFlags(argv, {
      env: environment,
      creationFlags: CREATE_NEW_PROCESS_GROUP,
      redirect: { stdout: stdoutPath, stderr: stderrPath },
    });
  }
  const stdout = openSync(stdoutPath, "w");
  const stderr = stderrPath === stdoutPath ? stdout : openSync(stderrPath, "w");
  try {
    return Bun.spawn(argv, {
      env: environment,
      stdin: "inherit",
      stdout,
      stderr,
    });
  } finally {
    closeSync(stdout);
    if (stderr !== stdout) closeSync(stderr);
  }
}

function isFile(path: string): boolean {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

function isDirectory(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

/**
 * pathlib's `Path.resolve()` (strict=False): absolute, with symbolic links in
 * the part of the path that exists resolved, and the rest appended as is.
 */
function pyResolve(path: string): string {
  const absolute = resolve(path);
  let existing = absolute;
  const rest: string[] = [];
  while (!existsSync(existing)) {
    const parent = dirname(existing);
    if (parent === existing) return absolute;
    rest.unshift(basename(existing));
    existing = parent;
  }
  return join(realpathSync.native(existing), ...rest);
}

/** pathlib's `Path(...)` of an absolute path: separators and `.` normalised. */
function pyAbsolute(path: string): string {
  return isAbsolute(path) ? normalize(path) : join(process.cwd(), path);
}

/** `sorted(root.rglob('*'))` reduced to files: (relative path, size) pairs. */
function filesUnder(root: string): Array<[string, number]> {
  const files: Array<[string, number]> = [];
  let names: string[];
  try {
    names = readdirSync(root, { recursive: true }) as string[];
  } catch {
    return files;
  }
  for (const name of names) {
    const full = join(root, name);
    try {
      const info = statSync(full);
      if (info.isFile()) files.push([relative(root, full), info.size]);
    } catch {
      // Gone between listing and stat, which rglob would not have reported.
    }
  }
  // pathlib orders paths by their parts, case-insensitively on Windows.
  const key = (path: string) =>
    (IS_WINDOWS ? path.toLowerCase() : path).split(sep);
  files.sort(([a], [b]) => compareParts(key(a), key(b)));
  return files;
}

function compareParts(a: string[], b: string[]): number {
  for (let i = 0; i < Math.min(a.length, b.length); i++) {
    const x = a[i] as string;
    const y = b[i] as string;
    if (x !== y) return x < y ? -1 : 1;
  }
  return a.length - b.length;
}

function readTextReplace(path: string): string {
  return new TextDecoder("utf-8", { fatal: false }).decode(readFileSync(path));
}

/**
 * An environment in which this launch owns every piece of state it writes.
 *
 * The WebView2 runtime takes an exclusive lock inside its user-data
 * directory, and the application keeps its own settings under the roaming and
 * local application-data directories. Giving each launch its own copies means
 * a probe can never be blamed on, or interfere with, another launch.
 */
export function isolatedEnvironment(scratch: string): Record<string, string> {
  return {
    ...environ(),
    TAURI_WEBVIEW_AUTOMATION: "true",
    APPDATA: join(scratch, "appdata"),
    LOCALAPPDATA: join(scratch, "localappdata"),
    WEBVIEW2_USER_DATA_FOLDER: join(scratch, DIRECT_START_DIRECTORY),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--enable-logging --v=1",
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS_LOG: join(scratch, "webview2.log"),
  };
}

/**
 * Launches the application and requires it to still be running afterwards.
 *
 * This is the claim the whole Windows job rests on: a user double-clicks the
 * installed executable and the application opens. It is deliberately not a
 * driven session, so it holds even on a host where WebDriver cannot attach,
 * and it is deliberately fatal when it fails, because an application that
 * exits on start-up is broken regardless of what else the host cannot do.
 */
export async function runDirectStart(
  binary: string,
  output: string,
  isolationRoot: string,
  seconds: number = DIRECT_START_SECONDS,
): Promise<Result> {
  const facts = verifyPackagedApplication(binary);
  mkdirSync(output, { recursive: true });
  mkdirSync(isolationRoot, { recursive: true });
  const scratch = mkdtempSync(join(isolationRoot, "smm-direct-start-"));
  const environment = isolatedEnvironment(scratch);
  const stdoutPath = join(output, "direct-start-stdout.log");
  const stderrPath = join(output, "direct-start-stderr.log");

  const child = track(
    await startLogged([binary], environment, stdoutPath, stderrPath),
  );
  const forgetInterrupt = onInterrupt(killStarted);
  try {
    await sleep(seconds);
    if (hasExited(child)) {
      throw new AssertionError(
        `the packaged application exited with code ${returnCode(child)} within ` +
          `${pyG(seconds)} seconds of being launched`,
      );
    }
    const userData = join(scratch, DIRECT_START_DIRECTORY);
    if (!isDirectory(userData)) {
      throw new AssertionError(
        "the packaged application never created its WebView2 user-data " +
          `directory at ${userData}, so its window did not start`,
      );
    }
    return {
      status: "passed",
      alive_seconds: new PyFloat(seconds),
      webview2_user_data: userData,
      checks: [
        "packaged application present and a PE image",
        `application launched directly and stayed alive for ${pyG(seconds)} seconds`,
        "WebView2 runtime created its user-data directory",
      ],
      ...facts,
    };
  } catch (error) {
    writeDirectStartEvidence(
      output,
      stdoutPath,
      stderrPath,
      environment,
      scratch,
    );
    throw error;
  } finally {
    if (!hasExited(child)) {
      child.kill();
      if (!(await waitForExit(child, 10))) {
        child.kill("SIGKILL");
        await child.exited;
      }
    }
    forgetInterrupt();
    await removeTree(scratch);
  }
}

function environmentLines(environment: Record<string, string>): string[] {
  return sortedKeys(environment)
    .filter((name) =>
      DIAGNOSTIC_PREFIXES.some((prefix) => name.startsWith(prefix)),
    )
    .map((name) => `${name}=${environment[name]}`);
}

/** Everything needed to explain a start-up failure, written next to the result. */
function writeDirectStartEvidence(
  output: string,
  stdoutPath: string,
  stderrPath: string,
  environment: Record<string, string>,
  scratch: string,
): void {
  const lines = ["--- application stdout ---"];
  lines.push(...tail(stdoutPath));
  lines.push("--- application stderr ---");
  lines.push(...tail(stderrPath));
  lines.push("--- child environment ---");
  lines.push(...environmentLines(environment));
  lines.push("--- isolated scratch contents ---");
  for (const [path, size] of filesUnder(scratch)) {
    lines.push(`${path} (${size} bytes)`);
  }
  for (const logName of ["webview2.log"]) {
    const candidate = join(scratch, logName);
    if (isFile(candidate)) {
      lines.push(`--- ${logName} (tail) ---`);
      lines.push(...tail(candidate, 80));
    }
  }
  writeText(
    join(output, "direct-start-diagnostics.txt"),
    `${lines.join("\n")}\n`,
  );
}

function tail(path: string, lines = 40): string[] {
  if (!isFile(path)) return ["(none)"];
  const content = pySplitlines(readTextReplace(path));
  const last = content.slice(-lines);
  return last.length > 0 ? last : ["(empty)"];
}

/** The native WebDriver (msedgedriver) that tauri-driver proxies to. */
function findNativeDriver(): string | null {
  for (const name of NATIVE_DRIVER_NAMES) {
    const located = Bun.which(name);
    if (located) return located;
  }
  return null;
}

/** The tauri-driver proxy, which is what a WebView2 session needs. */
function findTauriDriver(): string | null {
  return Bun.which(TAURI_DRIVER_NAME) ?? null;
}

/**
 * The packaged executable, under either its package or its product name.
 *
 * The product name is accepted here only so a bundle that renames the binary
 * is still found; being tolerant of the file name is not the same as assuming
 * a directory layout.
 */
export function findApplication(searchRoot: string): string {
  const preferred = join(searchRoot, BINARY_NAME);
  if (isFile(preferred)) return preferred;
  let names: string[] = [];
  try {
    names = readdirSync(searchRoot);
  } catch {
    names = [];
  }
  // Path.glob('*.exe') matches the suffix case-insensitively on Windows.
  const candidates = names
    .filter((name) => (IS_WINDOWS ? name.toLowerCase() : name).endsWith(".exe"))
    .map((name) => join(searchRoot, name))
    .filter(isFile)
    .sort((a, b) => compareParts(partsKey(a), partsKey(b)))
    .filter((path) => !basename(path).startsWith("uninstall"));
  const first = candidates[0];
  if (first === undefined) {
    throw new AssertionError(
      `no packaged executable was found in ${searchRoot}`,
    );
  }
  return first;
}

function partsKey(path: string): string[] {
  return (IS_WINDOWS ? path.toLowerCase() : path).split(sep);
}

function writeWindowsGameFixture(game: string): void {
  mkdirSync(game, { recursive: true });
  // A native Windows installation: the game launcher, its managed assembly and
  // the runtime dependency manifests the inspector verifies.
  for (const name of [
    "Stardew Valley.exe",
    "Stardew Valley.dll",
    "Stardew Valley.deps.json",
    "StardewValley.GameData.dll",
  ]) {
    writeFileSync(
      join(game, name),
      Buffer.from([0x4d, 0x5a, 0x90, 0x00, ...Buffer.from("fixture")]),
    );
  }
  writeText(
    join(game, "Stardew Valley.deps.json"),
    '{"targets":{"net6.0":{"Stardew Valley/1.6.15.24356":{}}}}',
  );
}

/**
 * Checks the packaged binary without launching a webview.
 *
 * This is the part that always runs: it proves the executable is a real PE
 * image, which is what a truncated or failed build would not be. The frontend
 * needs no separate check because it is embedded in this file.
 */
export function verifyPackagedApplication(binary: string): Result {
  if (!isFile(binary)) {
    throw new AssertionError(
      `the packaged application executable is missing: ${binary}`,
    );
  }
  const header = readFileSync(binary).subarray(0, 2);
  if (header.toString("latin1") !== "MZ") {
    throw new AssertionError(
      `the packaged executable is not a PE image: ${binary}`,
    );
  }
  return {
    executable: binary,
    executable_bytes: statSync(binary).size,
  };
}

/**
 * Starts the driven session, retrying a transient start-up failure.
 *
 * A WebView2 session can fail to start while the previous instance of the
 * runtime is still shutting down, which a second attempt clears. The last
 * error is raised so a genuine failure is still reported.
 */
async function startSessionWithRetry(
  session: Session,
  binary: string,
  attempts = 3,
): Promise<void> {
  let last: unknown = null;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      await session.newSession(
        { "tauri:options": { application: binary } },
        SESSION_TIMEOUT_SECONDS,
      );
      return;
    } catch (error) {
      last = error;
      print(
        `session attempt ${attempt}/${attempts} failed: ${error instanceof Error ? error.message : error}`,
      );
      await sleep(5);
    }
  }
  throw last;
}

/**
 * Copies whatever the runtime and the application left behind.
 *
 * A WebView2 session that never starts is otherwise invisible: the driver only
 * reports that it could not find the debugging port. The environment the child
 * actually ran with, and anything the runtime logged, are written next to the
 * smoke output so the failure can be diagnosed from the artifact.
 */
function reportDiagnostics(
  scratch: string,
  output: string,
  environment: Record<string, string>,
): void {
  const lines = ["--- child environment ---"];
  lines.push(...environmentLines(environment));
  lines.push("--- scratch contents ---");
  for (const [path, size] of filesUnder(scratch)) {
    lines.push(`${path} (${size} bytes)`);
  }
  for (const logName of ["webview2.log"]) {
    const candidate = join(scratch, logName);
    if (isFile(candidate)) {
      lines.push(`--- ${logName} (tail) ---`);
      lines.push(...pySplitlines(readTextReplace(candidate)).slice(-80));
    }
  }
  writeText(
    join(output, "native-failure-diagnostics.txt"),
    `${lines.join("\n")}\n`,
  );
}

type Resolved = [
  string,
  Session,
  EmbeddedApplication | null,
  ChildProcess | null,
];

/**
 * Which WebDriver provider to use, and the objects that go with it.
 *
 * Returns (provider, session, application, driver). The application is the
 * embedded launch when there is one: it has to stay alive for the session to
 * work and has to be stopped afterwards. The driver is the external proxy
 * process, which the same is true of.
 */
async function resolveProvider(
  requested: string,
  binary: string,
  environment: Record<string, string>,
  output: string,
): Promise<Resolved> {
  if (
    requested !== "auto" &&
    !(PROVIDERS as readonly string[]).includes(requested)
  ) {
    throw new AssertionError(`unknown driver provider: ${requested}`);
  }

  if (requested === "auto" || requested === PROVIDER_EMBEDDED) {
    const port = await reservePort();
    const application = new EmbeddedApplication(binary, environment, port);
    await application.start();
    if (application.process !== null) track(application.process);
    const timeout =
      requested === PROVIDER_EMBEDDED
        ? EMBEDDED_READY_TIMEOUT_SECONDS
        : EMBEDDED_AUTODETECT_TIMEOUT_SECONDS;
    let ready = false;
    try {
      await application.waitUntilReady(timeout);
      ready = true;
    } catch (error) {
      if (!(error instanceof ProviderUnavailable)) throw error;
      await application.stop();
      if (requested === PROVIDER_EMBEDDED) throw error;
      print(
        "no embedded WebDriver server; falling back to the external driver",
      );
    }
    if (ready) {
      const session = new Session(port, output);
      await session.newSession({}, SESSION_TIMEOUT_SECONDS);
      return [PROVIDER_EMBEDDED, session, application, null];
    }
  }

  const nativeDriver = findNativeDriver();
  const tauriDriver = findTauriDriver();
  const missing: string[] = [];
  if (nativeDriver === null)
    missing.push("Microsoft Edge Driver (msedgedriver.exe)");
  if (tauriDriver === null)
    missing.push("tauri-driver (cargo install tauri-driver)");
  if (nativeDriver === null || tauriDriver === null) {
    throw new ProviderUnavailable(
      `the external WebDriver is not available: ${missing.join(" and ")}`,
    );
  }

  const port = await reservePort();
  const nativePort = await reservePort();
  const log = join(output, "native-smoke.log");
  const driver = track(
    await startLogged(
      [
        tauriDriver,
        `--port=${port}`,
        `--native-port=${nativePort}`,
        "--native-driver",
        nativeDriver,
      ],
      environment,
      log,
      log,
    ),
  );
  const session = new Session(port, output);
  await session.waitUntilReady();
  await startSessionWithRetry(session, binary);
  return [PROVIDER_EXTERNAL, session, null, driver];
}

// The provider a failed driven journey was using, kept for failureResult.
const failedProvider = new WeakMap<object, string | null>();

// How much of the error a failure result.json carries. The message of a
// journey failure includes the whole page text, which is where the
// application states what went wrong, so it is kept rather than cut to a line.
const FAILURE_REASON_LIMIT = 4000;

/**
 * The result.json of a run that failed.
 *
 * The workflow reads result.json after every run and reports its status and
 * reason, so a failure that left none behind was reported as a missing file
 * instead of as what actually happened. The run still exits 1.
 */
export function failureResult(error: unknown, directStart: boolean): Result {
  const message = error instanceof Error ? error.message : String(error);
  const result: Result = {
    status: "failed",
    reason: Array.from(message).slice(0, FAILURE_REASON_LIMIT).join(""),
  };
  if (!directStart) {
    result.driver_provider =
      typeof error === "object" && error !== null
        ? (failedProvider.get(error) ?? null)
        : null;
  }
  return result;
}

export interface WindowsOptions {
  requireWebview2?: boolean;
  directStart?: boolean;
  isolationRoot?: string | null;
  directStartSeconds?: number;
  driverProvider?: string;
}

export async function run(
  binaryArgument: string,
  output: string,
  options: WindowsOptions = {},
): Promise<Result> {
  const {
    directStart = false,
    isolationRoot = null,
    directStartSeconds = DIRECT_START_SECONDS,
    driverProvider = "auto",
  } = options;
  mkdirSync(output, { recursive: true });
  let binary = pyAbsolute(binaryArgument);
  binary = isFile(binary) ? binary : findApplication(dirname(binary));

  const facts = verifyPackagedApplication(binary);

  if (directStart) {
    // Absolute paths are not cosmetic here: the WebView2 runtime ignores a
    // relative WEBVIEW2_USER_DATA_FOLDER and silently writes to the shared
    // default profile instead, which would make the isolation a fiction.
    const root = pyResolve(isolationRoot ?? join(output, "webview2-isolation"));
    return runDirectStart(binary, output, root, directStartSeconds);
  }

  const scratch = mkdtempSync(join(tmpdir(), "smm-native-smoke-"));
  try {
    return await runDriven(binary, output, scratch, facts, driverProvider);
  } finally {
    await removeTree(scratch);
  }
}

/** Drives the installed application through whichever provider works here. */
async function runDriven(
  binary: string,
  output: string,
  scratch: string,
  facts: Result,
  driverProvider: string,
): Promise<Result> {
  const root = scratch;
  const game = join(root, "Stardew Valley");
  writeWindowsGameFixture(game);
  const archive = buildModArchive(join(root, "NativeSmoke.zip"));
  const environment = { ...isolatedEnvironment(root), RUST_LOG: "info" };

  let provider: string | null = null;
  let session: Session | null = null;
  let application: EmbeddedApplication | null = null;
  let driver: ChildProcess | null = null;
  const forgetInterrupt = onInterrupt(killStarted);
  try {
    [provider, session, application, driver] = await resolveProvider(
      driverProvider,
      binary,
      environment,
      output,
    );
    const result = await runUserJourney(session, game, archive);
    return { ...result, driver_provider: provider, ...facts };
  } catch (error) {
    const [status, reason] = classifyFailure(error);
    reportDiagnostics(root, output, environment);
    if (status === "failed") {
      if (session !== null) await captureFailure(session, output);
      if (typeof error === "object" && error !== null) {
        failedProvider.set(error, provider);
      }
      throw error;
    }
    const message = error instanceof Error ? error.message : String(error);
    return {
      status,
      driver_provider: provider,
      reason,
      checks: [
        "packaged application present and a PE image",
        "application started on this host",
      ],
      driver_error: Array.from(message).slice(0, 500).join(""),
      ...facts,
    };
  } finally {
    if (session !== null) await session.close();
    await stopProcess(application?.process ?? null);
    await stopProcess(driver);
    forgetInterrupt();
  }
}
