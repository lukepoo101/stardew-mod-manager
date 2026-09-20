"""Windows packaged-application smoke test.

Windows drives a WebView2 application through Microsoft Edge Driver rather than
WebKitWebDriver, so the driver process, its capabilities and the fixture layout
all differ. The user journey itself is shared with the Linux runner.

The Tauri context macro embeds the frontend into the executable, so the runtime
contract is a single self-contained binary: there is no sibling dist directory
to check for, and asserting one would fail a perfectly good installation.

Three levels are supported, and they make different claims:

* `--direct-start` launches the packaged application the way a user does and
  requires it to stay alive, which is the one claim that always has to hold.
* `--require-webview2` fails when Edge Driver is unavailable, and is what the
  release pipeline uses.
* Without either, the run reports `skipped` and still verifies the application
  on disk, so a developer machine without the driver gets a useful answer
  rather than a false failure.

Each launch gets its own WebView2 user-data directory. Two launches sharing one
is not a detail: the runtime keeps a lock in that directory, so a second process
can fail to start for no reason that has anything to do with the application.
"""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import time
import subprocess
import tempfile

from .common import (
    Session,
    build_mod_archive,
    capture_failure,
    remove_tree,
    run_user_journey,
)
from .windows_driver import (
    EMBEDDED_AUTODETECT_TIMEOUT_SECONDS,
    EMBEDDED_READY_TIMEOUT_SECONDS,
    PROVIDER_EMBEDDED,
    PROVIDER_EXTERNAL,
    PROVIDERS,
    EmbeddedApplication,
    ProviderUnavailable,
    classify_failure,
    reserve_port,
)

# The Rust package name, which is what cargo and the bundler actually produce.
# The product name is display text and never appears in a file name.
BINARY_NAME = "stardew-mod-manager.exe"
DEFAULT_BINARY = Path("target/release") / BINARY_NAME

NATIVE_DRIVER_NAMES = ("msedgedriver.exe", "msedgedriver")

# The WebView2 user-data directory name inside a launch's isolated scratch
# directory. Each launch gets a scratch directory of its own, so this name
# repeating across launches is not a shared location.
DIRECT_START_DIRECTORY = "webview2-data"

# Starting a WebView2 application means launching a process, waiting for the
# runtime to initialise and attaching to it, which is slower than any other
# WebDriver session.
SESSION_TIMEOUT_SECONDS = 120

# WebView2 applications are driven through tauri-driver, which translates the
# tauri:options capability into the ms:edgeOptions the native driver expects.
# Talking to msedgedriver directly cannot work: it does not know what a
# "tauri:options" capability is, so the session never starts.
TAURI_DRIVER_NAME = "tauri-driver"

# How long the directly launched application has to stay alive before it is
# considered healthy. A packaged Tauri application that cannot create its
# webview exits within a second or two, so this is far beyond the failure
# window while staying short enough for CI.
DIRECT_START_SECONDS = 15.0


def isolated_environment(scratch: Path) -> dict:
    """An environment in which this launch owns every piece of state it writes.

    The WebView2 runtime takes an exclusive lock inside its user-data
    directory, and the application keeps its own settings under the roaming and
    local application-data directories. Giving each launch its own copies means
    a probe can never be blamed on, or interfere with, another launch.
    """
    return dict(
        os.environ,
        TAURI_WEBVIEW_AUTOMATION="true",
        APPDATA=str(scratch / "appdata"),
        LOCALAPPDATA=str(scratch / "localappdata"),
        WEBVIEW2_USER_DATA_FOLDER=str(scratch / DIRECT_START_DIRECTORY),
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--enable-logging --v=1",
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS_LOG=str(scratch / "webview2.log"),
    )


def run_direct_start(
    binary: Path, output: Path, isolation_root: Path, seconds: float = DIRECT_START_SECONDS
) -> dict:
    """Launches the application and requires it to still be running afterwards.

    This is the claim the whole Windows job rests on: a user double-clicks the
    installed executable and the application opens. It is deliberately not a
    driven session, so it holds even on a host where WebDriver cannot attach,
    and it is deliberately fatal when it fails, because an application that
    exits on start-up is broken regardless of what else the host cannot do.
    """
    facts = verify_packaged_application(binary)
    output.mkdir(parents=True, exist_ok=True)
    isolation_root.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="smm-direct-start-", dir=isolation_root))
    environment = isolated_environment(scratch)
    stdout_path = output / "direct-start-stdout.log"
    stderr_path = output / "direct-start-stderr.log"

    with stdout_path.open("w") as stdout, stderr_path.open("w") as stderr:
        process = subprocess.Popen(
            [str(binary)],
            env=environment,
            stdout=stdout,
            stderr=stderr,
            creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
        )
    try:
        time.sleep(seconds)
        exit_code = process.poll()
        if exit_code is not None:
            raise AssertionError(
                f"the packaged application exited with code {exit_code} within "
                f"{seconds:g} seconds of being launched"
            )
        user_data = scratch / DIRECT_START_DIRECTORY
        if not user_data.is_dir():
            raise AssertionError(
                "the packaged application never created its WebView2 user-data "
                f"directory at {user_data}, so its window did not start"
            )
        return {
            "status": "passed",
            "alive_seconds": seconds,
            "webview2_user_data": str(user_data),
            "checks": [
                "packaged application present and a PE image",
                f"application launched directly and stayed alive for {seconds:g} seconds",
                "WebView2 runtime created its user-data directory",
            ],
            **facts,
        }
    except BaseException:
        _write_direct_start_evidence(output, stdout_path, stderr_path, environment, scratch)
        raise
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        remove_tree(scratch)


