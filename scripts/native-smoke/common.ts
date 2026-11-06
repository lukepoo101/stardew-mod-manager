// Shared WebDriver client and user-journey assertions.
//
// The journey is identical on every platform - register a game, install SMAPI,
// install and remove a mod, create a profile - so it lives here once. A
// platform module supplies the driver, the launch capabilities and the fixture
// paths.

import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  AssertionError,
  KeyError,
  monotonic,
  print,
  pyRepr,
  sleep,
  TimeoutError,
  URLError,
  writeText,
} from "./py.ts";

/** The WebDriver session could not perform a requested step. */
export class DriverError extends Error {
  override name = "DriverError";
  readonly pyModule = "scripts.native_smoke.common";
}

/**
 * click_text never found the button enabled. Reported exactly as the
 * DriverError it was in Python; the subclass only lets submit_with_retry tell
 * it apart from a driver that failed.
 */
export class MissingButton extends DriverError {}

const CLICK_SCRIPT =
  "const button = [...document.querySelectorAll('button,a')]" +
  ".find(el => el.textContent.trim() === arguments[0]);" +
  "if (!button || button.disabled) return false;" +
  "button.click();" +
  "return true;";

/**
 * How many times click_text tries a click whose script call failed because the
 * page reloaded under it. Matches submit_with_retry's attempts.
 */
const CLICK_ATTEMPTS = 3;

/** A thin W3C WebDriver client over the driver's HTTP endpoint. */
export class Session {
  readonly base: string;
  sessionId: string | null = null;

  constructor(
    port: number,
    readonly output: string,
  ) {
    this.base = `http://127.0.0.1:${port}`;
  }

  // -- transport ---------------------------------------------------------
  async request(
    method: string,
    path: string,
    body?: unknown,
    timeout = 30,
  ): Promise<unknown> {
    let response: Response;
    let payload: unknown;
    try {
      response = await fetch(`${this.base}${path}`, {
        method,
        body: body === undefined ? undefined : JSON.stringify(body),
        headers: { "Content-Type": "application/json" },
        signal: AbortSignal.timeout(timeout * 1000),
        // urllib opened a connection per request; so does this.
        keepalive: false,
      });
      if (!response.ok) {
        throw new DriverError(await response.text());
      }
      payload = await response.json();
    } catch (error) {
      throw transportError(error);
    }
    if (
      payload === null ||
      typeof payload !== "object" ||
      !("value" in payload)
    ) {
      throw new KeyError("'value'");
    }
    return (payload as { value: unknown }).value;
  }

  async command(
    method: string,
    path: string,
    body?: unknown,
  ): Promise<unknown> {
    if (this.sessionId === null) {
      throw new DriverError("no active session");
    }
    return this.request(method, `/session/${this.sessionId}${path}`, body);
  }

  async waitUntilReady(attempts = 50): Promise<void> {
    for (let i = 0; i < attempts; i++) {
      try {
        await this.request("GET", "/status");
        return;
      } catch (error) {
        if (!(error instanceof URLError || error instanceof DriverError))
          throw error;
        await sleep(0.1);
      }
    }
    throw new DriverError("the WebDriver endpoint never became ready");
  }

  /**
   * Starts a session.
   *
   * Starting a WebView2 application is slower than a normal request: the
   * driver has to launch the process, wait for the runtime to initialise and
   * attach. The default request timeout is too short for that on a loaded CI
   * machine, and the resulting failure looks like a driver error rather than
   * a slow start.
   */
  async newSession(
    capabilities: Record<string, unknown>,
    timeout = 60,
  ): Promise<unknown> {
    const response = (await this.request(
      "POST",
      "/session",
      { capabilities: { alwaysMatch: capabilities } },
      timeout,
    )) as { sessionId?: string };
    if (
      response === null ||
      typeof response !== "object" ||
      !("sessionId" in response)
    ) {
      throw new KeyError("'sessionId'");
    }
    this.sessionId = response.sessionId as string;
    return response;
  }

  async close(): Promise<void> {
    if (this.sessionId === null) return;
    try {
      await this.request("DELETE", `/session/${this.sessionId}`);
    } catch {
      // Closing is best effort, as it was.
    }
    this.sessionId = null;
  }

