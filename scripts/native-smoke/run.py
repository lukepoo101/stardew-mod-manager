"""Run the packaged-application smoke test for the current platform.

Usage:
    python scripts/native-smoke.py --binary <path> --output <dir> [--require-webview2]

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
        '--require-webview2',
        action='store_true',
        help='Fail instead of skipping when Edge Driver is unavailable.',
    )
    args = parser.parse_args(argv)

    if sys.platform.startswith('win'):
        binary = args.binary or windows.DEFAULT_BINARY
        result = windows.run(binary, args.output, require_webview2=args.require_webview2)
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
