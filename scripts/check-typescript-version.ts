// Fail when `bun check` would type check with a different TypeScript than the
// one apps/desktop pins.
//
// Why: `typecheck` and `build` run `bun check`, which uses the TypeScript
// compiled into the Bun binary (`bun -p process.versions.typescript`), not the
// `typescript` package in node_modules. Editors and Renovate still follow
// apps/desktop/package.json#devDependencies.typescript. If either moves
// without the other (a Bun upgrade, or a Renovate bump of `typescript`), the
// command line and the editor would silently disagree about which errors
// exist. This guard turns that drift into a failed `typecheck`.
//
// Usage:
//   bun scripts/check-typescript-version.ts
//
// Exit codes:
//   0  the versions are equal
//   1  they differ, or one of them is missing or not an exact version

import { join } from "node:path";

const manifestPath = join(
  import.meta.dir,
  "..",
  "apps",
  "desktop",
  "package.json",
);

async function main(): Promise<number> {
  const bundled = process.versions.typescript;
  const manifest = await Bun.file(manifestPath).json();
  const pinned: unknown = manifest?.devDependencies?.typescript;

  if (!bundled) {
    console.error(
      `check-typescript-version: this Bun (${Bun.version}) reports no bundled TypeScript ` +
        "(process.versions.typescript is empty), so `bun check` cannot be trusted to match.",
    );
    return 1;
  }
  if (
    typeof pinned !== "string" ||
    !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(pinned)
  ) {
    console.error(
      `check-typescript-version: ${manifestPath} must pin devDependencies.typescript ` +
        `to an exact version, found ${JSON.stringify(pinned)}.`,
    );
    return 1;
  }
  if (bundled !== pinned) {
    console.error(
      `check-typescript-version: bun check uses TypeScript ${bundled} (Bun ${Bun.version}), ` +
        `but apps/desktop pins typescript ${pinned}.\n` +
        "Upgrade Bun (.bun-version) and the typescript devDependency together, so that " +
        "`bun check` and editors report the same errors.",
    );
    return 1;
  }
  console.log(
    `TypeScript ${bundled}: bun check matches the apps/desktop devDependency.`,
  );
  return 0;
}

try {
  process.exitCode = await main();
} catch (error) {
  console.error(`check-typescript-version: ${(error as Error).message}`);
  process.exitCode = 1;
}
