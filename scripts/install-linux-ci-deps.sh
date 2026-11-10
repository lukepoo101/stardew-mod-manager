#!/usr/bin/env bash
# Installs the system libraries Tauri builds need on Ubuntu CI runners.
#
# A stalled package mirror makes apt wait indefinitely, which holds a job
# until the runner's six-hour limit. Each attempt is therefore bounded and
# retried, so a mirror problem fails the step in minutes instead.
#
# A healthy install takes about 25 seconds. The bounds are a few times that
# rather than many minutes: the runner's default mirror
# (azure.archive.ubuntu.com) regularly stalls on one package after another,
# and a generous bound let a single attempt run for a quarter of an hour
# before the retry, which then succeeded at once. After a failed attempt that
# mirror is dropped from the runner's mirror list, so the retry goes to
# archive.ubuntu.com instead of back to the mirror that just stalled.
set -euo pipefail

packages=(
  libwebkit2gtk-4.1-dev
  libappindicator3-dev
  librsvg2-dev
  libxdo-dev
  libssl-dev
)
apt_options=(
  -o Acquire::Retries=1
  -o Acquire::http::Timeout=15
  -o Acquire::https::Timeout=15
)
mirror_list=/etc/apt/apt-mirrors.txt
stalling_mirror='azure\.archive\.ubuntu\.com'

for attempt in 1 2 3; do
  if timeout 120 sudo apt-get "${apt_options[@]}" update &&
    timeout 240 sudo apt-get "${apt_options[@]}" install -y "${packages[@]}"; then
    exit 0
  fi
  echo "Installing Linux dependencies failed (attempt ${attempt} of 3)." >&2

  # Only when another mirror would be left: an empty list cannot install.
  if [ -f "$mirror_list" ] && grep -q "$stalling_mirror" "$mirror_list" &&
    grep -qv "$stalling_mirror" "$mirror_list"; then
    echo "Dropping the stalled mirror from ${mirror_list} for the retry." >&2
    sudo sed -i "/${stalling_mirror}/d" "$mirror_list"
  fi
  sleep $((attempt * 5))
done
exit 1
