#!/usr/bin/env python3
"""Entry point for the packaged-application smoke test.

The implementation lives in the native-smoke package: this file exists so the
documented invocation keeps working from the repository root, and so the
platform dispatch happens in one place.
"""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from scripts.native_smoke_bridge import main  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(main())
