// Process and port helpers shared by the platform modules.

import { createServer } from "node:net";
import { constants } from "node:os";
import type { Subprocess } from "bun";

/** A free loopback port: bound, read and released, as socket.bind(0) did. */
export function reservePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port =
        typeof address === "object" && address !== null ? address.port : 0;
      server.close(() => resolve(port));
    });
  });
}

/** Whether the process has exited (Popen.poll() is not None). */
export function hasExited(process: Subprocess): boolean {
  return process.exitCode !== null || process.signalCode !== null;
}

/** Popen.returncode: the exit code, or minus the signal number. */
export function returnCode(process: Subprocess): number | null {
  if (process.exitCode !== null) return process.exitCode;
  if (process.signalCode !== null) {
    return -(
      (constants.signals as Record<string, number>)[process.signalCode] ?? 0
    );
  }
  return null;
}

/** Popen.wait(timeout): true if the process exited within `seconds`. */
export async function waitForExit(
  process: Subprocess,
  seconds: number,
): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timedOut = new Promise<false>((resolve) => {
    timer = setTimeout(() => resolve(false), seconds * 1000);
  });
  try {
    return await Promise.race([
      process.exited.then(() => true as const),
      timedOut,
    ]);
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Stops a process this run started, without waiting forever for it:
 * terminate, ten seconds' grace, then kill. On Windows both are
 * TerminateProcess, as they were in Python.
 */
export async function stopProcess(
  process: Subprocess | null | undefined,
): Promise<void> {
  if (process === null || process === undefined || hasExited(process)) return;
  process.kill("SIGTERM");
  if (!(await waitForExit(process, 10))) {
    process.kill("SIGKILL");
    await process.exited;
  }
}
