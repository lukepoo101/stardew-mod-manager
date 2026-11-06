// Linux packaged-application smoke test.
//
// Drives the packaged binary through WebKitWebDriver (WebKitGTK). A graphical
// session is required; CI provides one through xvfb-run.

import {
  chmodSync,
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import {
  buildModArchive,
  captureFailure,
  onInterrupt,
  type Result,
  runUserJourney,
  Session,
} from "./common.ts";
import { reservePort, waitForExit } from "./process.ts";
import { environ } from "./py.ts";

export const DEFAULT_BINARY = join("target", "release", "stardew-mod-manager");

function writeLinuxGameFixture(game: string): void {
  mkdirSync(game, { recursive: true });
  const native = join(game, "StardewValley");
  writeFileSync(native, "#!/bin/sh\nexit 0\n");
  chmodSync(native, 0o755);
  // The inspector requires the managed assembly and accepts a missing
  // version manifest, which is what a freshly copied installation looks like.
  for (const name of ["Stardew Valley.dll", "Stardew Valley.deps.json"]) {
    writeFileSync(join(game, name), "{}");
  }
}

export async function run(
  binaryArgument: string,
  output: string,
): Promise<Result> {
  mkdirSync(output, { recursive: true });
  const binary = isAbsolute(binaryArgument)
    ? binaryArgument
    : join(process.cwd(), binaryArgument);
  const root = mkdtempSync(join(tmpdir(), "smm-native-smoke-"));
  try {
    const game = join(root, "Stardew Valley");
    writeLinuxGameFixture(game);
    const archive = buildModArchive(join(root, "NativeSmoke.zip"));

    const port = await reservePort();

    const environment = {
      ...environ(),
      TAURI_WEBVIEW_AUTOMATION: "true",
      XDG_DATA_HOME: join(root, "data"),
      XDG_CACHE_HOME: join(root, "cache"),
      RUST_LOG: "info",
      GDK_BACKEND: "x11",
    };

    const log = openSync(join(output, "native-smoke.log"), "w");
    try {
      // `detached` starts the driver in a session of its own (setsid), as
      // start_new_session did, so the whole group - the driver and the
      // application it launched - can be stopped together.
      const driver = Bun.spawn(
        ["WebKitWebDriver", `--port=${port}`, "--host=127.0.0.1"],
        {
          env: environment,
          stdin: "inherit",
          stdout: log,
          stderr: log,
          detached: true,
        },
      );
      const killGroup = () => {
        try {
          process.kill(-driver.pid, "SIGTERM");
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
        }
      };
      const forgetInterrupt = onInterrupt(killGroup);
      const session = new Session(port, output);
      try {
        await session.waitUntilReady();
        await session.newSession({
          "webkitgtk:browserOptions": {
            binary,
            args: [],
          },
        });
        return await runUserJourney(session, game, archive);
      } catch (error) {
        await captureFailure(session, output);
        throw error;
      } finally {
        await session.close();
        killGroup();
        if (!(await waitForExit(driver, 5))) {
          driver.kill("SIGKILL");
          await driver.exited;
        }
        forgetInterrupt();
      }
    } finally {
      closeSync(log);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