  // -- page helpers ------------------------------------------------------
  script(code: string, ...values: unknown[]): Promise<unknown> {
    return this.command("POST", "/execute/sync", {
      script: code,
      args: values,
    });
  }

  async bodyText(): Promise<string> {
    return (await this.script("return document.body.innerText")) as string;
  }

  async waitForText(text: string, timeout = 20.0): Promise<string> {
    const deadline = monotonic() + timeout;
    let body = "";
    while (monotonic() < deadline) {
      body = await this.bodyText();
      if (body.includes(text)) return body;
      await sleep(0.15);
    }
    throw new AssertionError(`Missing ${pyRepr(text)}; page: ${body}`);
  }

  /**
   * Clicks the button or link with exactly this text.
   *
   * A click often follows a navigation, and the next page renders
   * asynchronously, so the button is waited for rather than required to
   * exist at once. A disabled button is waited on too: clicking it would
   * do nothing and the journey would stall at the next step.
   *
   * On a heavily loaded machine the page occasionally reloads while the click
   * script is running, and the driver answers that call with an error even
   * though nothing is wrong with the application. The page is marked before
   * the first try; when the click call fails and the mark is gone, the page
   * reloaded, so the click is tried again with a fresh wait, up to
   * CLICK_ATTEMPTS times in all. An error on a page that did not reload is
   * raised at once, exactly as before. (This retry is new in the Bun port; the
   * Python click_text failed the run on the first such error.)
   *
   * A button that never becomes clickable raises MissingButton, which is the
   * DriverError Python raised, with the same message.
   *
   * submit_with_retry passes `mark: false`, because it has marked the page
   * itself and reports reloads against that mark.
   */
  async clickText(
    text: string,
    timeout = 10.0,
    { mark = true } = {},
  ): Promise<void> {
    if (mark) await this.tryMarkPage();
    let attempt = 1;
    let deadline = monotonic() + timeout;
    while (true) {
      let clicked: unknown;
      try {
        clicked = await this.script(CLICK_SCRIPT, text);
      } catch (error) {
        if (
          !(error instanceof DriverError) ||
          attempt >= CLICK_ATTEMPTS ||
          !(await this.reloadedOrUnreadable())
        ) {
          throw error;
        }
        print(
          `click attempt ${attempt}: clicking ${pyRepr(text)} failed (the page reloaded); retrying`,
        );
        attempt += 1;
        await this.tryMarkPage();
        deadline = monotonic() + timeout;
        await sleep(0.15);
        continue;
      }
      if (clicked) return;
      if (monotonic() >= deadline) {
        throw new MissingButton(
          `Missing button: ${text}; page: ${await this.bodyText()}`,
        );
      }
      await sleep(0.15);
    }
  }

  async fill(selector: string, value: string): Promise<void> {
    // The GTK driver does not implement keyboard input on every build, so
    // the native DOM setter and an input event are used instead: React then
    // sees a real edit either way.
    await this.script(
      "const el = document.querySelector(arguments[0]);" +
        "if (!el) throw new Error('Missing input: ' + arguments[0]);" +
        "Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')" +
        ".set.call(el, arguments[1]);" +
        "el.dispatchEvent(new Event('input', {bubbles: true}));",
      selector,
      String(value),
    );
  }

  async buttonEnabled(text: string): Promise<boolean> {
    return Boolean(
      await this.script(
        "const button = [...document.querySelectorAll('button')]" +
          ".find(el => el.textContent.trim() === arguments[0]);" +
          "return !!button && !button.disabled;",
        text,
      ),
    );
  }

  async markPage(): Promise<void> {
    await this.script("window.__smokePageMark = true;");
  }

  async pageWasReloaded(): Promise<boolean> {
    return !(await this.script("return window.__smokePageMark === true;"));
  }

  /** Marks the page, tolerating a page that is mid-reload and cannot answer. */
  private async tryMarkPage(): Promise<void> {
    try {
      await this.markPage();
    } catch (error) {
      if (!(error instanceof DriverError)) throw error;
    }
  }

