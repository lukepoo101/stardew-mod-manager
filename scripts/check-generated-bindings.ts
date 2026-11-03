// Regenerate the checked-in TypeScript DTO destination and fail on drift.
//
//   cargo test -p manager-app --locked   # ts-rs exports the source bindings
//   bun run bindings:check
//
// Copies crates/manager-app/bindings/*.ts over the checked-in destination,
// deletes destination files that no longer have a source (except index.ts),
// formats the destination with the Biome pinned in apps/desktop, then exits 1
// if git sees a modified, deleted or untracked file under the destination.
import { $ } from "bun";
import { mkdir, readdir } from "node:fs/promises";
import { realpathSync } from "node:fs";
import path from "node:path";

const ROOT = path.resolve(realpathSync(import.meta.dir), "..");
const SOURCE = path.join(ROOT, "crates", "manager-app", "bindings");
const DESKTOP = path.join(ROOT, "apps", "desktop");
// Forward slashes on purpose: git accepts them as pathspecs on every platform.
const DESTINATION_REL = "apps/desktop/src/shared/api/generated";
const DESTINATION = path.join(ROOT, ...DESTINATION_REL.split("/"));

// Python's Path.glob("*.ts") matched case-insensitively on Windows only.
const isTs =
  process.platform === "win32"
    ? (name: string) => name.toLowerCase().endsWith(".ts")
    : (name: string) => name.endsWith(".ts");

// Like Path.glob, a missing directory yields no names rather than an error.
async function tsNames(dir: string): Promise<string[]> {
  try {
    return (await readdir(dir)).filter(isTs);
  } catch (error) {
    if ((error as { code?: string }).code === "ENOENT") return [];
    throw error;
  }
}

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

// Biome's npm launcher picks the platform binary (biome.exe on Windows). It is
// taken from apps/desktop/node_modules only, so the version is the one pinned
// in apps/desktop/package.json and bun.lock, and it runs under this Bun with
// --no-install, so it needs no `node` on PATH and nothing is ever downloaded.
// The alternatives all find some other Biome when the pinned one is missing:
// `bun x biome` fetches the latest, `bun run biome` falls back to PATH, and
// Bun.resolveSync auto-installs from the global cache outside a project.
async function pinnedBiome(): Promise<string> {
  const launcher = path.join(
    DESKTOP,
    "node_modules",
    "@biomejs",
    "biome",
    "bin",
    "biome",
  );
  if (!(await Bun.file(launcher).exists())) {
    fail(
      "Biome is not installed in apps/desktop. Run `bun install --frozen-lockfile` first.",
    );
  }
  return launcher;
}

// Biome and `git diff` inherit the terminal, as they did under Python's
// subprocess.run: their output, colours and git's pager are unchanged.
async function run(cmd: string[], cwd: string): Promise<number> {
  const child = Bun.spawn(cmd, {
    cwd,
    stdio: ["inherit", "inherit", "inherit"],
  });
  return await child.exited;
}

await mkdir(DESTINATION, { recursive: true });

const generated = new Set(await tsNames(SOURCE));
for (const name of await tsNames(DESTINATION)) {
  if (name !== "index.ts" && !generated.has(name)) {
    await Bun.file(path.join(DESTINATION, name)).delete();
  }
}
for (const name of generated) {
  await Bun.write(
    path.join(DESTINATION, name),
    Bun.file(path.join(SOURCE, name)),
  );
}

const biomeExit = await run(
  [
    process.execPath,
    "--no-install",
    await pinnedBiome(),
    "format",
    "--write",
    "src/shared/api/generated",
  ],
  DESKTOP,
);
if (biomeExit !== 0) {
  fail(`biome format failed with exit code ${biomeExit}.`);
}

const changed = await run(
  ["git", "diff", "--exit-code", "--", DESTINATION_REL],
  ROOT,
);
const listed =
  await $`git ls-files --others --exclude-standard -- ${DESTINATION_REL}`
    .cwd(ROOT)
    .quiet()
    .nothrow();
if (listed.exitCode !== 0) {
  fail(
    `git ls-files failed with exit code ${listed.exitCode}: ${listed.stderr.toString().trim()}`,
  );
}
const untracked = listed.stdout.toString().trim();

if (changed !== 0 || untracked) {
  fail(
    "Generated DTOs are out of date. Run `cargo test -p manager-app`, " +
      "then `bun run bindings:check` and commit the result.",
  );
}
