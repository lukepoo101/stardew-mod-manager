#!/usr/bin/env bash
# Installs the system libraries Tauri builds need on Ubuntu CI runners.
#
# A stalled package mirror makes apt wait indefinitely, which holds a job
# until the runner's six-hour limit. Each attempt is therefore bounded and
# retried, so a mirror problem fails the step in minutes instead.
set -euo pipefail

packages=(
  libwebkit2gtk-4.1-dev
  libappindicator3-dev
  librsvg2-dev
  libxdo-dev
  libssl-dev
)
apt_options=(
  -o Acquire::Retries=3
  -o Acquire::http::Timeout=30
  -o Acquire::https::Timeout=30
)

for attempt in 1 2 3; do
  if timeout 300 sudo apt-get "${apt_options[@]}" update &&
    timeout 900 sudo apt-get "${apt_options[@]}" install -y "${packages[@]}"; then
    exit 0
  fi
  echo "Installing Linux dependencies failed (attempt ${attempt} of 3)." >&2
  sleep $((attempt * 20))
done
exit 1