  /** Whether the page reloaded; a page too busy reloading to answer counts. */
  private async reloadedOrUnreadable(): Promise<boolean> {
    try {
      return await this.pageWasReloaded();
    } catch (error) {
      if (error instanceof DriverError) return true;
      throw error;
    }
  }

  /**
   * Fills an input, presses a button and waits for the result.
   *
   * On the Linux CI runner the WebKitGTK page occasionally reloads between
   * steps, which silently discards a filled controlled input: the button
   * then stays disabled and the click does nothing. Each attempt confirms
   * React accepted the value before clicking and, if the expected text never
   * appears, says whether the page reloaded before trying again.
   *
   * Under heavy load the input can also be discarded after the button was
   * seen enabled and before it is clicked: the app re-renders the view, with
   * or without a page reload, and the button goes back to disabled. Python's
   * click_text then waited out its timeout and the run failed with
   * "Missing button". The Bun port treats that like the "stayed disabled"
   * case - the value was discarded, so fill it again - and tries again within
   * the same attempts. When the last attempt fails that way, the same
   * DriverError is raised as before. (New in the Bun port.)
   */
  async submitWithRetry(
    selector: string,
    value: string,
    button: string,
    expected: string,
    attempts = 3,
    timeout = 20.0,
  ): Promise<string> {
    let lastError: AssertionError | DriverError | null = null;
    for (let attempt = 1; attempt <= attempts; attempt++) {
      await this.markPage();
      await this.fill(selector, value);
      const deadline = monotonic() + 5;
      while (!(await this.buttonEnabled(button)) && monotonic() < deadline) {
        await sleep(0.1);
      }
      if (!(await this.buttonEnabled(button))) {
        // The value was discarded (usually a page reload); fill again.
        lastError = new AssertionError(`${pyRepr(button)} stayed disabled`);
        print(
          `attempt ${attempt}: ${pyRepr(button)} stayed disabled` +
            `${(await this.pageWasReloaded()) ? " (the page reloaded)" : ""}; retrying`,
        );
        continue;
      }
      try {
        await this.clickText(button, 10.0, { mark: false });
      } catch (error) {
        if (!(error instanceof MissingButton)) throw error;
        // The value was discarded between the check and the click; fill again.
        lastError = error;
        print(
          `attempt ${attempt}: ${pyRepr(button)} was disabled again before the click` +
            `${(await this.pageWasReloaded()) ? " (the page reloaded)" : ""}; retrying`,
        );
        continue;
      }
      try {
        return await this.waitForText(expected, timeout);
      } catch (error) {
        if (!(error instanceof AssertionError)) throw error;
        lastError = error;
        const reloaded = await this.pageWasReloaded();
        print(
          `attempt ${attempt}: ${pyRepr(expected)} did not appear` +
            `${reloaded ? " (the page reloaded)" : ""}; retrying`,
        );
      }
    }
    if (lastError === null) throw new AssertionError("");
    throw lastError;
  }

  async screenshot(name: string): Promise<void> {
    const encoded = (await this.command("GET", "/screenshot")) as string;
    writeFileSync(join(this.output, name), Buffer.from(encoded, "base64"));
  }

  async url(): Promise<string> {
    return (await this.command("GET", "/url")) as string;
  }

  async assertEmbeddedFrontend(): Promise<string> {
    const url = await this.url();
    if (url.includes(":1420")) {
      throw new AssertionError(
        `the packaged application served the dev server: ${url}`,
      );
    }
    if (
      !["tauri:", "http://tauri.localhost", "https://tauri.localhost"].some(
        (p) => url.startsWith(p),
      )
    ) {
      throw new AssertionError(url);
    }
    if (!(await this.script("return !!window.__TAURI_INTERNALS__"))) {
      throw new AssertionError("the Tauri IPC bridge is missing");
    }
    return url;
  }
}

function transportError(error: unknown): Error {
  if (
    error instanceof DriverError ||
    error instanceof KeyError ||
    error instanceof URLError ||
    error instanceof TimeoutError
  ) {
    return error;
  }
  if (
    error instanceof Error &&
    (error.name === "TimeoutError" || error.name === "AbortError")
  ) {
    return new TimeoutError("timed out", { cause: error });
  }
  if (error instanceof SyntaxError) {
    // json.load on a body that is not JSON raised a ValueError.
    return error;
  }
  const reason = error instanceof Error ? error.message : String(error);
  return new URLError(`<urlopen error ${reason}>`, { cause: error });
}

