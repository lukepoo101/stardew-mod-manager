"""How the Windows smoke test starts a WebDriver session.

Two providers can drive the packaged application, and they fail for
completely different reasons:

* `embedded` - the application itself hosts a W3C WebDriver server
  (`tauri-plugin-wdio-webdriver`, behind the `webdriver` cargo feature) and
  drives its own WebView2 through the runtime's native API. Nothing about
  another process attaching to the application is involved.
* `external` - `tauri-driver` proxies `msedgedriver`, which launches the
  application and attaches over the DevTools port. A host that refuses to
  let that happen produces the same error regardless of the application.

The distinction is the whole point: the second provider is the one a hosted
CI runner can defeat, so the smoke test reports which one it used and why it
could not be used, instead of reporting an environment failure as if it were
an application failure.
"""
from __future__ import annotations

import os
from pathlib import Path
import json
import socket
import subprocess
import time
import urllib.error
import urllib.request


class ProviderUnavailable(RuntimeError):
    """The provider cannot be used here, and the reason is not the app."""


PROVIDER_EMBEDDED = "embedded"
PROVIDER_EXTERNAL = "external"
PROVIDERS = (PROVIDER_EMBEDDED, PROVIDER_EXTERNAL)

# The environment variable the embedded server reads its port from. It is how
# @wdio/tauri-service configures the provider, so setting it is the whole
# contract rather than a private arrangement with this script.
EMBEDDED_PORT_VARIABLE = "TAURI_WEBDRIVER_PORT"

# The embedded server is a thread inside the application, so it is ready a
# moment after the window is. This is only a bound on how long to wait.
EMBEDDED_READY_TIMEOUT_SECONDS = 30.0

# When the provider is auto-detected, the application is given a short head
# start: a build without the feature never listens at all, and waiting the
# full timeout before falling back would only slow the run down.
EMBEDDED_AUTODETECT_TIMEOUT_SECONDS = 12.0


def classify_embedded_response(payload) -> bool:
    """Whether a GET /status response body is an embedded WebDriver server.

    The embedded provider and the external one both answer /status, and a
    wrong guess is expensive: connecting to the proxy port with the embedded
    protocol produces a session error that looks like an application defect.
    """
    if not isinstance(payload, dict):
        return False
    value = payload.get("value", payload)
    if not isinstance(value, dict) or "ready" not in value:
        return False
    message = str(value.get("message", ""))
    return "webdriver" in message.lower()


def reserve_port() -> int:
    """A free loopback port, which the embedded provider needs to be told."""
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def embedded_server_present(port: int, timeout: float = 1.0) -> bool:
    try:
        with urllib.request.urlopen(
            f"http://127.0.0.1:{port}/status", timeout=timeout
        ) as response:
            return classify_embedded_response(json.load(response))
    except (urllib.error.URLError, ConnectionError, ValueError, OSError):
        return False


class EmbeddedApplication:
    """The packaged application, launched with its WebDriver server enabled.

    The application is given a console, exactly as the Windows console
    adapter gives one to the SMAPI installer: that installer calls
    Console.ReadKey() when it fails, which throws when no console exists, and
    the real error is then replaced by the symptom.
    """

    def __init__(self, binary: Path, environment: dict, port: int):
        self.binary = binary
        self.port = port
        self.environment = dict(environment, **{EMBEDDED_PORT_VARIABLE: str(port)})
        self.process: subprocess.Popen | None = None

    def start(self) -> None:
        self.process = subprocess.Popen(
            [str(self.binary)],
            env=self.environment,
            creationflags=getattr(subprocess, "CREATE_NEW_CONSOLE", 0),
        )

    def wait_until_ready(self, timeout: float = EMBEDDED_READY_TIMEOUT_SECONDS) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.process is not None and self.process.poll() is not None:
                raise AssertionError(
                    f"the packaged application exited with code {self.process.returncode} "
                    "before its WebDriver server became ready"
                )
            if embedded_server_present(self.port):
                return
            time.sleep(0.25)
        raise ProviderUnavailable(
            f"the packaged application did not start an embedded WebDriver server on "
            f"port {self.port} within {timeout:g} seconds, so it was built without the "
            "webdriver feature"
        )

    def stop(self) -> None:
        if self.process is None or self.process.poll() is not None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()


def classify_failure(error: BaseException) -> tuple:
    """Whether a failed driven session says something about the application.

    Three conditions are statements about the host rather than the product:
    the application never created a debugging port for an external driver to
    attach to, the application has no embedded WebDriver server at all, and an
    external driver launched it without the console the upstream SMAPI
    installer requires. Everything else - including a journey that starts and
    then fails - is about the product.
    """
    if isinstance(error, ProviderUnavailable):
        return ("blocked", str(error))
    message = str(error)
    if SMAPI_WITHOUT_CONSOLE_MARKER in message:
        return (
            "blocked",
            "the application was launched by an external driver, which gives it no "
            "console; the upstream SMAPI installer requires one, so the journey could "
            "not be completed under this provider",
        )
    if "DevToolsActivePort" in message:
        return (
            "blocked",
            "the WebView2 runtime did not initialise in the driver-launched application, "
            "so no WebDriver session could be created; this host cannot drive a WebView2 "
            "window through an external driver",
        )
    return ("failed", message)
