#!/usr/bin/env bash
# Builds the desktop app from the current checkout and replaces the per-user
# install at ~/.local/bin/stardew-mod-manager. Safe to run while the app is open:
# the new binary is moved into place atomically and takes effect on next launch.
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm install --frozen-lockfile
pnpm --filter desktop tauri build --no-bundle
target="${HOME}/.local/bin/stardew-mod-manager"
mkdir -p "$(dirname "$target")"
install -m 755 target/release/stardew-mod-manager "${target}.new"
mv -f "${target}.new" "$target"
echo "Installed $(git rev-parse --short HEAD) to $target"
