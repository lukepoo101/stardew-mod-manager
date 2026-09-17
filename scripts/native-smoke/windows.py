"""Windows packaged-application smoke test.

Windows drives a WebView2 application through Microsoft Edge Driver rather than
WebKitWebDriver, so the driver process, its capabilities and the fixture layout
all differ. The user journey itself is shared.

Two levels are supported:

* \`--require-webview2\` (default off) fails when Edge Driver is unavailable, and
  is what the release pipeline uses.
* Without it, the run reports \`skipped\` and still verifies the installed
  application on disk, so a developer machine without the driver gets a useful
  answer instead of a false failure.
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile

from .common import Session, build_mod_archive, capture_failure, run_user_journey

DEFAULT_BINARY = Path('target/release/Stardew Mod Manager.exe')

DRIVER_NAMES = ('msedgedriver.exe', 'msedgedriver')


def find_driver() -> Path | None:
    """The Edge Driver on PATH, or next to the application."""
    for name in DRIVER_NAMES:
        located = shutil.which(name)
        if located:
            return Path(located)
    return None


def _write_windows_game_fixture(game: Path) -> None:
    game.mkdir(parents=True)
    # A native Windows installation: the game launcher, its managed assembly and
    # the runtime dependency manifests the inspector verifies.
    for name in [
        'Stardew Valley.exe',
        'Stardew Valley.dll',
        'Stardew Valley.deps.json',
        'StardewValley.GameData.dll',
    ]:
        (game / name).write_bytes(b'MZ\x90\x00fixture')
    (game / 'Stardew Valley.deps.json').write_text(
        '{"targets":{"net6.0":{"Stardew Valley/1.6.15.24356":{}}}}'
    )


def verify_installed_application(install_root: Path) -> dict:
    """Checks an installed application tree without launching a webview.

    This is the part of the smoke test that always runs: it proves the bundle
    contains a real executable and the embedded frontend, which is what a broken
    or incomplete installer would be missing.
    """
    executable = install_root / 'Stardew Mod Manager.exe'
    if not executable.is_file():
        raise AssertionError(f'the installed application executable is missing: {executable}')

    header = executable.read_bytes()[:2]
    assert header == b'MZ', f'the installed executable is not a PE image: {executable}'

    frontend = install_root / 'dist' / 'index.html'
    if not frontend.is_file():
        raise AssertionError(f'the embedded frontend is missing: {frontend}')

    return {
        'executable': str(executable),
        'executable_bytes': executable.stat().st_size,
        'frontend': str(frontend),
    }


def run(binary: Path, output: Path, require_webview2: bool = False) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    binary = binary if binary.is_absolute() else (Path.cwd() / binary)

    facts = verify_installed_application(binary.parent)

    driver_path = find_driver()
    if driver_path is None:
        if require_webview2:
            raise AssertionError(
                'Microsoft Edge Driver (msedgedriver.exe) is required for the Windows '
                'WebView2 smoke test but was not found on PATH'
            )
        return {
            'status': 'skipped',
            'reason': 'Microsoft Edge Driver is not available on this machine',
            'checks': ['installed application executable present', 'embedded frontend present'],
            **facts,
        }

    with tempfile.TemporaryDirectory(prefix='smm-native-smoke-') as temporary:
        root = Path(temporary)
        game = root / 'Stardew Valley'
        _write_windows_game_fixture(game)
        archive = build_mod_archive(root / 'NativeSmoke.zip')

        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]

        environment = dict(
            os.environ,
            TAURI_WEBVIEW_AUTOMATION='true',
            APPDATA=str(root / 'appdata'),
            LOCALAPPDATA=str(root / 'localappdata'),
            RUST_LOG='info',
        )

        log_path = output / 'native-smoke.log'
        with log_path.open('w') as log:
            driver = subprocess.Popen(
                [str(driver_path), f'--port={port}', '--host=127.0.0.1'],
                env=environment,
                stdout=log,
                stderr=log,
                creationflags=getattr(subprocess, 'CREATE_NEW_PROCESS_GROUP', 0),
            )
            session = Session(port, output)
            try:
                session.wait_until_ready()
                session.new_session(
                    {
                        'browserName': 'webview2',
                        'ms:edgeOptions': {
                            'binary': str(binary),
                            'args': [],
                        },
                    }
                )
                result = run_user_journey(session, game, archive, output)
                return {**result, **facts}
            except Exception:
                capture_failure(session, output)
                raise
            finally:
                session.close()
                driver.terminate()
                try:
                    driver.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    driver.kill()
                    driver.wait()
