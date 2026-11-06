// Verify bun.lock against the release-age policy in bunfig.toml.
//
// Why: `bun install` applies `minimumReleaseAge` only when it resolves a
// version. A `--frozen-lockfile` install trusts bun.lock as written, so a
// lockfile edited by hand, or by a tool that ignores bunfig.toml, could pin a
// release published an hour ago and still install. pnpm's strict mode
// re-checked the lockfile on every install; this script restores that check.
// CI runs it straight after the frozen install.
//
// Usage:
//   bun scripts/verify-lockfile-age.ts [--min-age-seconds N]
//
//   --min-age-seconds N  check against N seconds instead of bunfig.toml's
//                        minimumReleaseAge (for testing the check itself)
//
// Policy: every registry entry in bun.lock must have been published at least
// `[install] minimumReleaseAge` seconds ago, unless
// `[install] minimumReleaseAgeExcludes` exempts it. Exemptions take the forms
// Bun accepts: "name", "name@1.2.3" and "name@1.2.3 || 1.2.4". Bun ignores any
// other form (with a warning), and so does this script. Entries that do not
// come from a registry (git, tarball, file or link dependencies) cannot be
// dated, so they fail unless exempted by name. Workspace entries are skipped.
//
// Network and offline behaviour: publish times come only from the registry's
// full package documents (the `time` field). Bun's cache does not keep them,
// so the check needs the network. Each request has a 30 s timeout and is
// tried three times. If a package document still cannot be fetched, the
// script stops fetching (so an offline run fails in seconds, not minutes) and
// fails closed with exit 2, naming the failed requests and how many packages
// went unchecked. It never reports success without having dated every entry,
// and there is deliberately no flag to skip it. Offline, re-run it once the
// network is back; in CI, exit 2 means "re-run the job", not "the lockfile
// is fine".
//
// Registry: an entry's own registry URL in bun.lock if it has one, otherwise
// bunfig.toml's `[install] registry`, then BUN_CONFIG_REGISTRY, then
// NPM_CONFIG_REGISTRY, then https://registry.npmjs.org. .npmrc is not read.
//
// Exit codes:
//   0  every entry is old enough or exempt
//   1  policy violation: an entry is too young, has no publish time, or
//      cannot be dated (each one is named)
//   2  the check could not run: unreadable bunfig.toml or bun.lock, a missing
//      or invalid minimumReleaseAge, bad arguments, or registry failures
import path from "node:path";

const ROOT = path.resolve(import.meta.dir, "..");
const BUNFIG = path.join(ROOT, "bunfig.toml");
const LOCKFILE = path.join(ROOT, "bun.lock");
const CONCURRENCY = 16;
const ATTEMPTS = 3;
const TIMEOUT_MS = 30_000;
const DAY = 86_400;

class CannotVerify extends Error {}

interface Group {
  registry: string;
  name: string;
  versions: string[];
}

function formatSeconds(seconds: number): string {
  if (Math.abs(seconds) >= DAY) return `${(seconds / DAY).toFixed(1)} days`;
  if (Math.abs(seconds) >= 3600) return `${(seconds / 3600).toFixed(1)} hours`;
  return `${Math.round(seconds)} s`;
}

function parseArgs(argv: string[]): number | undefined {
  let minAge: number | undefined;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    let value: string | undefined;
    if (arg === "--min-age-seconds") value = argv[++i];
    else if (arg.startsWith("--min-age-seconds="))
      value = arg.slice("--min-age-seconds=".length);
    else throw new CannotVerify(`unknown argument: ${arg}`);
    const seconds = Number(value);
    if (
      value === undefined ||
      value.trim() === "" ||
      !Number.isFinite(seconds) ||
      seconds < 0
    ) {
      throw new CannotVerify(
        `--min-age-seconds needs a non-negative number of seconds, got ${value}`,
      );
    }
    minAge = seconds;
  }
  return minAge;
}

async function readText(file: string): Promise<string> {
  try {
    return await Bun.file(file).text();
  } catch (error) {
    throw new CannotVerify(
      `cannot read ${path.relative(ROOT, file)}: ${(error as Error).message}`,
    );
  }
}

