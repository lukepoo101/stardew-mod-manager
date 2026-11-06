// Expose the repository toolchain as a GitHub Actions step output.
//
//   bun scripts/read-rust-toolchain.ts >> "$GITHUB_OUTPUT"
//
// Prints `toolchain=<channel>` from rust-toolchain.toml and exits 0. Exits 1
// when the file is missing or unparsable, or when the channel is not a single
// non-empty line.
import { realpathSync } from "node:fs";
import { EOL } from "node:os";
import path from "node:path";

const ROOT = path.resolve(realpathSync(import.meta.dir), "..");

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

const source = path.join(ROOT, "rust-toolchain.toml");
let manifest: unknown;
try {
  manifest = Bun.TOML.parse(await Bun.file(source).text());
} catch (error) {
  fail(`Could not read ${source}: ${(error as Error).message}`);
}

const toolchain = (manifest as { toolchain?: unknown }).toolchain;
const channel =
  typeof toolchain === "object" && toolchain !== null
    ? (toolchain as { channel?: unknown }).channel
    : undefined;
if (typeof channel !== "string" || !channel || /[\r\n]/.test(channel)) {
  fail("rust-toolchain.toml must contain a single toolchain channel");
}

// EOL keeps the bytes the Python version printed: Python's text-mode stdout
// writes "\r\n" on Windows. The Actions runner accepts either ending in
// $GITHUB_OUTPUT.
process.stdout.write(`toolchain=${channel}${EOL}`);
