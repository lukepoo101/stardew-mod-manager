"""Linux packaged-application smoke test.

Drives the packaged binary through WebKitWebDriver (WebKitGTK). A graphical
session is required; CI provides one through xvfb-run.
"""
from __future__ import annotations

import os
from pathlib import Path
import signal
import subprocess
import tempfile

from .common import Session, build_mod_archive, capture_failure, run_user_journey

DEFAULT_BINARY = Path('target/release/stardew-mod-manager')


def _write_linux_game_fixture(game: Path) -> None:
    game.mkdir(parents=True)
    native = game / 'StardewValley'
    native.write_text('#!/bin/sh\nexit 0\n')
    native.chmod(0o755)
    # The inspector requires the managed assembly and accepts a missing
    # version manifest, which is what a freshly copied installation looks like.
    for name in ['Stardew Valley.dll', 'Stardew Valley.deps.json']:
        (game / name).write_text('{}')


def run(binary: Path, output: Path) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    binary = binary if binary.is_absolute() else (Path.cwd() / binary)
    with tempfile.TemporaryDirectory(prefix='smm-native-smoke-') as temporary:
        root = Path(temporary)
        game = root / 'Stardew Valley'
        _write_linux_game_fixture(game)
        archive = build_mod_archive(root / 'NativeSmoke.zip')

        from socket import socket

        with socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]

        environment = dict(
            os.environ,
            TAURI_WEBVIEW_AUTOMATION='true',
            XDG_DATA_HOME=str(root / 'data'),
            XDG_CACHE_HOME=str(root / 'cache'),
            RUST_LOG='info',
            GDK_BACKEND='x11',
        )

        log_path = output / 'native-smoke.log'
        with log_path.open('w') as log:
            driver = subprocess.Popen(
                ['WebKitWebDriver', f'--port={port}', '--host=127.0.0.1'],
                env=environment,
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
            session = Session(port, output)
            try:
                session.wait_until_ready()
                session.new_session(
                    {
                        'webkitgtk:browserOptions': {
                            'binary': str(binary),
                            'args': [],
                        }
                    }
                )
                return run_user_journey(session, game, archive, output)
            except Exception:
                capture_failure(session, output)
                raise
            finally:
                session.close()
                try:
                    os.killpg(driver.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    driver.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    driver.kill()
                    driver.wait()