// Mirrors how Bun 1.4.4 reads minimumReleaseAgeExcludes.
function parseExcludes(
  raw: unknown,
): (name: string, version?: string) => boolean {
  const names = new Set<string>();
  const versions = new Map<string, Set<string>>();
  if (raw !== undefined && !Array.isArray(raw)) {
    throw new CannotVerify(
      "bunfig.toml: [install] minimumReleaseAgeExcludes must be an array",
    );
  }
  const exact = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
  for (const item of raw ?? []) {
    if (typeof item !== "string") {
      throw new CannotVerify(
        "bunfig.toml: [install] minimumReleaseAgeExcludes must hold strings",
      );
    }
    const at = item.lastIndexOf("@");
    if (at <= 0) {
      names.add(item);
      continue;
    }
    const list = item
      .slice(at + 1)
      .split("||")
      .map((v) => v.trim());
    if (!list.every((v) => exact.test(v))) {
      console.warn(
        `warning: ignoring minimumReleaseAgeExcludes entry "${item}": expected a package name or exact versions, as Bun does`,
      );
      continue;
    }
    const name = item.slice(0, at);
    const set = versions.get(name) ?? new Set<string>();
    for (const v of list) set.add(v);
    versions.set(name, set);
  }
  return (name, version) =>
    names.has(name) ||
    (version !== undefined && (versions.get(name)?.has(version) ?? false));
}

function defaultRegistry(fromBunfig: unknown): string {
  const configured =
    typeof fromBunfig === "string"
      ? fromBunfig
      : typeof (fromBunfig as { url?: unknown } | undefined)?.url === "string"
        ? (fromBunfig as { url: string }).url
        : undefined;
  return (
    configured ||
    process.env.BUN_CONFIG_REGISTRY ||
    process.env.NPM_CONFIG_REGISTRY ||
    process.env.npm_config_registry ||
    "https://registry.npmjs.org"
  ).replace(/\/+$/, "");
}

async function fetchPublishTimes(
  registry: string,
  name: string,
): Promise<Record<string, string>> {
  const url = `${registry}/${name.replaceAll("/", "%2f")}`;
  let lastError = "";
  for (let attempt = 1; attempt <= ATTEMPTS; attempt++) {
    try {
      const res = await fetch(url, {
        headers: { accept: "application/json" },
        signal: AbortSignal.timeout(TIMEOUT_MS),
      });
      if (res.ok) {
        const doc = (await res.json()) as { time?: Record<string, string> };
        return doc.time ?? {};
      }
      lastError = `HTTP ${res.status}`;
      // A 4xx other than rate limiting will not change on retry.
      if (res.status !== 429 && res.status < 500) break;
    } catch (error) {
      lastError = (error as Error).message;
    }
    if (attempt < ATTEMPTS) await Bun.sleep(1000 * attempt);
  }
  throw new CannotVerify(`${url}: ${lastError}`);
}

async function parseFile<T>(
  file: string,
  parse: (text: string) => unknown,
): Promise<T> {
  const text = await readText(file);
  try {
    return parse(text) as T;
  } catch (error) {
    throw new CannotVerify(
      `cannot parse ${path.relative(ROOT, file)}: ${(error as Error).message}`,
    );
  }
}

