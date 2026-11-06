// Entry point for `bun run test`: runs `bun test --parallel` on half the
// cores, passing any extra arguments (a file filter, --coverage) through.
//
// One jsdom worker per core starved each other on a busy machine and timed
// out tests that pass alone; half the cores keeps a full run fast without
// that. CI runners have few cores, so little changes there. Bun's --parallel
// takes only a worker count and bunfig.toml has no key for it, so the count
// is worked out here, as Vitest's `maxWorkers: "50%"` did.
import { availableParallelism } from "node:os";
import path from "node:path";

const workers = Math.max(1, Math.floor(availableParallelism() / 2));
const child = Bun.spawn(
  [process.execPath, "test", `--parallel=${workers}`, ...process.argv.slice(2)],
  {
    cwd: path.resolve(import.meta.dir, "../.."),
    stdio: ["inherit", "inherit", "inherit"],
  },
);

// Pass a stop request on, then report the child's result (128 + the signal
// number if it was stopped, as a shell would).
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => child.kill(signal));
}

process.exit(await child.exited);
