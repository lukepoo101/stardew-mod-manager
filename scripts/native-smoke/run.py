"""Run the packaged-application smoke test for the current platform.

Usage:
    python scripts/native-smoke.py --binary <path> --output <dir> \
        [--direct-start] [--require-webview2]

Two claims can be checked, and they are not the same claim:

* `--direct-start` launches the installed executable the way a user does and
  requires it to still be running after 15 seconds. It needs no driver, so it
  is the gate that always has to hold.
* `--require-webview2` additionally drives the application through Edge Driver
  and completes the user journey. It needs a host that lets the WebView2
  runtime initialise inside a WebDriver-launched process, which not every
  host permits, and it reports `blocked` rather than `failed` when that is
  what happened.

The platform module owns the driver and the fixture layout; the user journey is
shared, so a failure here means the packaged application is broken rather than
that two scripts disagreed.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import json
import sys

from . import linux, windows


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog='native-smoke')
    parser.add_argument('--binary', type=Path, default=None)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument(
        '--direct-start',
        action='store_true',
        help=('Launch the installed executable directly and require it to stay '
              'alive; makes no use of a WebDriver.'),
    )
    parser.add_argument(
        '--webview2-isolation',
        type=Path,
        default=None,
        help=(
            'Directory that holds one isolated WebView2 user-data directory per '
            'launch. Defaults to <output>/webview2-isolation.'
        ),
    )
    parser.add_argument(
        '--driver-provider',
        choices=('auto', 'embedded', 'external'),
        default='auto',
        help=(
            'How to drive the application. "embedded" uses the WebDriver server the '
            'application hosts itself behind the webdriver cargo feature, "external" '
            'uses tauri-driver and the platform driver, and "auto" prefers embedded '
            'and falls back. Only meaningful on Windows.'
        ),
    )
    parser.add_argument(
        '--require-webview2',
        action='store_true',
        help=(
            'Fail instead of skipping when no WebDriver provider is available, and '
            'report an environment that cannot host a WebView2 window as blocked '
            'rather than as a failure.'
        ),
    )
    args = parser.parse_args(argv)

    if sys.platform.startswith('win'):
        binary = args.binary or windows.DEFAULT_BINARY
        result = windows.run(
            binary,
            args.output,
            require_webview2=args.require_webview2,
            direct_start=args.direct_start,
            isolation_root=args.webview2_isolation,
            driver_provider=args.driver_provider,
        )
    else:
        binary = args.binary or linux.DEFAULT_BINARY
        result = linux.run(binary, args.output)

    args.output.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(result, indent=2) + '\n'
    (args.output / 'result.json').write_text(payload)
    for name in ('native-failure.png', 'native-failure.txt'):
        (args.output / name).unlink(missing_ok=True)
    print(payload, flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
