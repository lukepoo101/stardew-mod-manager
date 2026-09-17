"""Shared WebDriver client and user-journey assertions.

The journey is identical on every platform - register a game, install SMAPI,
install and remove a mod, create a profile - so it lives here once. A platform
module supplies the driver, the launch capabilities and the fixture paths.
"""
from __future__ import annotations

import base64
from pathlib import Path
import json
import time
import urllib.error
import urllib.request


class DriverError(RuntimeError):
    """The WebDriver session could not perform a requested step."""


class Session:
    """A thin W3C WebDriver client over the driver's HTTP endpoint."""

    def __init__(self, port: int, output: Path):
        self.base = f'http://127.0.0.1:{port}'
        self.output = output
        self.session_id: str | None = None

    # -- transport ---------------------------------------------------------
    def request(self, method: str, path: str, body=None, timeout: int = 30):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(
            f'{self.base}{path}',
            data=data,
            method=method,
            headers={'Content-Type': 'application/json'},
        )
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return json.load(response)['value']
        except urllib.error.HTTPError as error:
            raise DriverError(error.read().decode()) from error

    def command(self, method: str, path: str, body=None):
        if self.session_id is None:
            raise DriverError('no active session')
        return self.request(method, f'/session/{self.session_id}{path}', body)

    def wait_until_ready(self, attempts: int = 50) -> None:
        for _ in range(attempts):
            try:
                self.request('GET', '/status')
                return
            except (urllib.error.URLError, ConnectionError, DriverError):
                time.sleep(0.1)
        raise DriverError('the WebDriver endpoint never became ready')

    def new_session(self, capabilities: dict) -> dict:
        response = self.request('POST', '/session', {'capabilities': {'alwaysMatch': capabilities}})
        self.session_id = response['sessionId']
        return response

    def close(self) -> None:
        if self.session_id is None:
            return
        try:
            self.request('DELETE', f'/session/{self.session_id}')
        except Exception:
            pass
        self.session_id = None

    # -- page helpers ------------------------------------------------------
    def script(self, code: str, *values):
        return self.command('POST', '/execute/sync', {'script': code, 'args': list(values)})

    def body_text(self) -> str:
        return self.script('return document.body.innerText')

    def wait_for_text(self, text: str, timeout: float = 20.0) -> str:
        deadline = time.monotonic() + timeout
        body = ''
        while time.monotonic() < deadline:
            body = self.body_text()
            if text in body:
                return body
            time.sleep(0.15)
        raise AssertionError(f'Missing {text!r}; page: {body}')

    def click_text(self, text: str) -> None:
        self.script(
            "const button = [...document.querySelectorAll('button,a')]"
            ".find(el => el.textContent.trim() === arguments[0]);"
            "if (!button) throw new Error('Missing button: ' + arguments[0]);"
            "button.click();",
            text,
        )

    def fill(self, selector: str, value) -> None:
        # The GTK driver does not implement keyboard input on every build, so
        # the native DOM setter and an input event are used instead: React then
        # sees a real edit either way.
        self.script(
            "const el = document.querySelector(arguments[0]);"
            "if (!el) throw new Error('Missing input: ' + arguments[0]);"
            "Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')"
            ".set.call(el, arguments[1]);"
            "el.dispatchEvent(new Event('input', {bubbles: true}));",
            selector,
            str(value),
        )

    def screenshot(self, name: str) -> None:
        (self.output / name).write_bytes(base64.b64decode(self.command('GET', '/screenshot')))

    def url(self) -> str:
        return self.command('GET', '/url')

    def assert_embedded_frontend(self) -> str:
        url = self.url()
        assert ':1420' not in url, f'the packaged application served the dev server: {url}'
        assert url.startswith(('tauri:', 'http://tauri.localhost', 'https://tauri.localhost')), url
        assert self.script('return !!window.__TAURI_INTERNALS__'), 'the Tauri IPC bridge is missing'
        return url


def build_mod_archive(archive: Path) -> Path:
    """A minimal valid SMAPI mod archive, identical on every platform."""
    import zipfile

    with zipfile.ZipFile(archive, 'w') as mod:
        mod.writestr(
            'Smoke/manifest.json',
            json.dumps(
                {
                    'Name': 'Native smoke mod',
                    'Author': 'Tests',
                    'Version': '1.0.0',
                    'UniqueID': 'Tests.NativeSmoke',
                    'EntryDll': 'Smoke.dll',
                }
            ),
        )
        mod.writestr('Smoke/Smoke.dll', 'fixture')
    return archive


def run_user_journey(session: Session, game: Path, archive: Path, output: Path) -> dict:
    """The end-to-end flow that proves the packaged application is functional."""
    session.wait_for_text('Locate Stardew Valley')
    url = session.assert_embedded_frontend()
    session.screenshot('native-onboarding.png')

    session.fill('input[aria-label="Game installation folder"]', game)
    session.click_text('Validate & Continue')
    session.wait_for_text('Install SMAPI')
    session.click_text('Install SMAPI')
    session.wait_for_text('Ready to Mod!')
    session.click_text('Go to Dashboard')
    session.wait_for_text('Ready to Play')

    session.click_text('Mods')
    session.wait_for_text('Installed Mods')
    session.fill('input[placeholder^="Or paste path"]', archive)
    session.click_text('Inspect')
    session.wait_for_text('Review mod installation')
    session.click_text('Install mod')
    session.wait_for_text('1 mod(s) in active profile')
    session.wait_for_text('Native smoke mod')
    session.screenshot('native-installed-mod.png')

    session.script("document.querySelector('button[title=\"Remove mod\"]').click()")
    session.wait_for_text('Review mod removal')
    session.click_text('Remove mod')
    session.wait_for_text('No user mods installed yet')

    session.click_text('Profiles')
    session.click_text('New Profile')
    session.fill('input[placeholder^="Profile name"]', 'Native smoke profile')
    session.click_text('Create')
    session.wait_for_text('Native smoke profile')
    session.screenshot('native-profiles.png')

    return {
        'status': 'passed',
        'url': url,
        'checks': [
            'embedded frontend startup',
            'real IPC available',
            'manual game registration and default profile',
            'onboarding completion',
            'mod ZIP preview and install',
            'mod removal',
            'profile creation and cache refresh',
        ],
    }


def capture_failure(session: Session, output: Path) -> None:
    try:
        session.screenshot('native-failure.png')
        (output / 'native-failure.txt').write_text(session.body_text())
    except Exception:
        pass