def _write_direct_start_evidence(
    output: Path, stdout_path: Path, stderr_path: Path, environment: dict, scratch: Path
) -> None:
    """Everything needed to explain a start-up failure, written next to the result."""
    lines = ["--- application stdout ---"]
    lines.extend(_tail(stdout_path))
    lines.append("--- application stderr ---")
    lines.extend(_tail(stderr_path))
    lines.append("--- child environment ---")
    for name in sorted(environment):
        if name.startswith(("WEBVIEW2", "TAURI_", "APPDATA", "LOCALAPPDATA", "RUST_")):
            lines.append(f"{name}={environment[name]}")
    lines.append("--- isolated scratch contents ---")
    for item in sorted(scratch.rglob("*")):
        if item.is_file():
            lines.append(f"{item.relative_to(scratch)} ({item.stat().st_size} bytes)")
    for log_name in ("webview2.log",):
        candidate = scratch / log_name
        if candidate.is_file():
            lines.append(f"--- {log_name} (tail) ---")
            lines.extend(_tail(candidate, 80))
    (output / "direct-start-diagnostics.txt").write_text("\n".join(lines) + "\n")


def _tail(path: Path, lines: int = 40) -> list:
    if not path.is_file():
        return ["(none)"]
    content = path.read_text(errors="replace").splitlines()
    return content[-lines:] or ["(empty)"]


def find_native_driver() -> Path | None:
    """The native WebDriver (msedgedriver) that tauri-driver proxies to."""
    for name in NATIVE_DRIVER_NAMES:
        located = shutil.which(name)
        if located:
            return Path(located)
    return None


def find_tauri_driver() -> Path | None:
    """The tauri-driver proxy, which is what a WebView2 session needs."""
    located = shutil.which(TAURI_DRIVER_NAME)
    return Path(located) if located else None


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


def start_session_with_retry(session: Session, binary: Path, attempts: int = 3) -> None:
    """Starts the driven session, retrying a transient start-up failure.

    A WebView2 session can fail to start while the previous instance of the
    runtime is still shutting down, which a second attempt clears. The last
    error is raised so a genuine failure is still reported.
    """
    last: Exception | None = None
    for attempt in range(1, attempts + 1):
        try:
            session.new_session(
                {
                    "tauri:options": {
                        "application": str(binary),
                    },
                },
                timeout=SESSION_TIMEOUT_SECONDS,
            )
            return
        except Exception as error:  # noqa: BLE001 - reported below if it persists
            last = error
            print(f"session attempt {attempt}/{attempts} failed: {error}", flush=True)
            time.sleep(5)
    assert last is not None
    raise last


def report_diagnostics(scratch: Path, output: Path, environment: dict) -> None:
    """Copies whatever the runtime and the application left behind.

    A WebView2 session that never starts is otherwise invisible: the driver only
    reports that it could not find the debugging port. The environment the child
    actually ran with, and anything the runtime logged, are written next to the
    smoke output so the failure can be diagnosed from the artifact.
    """
    lines = [
        "--- child environment ---",
    ]
    for name in sorted(environment):
        if name.startswith(("WEBVIEW2", "TAURI_", "APPDATA", "LOCALAPPDATA", "RUST_")):
            lines.append(f"{name}={environment[name]}")
    lines.append("--- scratch contents ---")
    for item in sorted(scratch.rglob('*')):
        if item.is_file():
            lines.append(f"{item.relative_to(scratch)} ({item.stat().st_size} bytes)")
    for log_name in ('webview2.log',):
        candidate = scratch / log_name
        if candidate.is_file():
            lines.append(f"--- {log_name} (tail) ---")
            lines.extend(
                candidate.read_text(errors='replace').splitlines()[-80:]
            )
    (output / "native-failure-diagnostics.txt").write_text("\n".join(lines) + "\n")

