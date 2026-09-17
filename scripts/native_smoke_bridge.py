"""Imports the native-smoke package from its hyphenated directory.

A python package directory cannot be imported by name when it contains a hyphen,
so this bridge loads it explicitly and re-exports its entry point.
"""
from __future__ import annotations

import importlib.util
from pathlib import Path
import sys

_PACKAGE_DIR = Path(__file__).resolve().parent / 'native-smoke'
_PACKAGE_NAME = 'scripts.native_smoke'


def _load_package() -> None:
    if _PACKAGE_NAME in sys.modules:
        return
    spec = importlib.util.spec_from_file_location(
        _PACKAGE_NAME,
        _PACKAGE_DIR / '__init__.py',
        submodule_search_locations=[str(_PACKAGE_DIR)],
    )
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise ImportError(f'cannot load the native-smoke package from {_PACKAGE_DIR}')
    module = importlib.util.module_from_spec(spec)
    sys.modules[_PACKAGE_NAME] = module
    spec.loader.exec_module(module)


_load_package()

from scripts.native_smoke.run import main as _main  # noqa: E402


def main(argv=None) -> int:
    return _main(argv)