// -- the mod fixture -------------------------------------------------------

/**
 * A stored (uncompressed) ZIP laid out exactly as Python's
 * `zipfile.ZipFile.writestr` writes one: version 2.0, local time, mode 0600,
 * made on Unix (or FAT on Windows), no extra fields.
 */
function storedZip(entries: Array<[string, string]>): Uint8Array {
  const now = new Date();
  const dosTime =
    (now.getHours() << 11) |
    (now.getMinutes() << 5) |
    Math.floor(now.getSeconds() / 2);
  const dosDate =
    ((now.getFullYear() - 1980) << 9) |
    ((now.getMonth() + 1) << 5) |
    now.getDate();
  const createSystem = process.platform === "win32" ? 0 : 3;
  const encoder = new TextEncoder();
  const locals: Uint8Array[] = [];
  const centrals: Uint8Array[] = [];
  let offset = 0;
  for (const [name, text] of entries) {
    const nameBytes = encoder.encode(name);
    const data = encoder.encode(text);
    const crc = Bun.hash.crc32(data);
    // biome-ignore lint/suspicious/noControlCharactersInRegex: matching non-ASCII names
    const flags = /^[\x00-\x7f]*$/.test(name) ? 0 : 0x800;

    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true);
    local.setUint16(6, flags, true);
    local.setUint16(8, 0, true);
    local.setUint16(10, dosTime, true);
    local.setUint16(12, dosDate, true);
    local.setUint32(14, crc, true);
    local.setUint32(18, data.length, true);
    local.setUint32(22, data.length, true);
    local.setUint16(26, nameBytes.length, true);
    local.setUint16(28, 0, true);
    const localBytes = concat([new Uint8Array(local.buffer), nameBytes, data]);
    locals.push(localBytes);

    const central = new DataView(new ArrayBuffer(46));
    central.setUint32(0, 0x02014b50, true);
    central.setUint8(4, 20);
    central.setUint8(5, createSystem);
    central.setUint16(6, 20, true);
    central.setUint16(8, flags, true);
    central.setUint16(10, 0, true);
    central.setUint16(12, dosTime, true);
    central.setUint16(14, dosDate, true);
    central.setUint32(16, crc, true);
    central.setUint32(20, data.length, true);
    central.setUint32(24, data.length, true);
    central.setUint16(28, nameBytes.length, true);
    central.setUint16(30, 0, true);
    central.setUint16(32, 0, true);
    central.setUint16(34, 0, true);
    central.setUint16(36, 0, true);
    central.setUint32(38, (0o600 << 16) >>> 0, true);
    central.setUint32(42, offset, true);
    centrals.push(concat([new Uint8Array(central.buffer), nameBytes]));
    offset += localBytes.length;
  }
  const directory = concat(centrals);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, entries.length, true);
  end.setUint16(10, entries.length, true);
  end.setUint32(12, directory.length, true);
  end.setUint32(16, offset, true);
  return concat([...locals, directory, new Uint8Array(end.buffer)]);
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(
    parts.reduce((total, part) => total + part.length, 0),
  );
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

/** A minimal valid SMAPI mod archive, identical on every platform. */
export function buildModArchive(archive: string): string {
  // json.dumps's default separators, so the bytes match the Python fixture.
  const manifest =
    '{"Name": "Native smoke mod", "Author": "Tests", "Version": "1.0.0", ' +
    '"UniqueID": "Tests.NativeSmoke", "EntryDll": "Smoke.dll"}';
  writeFileSync(
    archive,
    storedZip([
      ["Smoke/manifest.json", manifest],
      ["Smoke/Smoke.dll", "fixture"],
    ]),
  );
  return archive;
}

// Installing SMAPI runs the upstream installer as a child process, which does
// real filesystem work and may take tens of seconds. The default wait is sized
// for a UI transition, not for an installation.
export const INSTALL_TIMEOUT_SECONDS = 180;