async function main(): Promise<number> {
  const minAgeOverride = parseArgs(process.argv.slice(2));

  const bunfig = await parseFile<{ install?: Record<string, unknown> }>(
    BUNFIG,
    Bun.TOML.parse,
  );
  const install = bunfig.install ?? {};
  const configured = install.minimumReleaseAge;
  let minAge: number;
  if (minAgeOverride !== undefined) {
    minAge = minAgeOverride;
  } else if (
    typeof configured === "number" &&
    Number.isFinite(configured) &&
    configured > 0
  ) {
    minAge = configured;
  } else {
    throw new CannotVerify(
      `bunfig.toml: [install] minimumReleaseAge must be a positive number of seconds, got ${String(configured)}. ` +
        "A missing policy is not treated as a pass.",
    );
  }
  const isExempt = parseExcludes(install.minimumReleaseAgeExcludes);
  const registry = defaultRegistry(install.registry);

  const lock = await parseFile<{ packages?: Record<string, unknown> }>(
    LOCKFILE,
    Bun.JSONC.parse,
  );
  if (!lock.packages || typeof lock.packages !== "object") {
    throw new CannotVerify("bun.lock has no packages table");
  }

  const failures: string[] = [];
  let exempt = 0;
  // One registry document per (registry, name), however many versions are locked.
  const groups = new Map<string, Group>();
  for (const [key, value] of Object.entries(lock.packages)) {
    const [spec, source, , integrity] = Array.isArray(value) ? value : [];
    if (typeof spec !== "string")
      throw new CannotVerify(`bun.lock: entry "${key}" has no name@version`);
    const at = spec.lastIndexOf("@");
    const name = at > 0 ? spec.slice(0, at) : spec;
    const version = at > 0 ? spec.slice(at + 1) : "";
    if (version.startsWith("workspace:")) continue;
    // Registry entries are [name@version, registryUrl, info, integrity] with a plain version.
    const fromRegistry =
      (value as unknown[]).length === 4 &&
      typeof source === "string" &&
      typeof integrity === "string" &&
      /^\d+\.\d+\.\d+/.test(version);
    if (!fromRegistry) {
      if (isExempt(name)) exempt++;
      else
        failures.push(
          `${spec}: not from a registry, so its age cannot be checked (exempt it by name to allow it)`,
        );
      continue;
    }
    if (isExempt(name, version)) {
      exempt++;
      continue;
    }
    const base = source ? source.replace(/\/+$/, "") : registry;
    const group = groups.get(`${base} ${name}`) ?? {
      registry: base,
      name,
      versions: [],
    };
    group.versions.push(version);
    groups.set(`${base} ${name}`, group);
  }

  const now = Date.now();
  const errors: string[] = [];
  let unchecked = 0;
  let checked = 0;
  let youngest: { spec: string; age: number } | undefined;
  const queue = [...groups.values()];
  async function worker(): Promise<void> {
    for (let group = queue.shift(); group; group = queue.shift()) {
      // After one unrecoverable failure the result can only be exit 2.
      if (errors.length) {
        unchecked += group.versions.length;
        continue;
      }
      let times: Record<string, string>;
      try {
        times = await fetchPublishTimes(group.registry, group.name);
      } catch (error) {
        errors.push((error as Error).message);
        unchecked += group.versions.length;
        continue;
      }
      for (const version of group.versions) {
        checked++;
        const spec = `${group.name}@${version}`;
        const published = Date.parse(times[version] ?? "");
        if (Number.isNaN(published)) {
          failures.push(
            `${spec}: the registry has no publish time for this version`,
          );
          continue;
        }
        const age = (now - published) / 1000;
        if (!youngest || age < youngest.age) youngest = { spec, age };
        if (age < minAge) {
          failures.push(
            `${spec}: published ${new Date(published).toISOString()}, ${formatSeconds(age)} ago; ` +
              `old enough from ${new Date(published + minAge * 1000).toISOString()}`,
          );
        }
      }
    }
  }
  await Promise.all(Array.from({ length: CONCURRENCY }, worker));

  const policy = `minimumReleaseAge ${minAge} s (${formatSeconds(minAge)})`;
  if (errors.length) {
    console.error(
      `verify-lockfile-age: bun.lock was NOT verified: ${unchecked} of ${checked + unchecked} entries went ` +
        "unchecked because their registry documents could not be fetched. Failed requests:",
    );
    const shown = errors.sort().slice(0, 5);
    for (const error of shown) console.error(`  ${error}`);
    if (errors.length > shown.length) {
      console.error(`  ... and ${errors.length - shown.length} more`);
    }
    console.error("This is not a pass. Re-run when the registry is reachable.");
    return 2;
  }
  if (failures.length) {
    const count =
      failures.length === 1 ? "1 entry" : `${failures.length} entries`;
    console.error(
      `bun.lock violates the release-age policy, ${policy}. ${count} too young or undatable:`,
    );
    for (const failure of failures.sort()) console.error(`  ${failure}`);
    console.error(
      "Wait until each release is old enough, or lock an older version. For an urgent fix, exempt the exact " +
        'version in bunfig.toml [install] minimumReleaseAgeExcludes (for example "name@1.2.3") with a comment ' +
        "giving the reason.",
    );
    return 1;
  }
  const youngestText = youngest
    ? `; youngest is ${youngest.spec}, ${formatSeconds(youngest.age)} old`
    : "";
  console.log(
    `bun.lock: ${checked} entries pass ${policy}, ${exempt} exempt${youngestText}`,
  );
  return 0;
}

try {
  process.exitCode = await main();
} catch (error) {
  if (!(error instanceof CannotVerify)) throw error;
  console.error(`verify-lockfile-age: ${error.message}`);
  process.exitCode = 2;
}
