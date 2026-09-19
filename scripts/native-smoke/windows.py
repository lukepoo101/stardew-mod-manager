"""Windows packaged-application smoke test.

Windows drives a WebView2 application through Microsoft Edge Driver rather than
WebKitWebDriver, so the driver process, its capabilities and the fixture layout
all differ. The user journey itself is shared with the Linux runner.

The Tauri context macro embeds the frontend into the executable, so the runtime
contract is a single self-contained binary: there is no sibling dist directory
to check for, and asserting one would fail a perfectly good installation.

Two levels are supported:

* `--require-webview2` fails when Edge Driver is unavailable, and is what the
  release pipeline uses.
* Without it, the run reports `skipped` and still verifies the application on
  disk, so a developer machine without the driver gets a useful answer rather
  than a false failure.
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile

from .common import Session, build_mod_archive, capture_failure, run_user_journey

# The Rust package name, which is what cargo and the bundler actually produce.
# The product name is display text and never appears in a file name.
BINARY_NAME = "stardew-mod-manager.exe"
DEFAULT_BINARY = Path("target/release") / BINARY_NAME

DRIVER_NAMES = ("msedgedriver.exe", "msedgedriver")


def find_driver() -> Path | None:
    """The Edge Driver on PATH, or next to the driver environment."""
    for name in DRIVER_NAMES:
        located = shutil.which(name)
        if located:
            return Path(located)
    return None


def find_application(search_root: Path) -> Path:
    """The packaged executable, under either its package or its product name.

    The product name is accepted here only so a bundle that renames the binary
    is still found; being tolerant of the file name is not the same as assuming
    a directory layout.
    """
    preferred = search_root / BINARY_NAME
    if preferred.is_file():
        return preferred
    candidates = sorted(path for path in search_root.glob("*.exe") if path.is_file())
    candidates = [path for path in candidates if not path.name.startswith("uninstall")]
    if not candidates:
        raise AssertionError(f"no packaged executable was found in {search_root}")
    return candidates[0]


def _write_windows_game_fixture(game: Path) -> None:
    game.mkdir(parents=True)
    # A native Windows installation: the game launcher, its managed assembly and
    # the runtime dependency manifests the inspector verifies.
    for name in [
        "Stardew Valley.exe",
        "Stardew Valley.dll",
        "Stardew Valley.deps.json",
        "StardewValley.GameData.dll",
    ]:
        (game / name).write_bytes(b"MZ\x90\x00fixture")
    (game / "Stardew Valley.deps.json").write_text(
        '{"targets":{"net6.0":{"Stardew Valley/1.6.15.24356":{}}}}'
    )


def verify_packaged_application(binary: Path) -> dict:
    """Checks the packaged binary without launching a webview.

    This is the part that always runs: it proves the executable is a real PE
    image, which is what a truncated or failed build would not be. The frontend
    needs no separate check because it is embedded in this file.
    """
    if not binary.is_file():
        raise AssertionError(f"the packaged application executable is missing: {binary}")

    header = binary.read_bytes()[:2]
    if header != b"MZ":
        raise AssertionError(f"the packaged executable is not a PE image: {binary}")

    return {
        "executable": str(binary),
        "executable_bytes": binary.stat().st_size,
    }


def run(binary: Path, output: Path, require_webview2: bool = False) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    binary = binary if binary.is_absolute() else (Path.cwd() / binary)
    binary = find_application(binary.parent) if not binary.is_file() else binary

    facts = verify_packaged_application(binary)

    driver_path = find_driver()
    if driver_path is None:
        if require_webview2:
            raise AssertionError(
                "Microsoft Edge Driver (msedgedriver.exe) is required for the Windows "
                "WebView2 smoke test but was not found on PATH"
            )
        return {
            "status": "skipped",
            "reason": "Microsoft Edge Driver is not available on this machine",
            "checks": ["packaged application executable present and a PE image"],
            **facts,
        }

    with tempfile.TemporaryDirectory(prefix="smm-native-smoke-") as temporary:
        root = Path(temporary)
        game = root / "Stardew Valley"
        _write_windows_game_fixture(game)
        archive = build_mod_archive(root / "NativeSmoke.zip")

        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]

        environment = dict(
            os.environ,
            TAURI_WEBVIEW_AUTOMATION="true",
            APPDATA=str(root / "appdata"),
            LOCALAPPDATA=str(root / "localappdata"),
            RUST_LOG="info",
        )

        log_path = output / "native-smoke.log"
        with log_path.open("w") as log:
            driver = subprocess.Popen(
                [str(driver_path), f"--port={port}", "--host=127.0.0.1"],
                env=environment,
                stdout=log,
                stderr=log,
                creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
            )
            session = Session(port, output)
            try:
                session.wait_until_ready()
                session.new_session(
                    {
                        "browserName": "webview2",
                        "ms:edgeOptions": {
                            "binary": str(binary),
                            "args": [],
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