/**
 * Removes a scratch directory that a just-stopped process may still hold.
 *
 * Stopping the application does not stop the WebView2 runtime's helper
 * processes in the same instant, and they hold open files inside the
 * user-data directory. Failing the whole run over that would replace a real
 * result with a housekeeping error, and on Windows a locked file is a
 * transient condition rather than a statement about correctness.
 */
export async function removeTree(
  path: string,
  attempts = 20,
  delay = 0.5,
): Promise<void> {
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      rmSync(path, { recursive: true });
      return;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
      if (attempt === attempts) {
        print(
          `warning: could not remove the scratch directory ${path}; ` +
            "a WebView2 helper process is still holding a file in it",
        );
        return;
      }
      await sleep(delay);
    }
  }
}

export type Result = Record<string, unknown>;

/**
 * The end-to-end flow that proves the packaged application is functional.
 *
 * Every step here assumes the application owns a console. Reaching the SMAPI
 * step through an external driver instead - tauri-driver and msedgedriver,
 * which launch the application with no console - stops at that step, because
 * the upstream installer requires a console and its own error path calls
 * Console.ReadKey(). See docs/windows-verification.md.
 */
export async function runUserJourney(
  session: Session,
  game: string,
  archive: string,
): Promise<Result> {
  await session.waitForText("Locate Stardew Valley");
  const url = await session.assertEmbeddedFrontend();
  await session.screenshot("native-onboarding.png");

  await session.fill('input[aria-label="Game installation folder"]', game);
  await session.clickText("Validate & Continue");
  await session.waitForText("Set up modding");
  // Install is enabled only once the setup preview and its access checks
  // have loaded; clicking earlier does nothing.
  await session.waitForText("Will change the game folder");
  await session.waitForText("Access checks");
  await session.clickText("Install SMAPI");
  await session.waitForText("Ready to Mod!", INSTALL_TIMEOUT_SECONDS);
  await session.clickText("Go to Dashboard");
  await session.waitForText("Ready to Play");

  await session.clickText("Mods");
  await session.waitForText("Installed Mods");
  await session.submitWithRetry(
    'input[placeholder^="Or paste path"]',
    archive,
    "Inspect",
    "Review mod installation",
  );
  await session.clickText("Install mod");
  await session.waitForText("Installed 1 mod");
  await session.waitForText("Tests.NativeSmoke");
  await session.clickText("Done");
  await session.waitForText("1 mod(s) in active profile");
  await session.waitForText("Native smoke mod");
  await session.screenshot("native-installed-mod.png");

  await session.script(
    "document.querySelector('button[title=\"Remove mod\"]').click()",
  );
  await session.waitForText("Review mod removal");
  await session.clickText("Remove mod");
  await session.waitForText("No mods in this profile yet");

  await session.clickText("Profiles");
  await session.clickText("New Profile");
  await session.fill(
    'input[placeholder^="Profile name"]',
    "Native smoke profile",
  );
  await session.clickText("Create");
  await session.waitForText("Native smoke profile");
  await session.screenshot("native-profiles.png");

  return {
    status: "passed",
    url,
    checks: [
      "embedded frontend startup",
      "real IPC available",
      "manual game registration and default profile",
      "onboarding completion",
      "SMAPI install and verification",
      "mod ZIP inspection, preview and install",
      "mod removal",
      "profile creation and cache refresh",
    ],
  };
}

export async function captureFailure(
  session: Session,
  output: string,
): Promise<void> {
  try {
    await session.screenshot("native-failure.png");
    writeText(join(output, "native-failure.txt"), await session.bodyText());
  } catch {
    // Evidence is best effort; the original error is what gets reported.
  }
}

/**
 * Runs `cleanup` if the run is interrupted with Ctrl+C, then exits 130.
 *
 * Python raised KeyboardInterrupt, which ran every `finally` and so stopped
 * the driver and the application. Bun exits on SIGINT without unwinding, and
 * the driver runs in a session of its own that Ctrl+C does not reach, so the
 * cleanup is registered explicitly. Returns the function that unregisters it.
 */
export function onInterrupt(cleanup: () => void): () => void {
  const handler = () => {
    try {
      cleanup();
    } finally {
      process.exit(130);
    }
  };
  process.on("SIGINT", handler);
  return () => process.off("SIGINT", handler);
}