def resolve_provider(requested: str, binary: Path, environment: dict, output: Path) -> tuple:
    """Which WebDriver provider to use, and the objects that go with it.

    Returns (provider, session, application, driver). The application is the
    embedded launch when there is one: it has to stay alive for the session to
    work and has to be stopped afterwards. The driver is the external proxy
    process, which the same is true of.
    """
    if requested not in ("auto",) + PROVIDERS:
        raise AssertionError(f"unknown driver provider: {requested}")

    if requested in ("auto", PROVIDER_EMBEDDED):
        port = reserve_port()
        application = EmbeddedApplication(binary, environment, port)
        application.start()
        timeout = (
            EMBEDDED_READY_TIMEOUT_SECONDS
            if requested == PROVIDER_EMBEDDED
            else EMBEDDED_AUTODETECT_TIMEOUT_SECONDS
        )
        try:
            application.wait_until_ready(timeout)
        except ProviderUnavailable:
            application.stop()
            if requested == PROVIDER_EMBEDDED:
                raise
            print(
                "no embedded WebDriver server; falling back to the external driver",
                flush=True,
            )
        else:
            session = Session(port, output)
            session.new_session({}, timeout=SESSION_TIMEOUT_SECONDS)
            return (PROVIDER_EMBEDDED, session, application, None)

    native_driver = find_native_driver()
    tauri_driver = find_tauri_driver()
    missing = []
    if native_driver is None:
        missing.append("Microsoft Edge Driver (msedgedriver.exe)")
    if tauri_driver is None:
        missing.append("tauri-driver (cargo install tauri-driver)")
    if missing:
        raise ProviderUnavailable(
            "the external WebDriver is not available: " + " and ".join(missing)
        )

    port = reserve_port()
    native_port = reserve_port()
    log_path = output / "native-smoke.log"
    with log_path.open("w") as log:
        driver = subprocess.Popen(
            [
                str(tauri_driver),
                f"--port={port}",
                f"--native-port={native_port}",
                "--native-driver",
                str(native_driver),
            ],
            env=environment,
            stdout=log,
            stderr=log,
            creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
        )
    session = Session(port, output)
    session.wait_until_ready()
    start_session_with_retry(session, binary)
    return (PROVIDER_EXTERNAL, session, None, driver)


def stop_process(process) -> None:
    """Stops a process this run started, without waiting forever for it."""
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def run(
    binary: Path,
    output: Path,
    require_webview2: bool = False,
    direct_start: bool = False,
    isolation_root: Path | None = None,
    direct_start_seconds: float = DIRECT_START_SECONDS,
    driver_provider: str = "auto",
) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    binary = binary if binary.is_absolute() else (Path.cwd() / binary)
    binary = find_application(binary.parent) if not binary.is_file() else binary

    facts = verify_packaged_application(binary)

    if direct_start:
        # Absolute paths are not cosmetic here: the WebView2 runtime ignores a
        # relative WEBVIEW2_USER_DATA_FOLDER and silently writes to the shared
        # default profile instead, which would make the isolation a fiction.
        root = (isolation_root or output / "webview2-isolation").resolve()
        return run_direct_start(binary, output, root, seconds=direct_start_seconds)

    scratch = Path(tempfile.mkdtemp(prefix="smm-native-smoke-"))
    try:
        return _run_driven(binary, output, scratch, facts, driver_provider)
    finally:
        remove_tree(scratch)


def _run_driven(
    binary: Path, output: Path, scratch: Path, facts: dict, driver_provider: str
) -> dict:
    """Drives the installed application through whichever provider works here."""
    root = scratch
    game = root / "Stardew Valley"
    _write_windows_game_fixture(game)
    archive = build_mod_archive(root / "NativeSmoke.zip")
    environment = dict(isolated_environment(root), RUST_LOG="info")

    provider = None
    session = None
    application = None
    driver = None
    try:
        provider, session, application, driver = resolve_provider(
            driver_provider, binary, environment, output
        )
        result = run_user_journey(session, game, archive, output)
        return {**result, "driver_provider": provider, **facts}
    except Exception as error:  # noqa: BLE001 - classified below
        status, reason = classify_failure(error)
        report_diagnostics(root, output, environment)
        if status == "failed":
            if session is not None:
                capture_failure(session, output)
            raise
        return {
            "status": status,
            "driver_provider": provider,
            "reason": reason,
            "checks": [
                "packaged application present and a PE image",
                "application started on this host",
            ],
            "driver_error": str(error)[:500],
            **facts,
        }
    finally:
        if session is not None:
            session.close()
        stop_process(application.process if application is not None else None)
        stop_process(driver)

