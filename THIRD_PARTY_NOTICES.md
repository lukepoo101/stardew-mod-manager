# Third-party materials

The manager’s original source is MIT licensed; see LICENSE. Dependencies keep their own licenses. This file is a provenance guide, not a blanket relicensing of dependencies.

- Tauri, React, TypeScript, Vite, Tailwind CSS, Radix UI and the Rust/npm dependency trees are external projects. Bun, jsdom, Testing Library and Biome are development tools and are not shipped in the application. Exact resolved versions are recorded in Cargo.lock and bun.lock. Preserve required notices when distributing bundled binaries.
- SMAPI is downloaded at runtime from Pathoschild/SMAPI, not included as source or an installer archive in this repository. The pinned URL/checksum and version live in `crates/manager-core/src/smapi/mod.rs`. Verify upstream provenance during release acceptance.
- Stardew Valley and third-party mod files are not distributed by this repository. Synthetic tests are original fixtures; optional real-mod tests require user-supplied archives.
- The checked-in app icon is a plain color placeholder. No game artwork is intentionally included.

Before a binary release, inventory the resolved dependency licenses and include any required redistribution notices with the package. Record the result in the release checklist. This check remains outstanding until performed; the application’s MIT license alone does not complete it.
