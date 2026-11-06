// How the Windows smoke test starts a WebDriver session.
//
// Two providers can drive the packaged application, and they fail for
// completely different reasons:
//
// * `embedded` - the application itself hosts a W3C WebDriver server
//   (`tauri-plugin-wdio-webdriver`, behind the `webdriver` cargo feature) and
//   drives its own WebView2 through the runtime's native API. Nothing about
//   another process attaching to the application is involved.
// * `external` - `tauri-driver` proxies `msedgedriver`, which launches the
//   application and attaches over the DevTools port. A host that refuses to
//   let that happen produces the same error regardless of the application.
//
// The distinction is the whole point: the second provider is the one a hosted
// CI runner can defeat, so the smoke test reports which one it used and why it
// could not be used, instead of reporting an environment failure as if it were
// an application failure.

import {
  type ChildProcess,
  hasExited,
  returnCode,
  stopProcess,
} from "./process.ts";
import { AssertionError, IS_WINDOWS, monotonic, pyG, sleep } from "./py.ts";
import { CREATE_NEW_CONSOLE, spawnWithFlags } from "./win32-process.ts";

export { reservePort } from "./process.ts";

/** The provider cannot be used here, and the reason is not the app. */
export class ProviderUnavailable extends Error {
  override name = "ProviderUnavailable";
  readonly pyModule = "scripts.native_smoke.windows_driver";
}

export const PROVIDER_EMBEDDED = "embedded";
export const PROVIDER_EXTERNAL = "external";
export const PROVIDERS = [PROVIDER_EMBEDDED, PROVIDER_EXTERNAL] as const;

// The environment variable the embedded server reads its port from. It is how
// @wdio/tauri-service configures the provider, so setting it is the whole
// contract rather than a private arrangement with this script.
export const EMBEDDED_PORT_VARIABLE = "TAURI_WEBDRIVER_PORT";

// The embedded server is a thread inside the application, so it is ready a
// moment after the window is. This is only a bound on how long to wait.
export const EMBEDDED_READY_TIMEOUT_SECONDS = 30.0;

// When the provider is auto-detected, the application is given a short head
// start: a build without the feature never listens at all, and waiting the
// full timeout before falling back would only slow the run down.
export const EMBEDDED_AUTODETECT_TIMEOUT_SECONDS = 12.0;

/**
 * Whether a GET /status response body is an embedded WebDriver server.
 *
 * The embedded provider and the external one both answer /status, and a
 * wrong guess is expensive: connecting to the proxy port with the embedded
 * protocol produces a session error that looks like an application defect.
 */
export function classifyEmbeddedResponse(payload: unknown): boolean {
  if (!isDict(payload)) return false;
  const value = "value" in payload ? payload.value : payload;
  if (!isDict(value) || !("ready" in value)) return false;
  const message = "message" in value ? pyStr(value.message) : "";
  return message.toLowerCase().includes("webdriver");
}

function isDict(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** str() of a JSON value, for the one place a non-string message is read. */
function pyStr(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === null) return "None";
  if (value === true) return "True";
  if (value === false) return "False";
  return JSON.stringify(value);
}

export async function embeddedServerPresent(
  port: number,
  timeout = 1.0,
): Promise<boolean> {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/status`, {
      signal: AbortSignal.timeout(timeout * 1000),
      keepalive: false,
    });
    // urlopen raised HTTPError (a URLError) for any non-2xx answer.
    if (!response.ok) return false;
    return classifyEmbeddedResponse(await response.json());
  } catch {
    return false;
  }
}

/**
 * The packaged application, launched with its WebDriver server enabled.
 *
 * The application is given a console, exactly as the Windows console
 * adapter gives one to the SMAPI installer: that installer calls
 * Console.ReadKey() when it fails, which throws when no console exists, and
 * the real error is then replaced by the symptom.
 *
 * On Windows it is started as Popen(creationflags=CREATE_NEW_CONSOLE)
 * started it, through CreateProcessW (see win32-process.ts): a new console
 * and no redirected standard handles. The application passes its own handles
 * on to the installer, so redirecting them here, which Bun.spawn always does,
 * takes the installer's console away and it exits with 0xE0434352. Its output
 * goes to that console, as it did under Python, and is not captured.
 */
export class EmbeddedApplication {
  readonly environment: Record<string, string>;
  process: ChildProcess | null = null;

  constructor(
    readonly binary: string,
    environment: Record<string, string>,
    readonly port: number,
  ) {
    this.environment = {
      ...environment,
      [EMBEDDED_PORT_VARIABLE]: String(port),
    };
  }

  async start(): Promise<void> {
    if (IS_WINDOWS) {
      this.process = await spawnWithFlags([this.binary], {
        env: this.environment,
        creationFlags: CREATE_NEW_CONSOLE,
      });
      return;
    }
    this.process = Bun.spawn([this.binary], {
      env: this.environment,
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
    });
  }

  async waitUntilReady(
    timeout: number = EMBEDDED_READY_TIMEOUT_SECONDS,
  ): Promise<void> {
    const deadline = monotonic() + timeout;
    while (monotonic() < deadline) {
      if (this.process !== null && hasExited(this.process)) {
        throw new AssertionError(
          `the packaged application exited with code ${returnCode(this.process)} ` +
            "before its WebDriver server became ready",
        );
      }
      if (await embeddedServerPresent(this.port)) return;
      await sleep(0.25);
    }
    throw new ProviderUnavailable(
      "the packaged application did not start an embedded WebDriver server on " +
        `port ${this.port} within ${pyG(timeout)} seconds, so it was built without the ` +
        "webdriver feature",
    );
  }

  async stop(): Promise<void> {
    await stopProcess(this.process);
  }
}

// What .NET's Console.ReadKey() throws when the process has no console,
// which the upstream SMAPI installer's own error path reaches when an
// external driver launched the application (docs/windows-verification.md).
// The Python original used this marker without ever defining it, so every
// failure other than ProviderUnavailable crashed instead of being classified.
const SMAPI_WITHOUT_CONSOLE_MARKER =
  "Cannot read keys when either application does not have a console";

/**
 * Whether a failed driven session says something about the application.
 *
 * Three conditions are statements about the host rather than the product:
 * the application never created a debugging port for an external driver to
 * attach to, the application has no embedded WebDriver server at all, and an
 * external driver launched it without the console the upstream SMAPI
 * installer requires. Everything else - including a journey that starts and
 * then fails - is about the product.
 */
export function classifyFailure(error: unknown): [string, string] {
  if (error instanceof ProviderUnavailable) {
    return ["blocked", error.message];
  }
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes(SMAPI_WITHOUT_CONSOLE_MARKER)) {
    return [
      "blocked",
      "the application was launched by an external driver, which gives it no " +
        "console; the upstream SMAPI installer requires one, so the journey could " +
        "not be completed under this provider",
    ];
  }
  if (message.includes("DevToolsActivePort")) {
    return [
      "blocked",
      "the WebView2 runtime did not initialise in the driver-launched application, " +
        "so no WebDriver session could be created; this host cannot drive a WebView2 " +
        "window through an external driver",
    ];
  }
  return ["failed", message];
}
