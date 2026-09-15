#!/usr/bin/env python3
"""Exercise the packaged Linux webview with real IPC and temporary game/mod fixtures.
Requires WebKitWebDriver and a graphical session (or xvfb-run).
"""
import argparse
import base64
import json
import os
from pathlib import Path
import socket
import signal
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, default=Path('target/release/stardew-mod-manager'))
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='smm-native-smoke-') as temporary:
    root = Path(temporary)
    game = root / 'Stardew Valley'
    game.mkdir()
    for name in ['StardewValley']:
        (game / name).write_text('#!/bin/sh\nexit 0\n')
        (game / name).chmod(0o755)
    for name in ['Stardew Valley.dll', 'Stardew Valley.deps.json']:
        (game / name).write_text('{}')
    archive = root / 'NativeSmoke.zip'
    with zipfile.ZipFile(archive, 'w') as mod:
        mod.writestr('Smoke/manifest.json', json.dumps({'Name': 'Native smoke mod', 'Author': 'Tests', 'Version': '1.0.0', 'UniqueID': 'Tests.NativeSmoke', 'EntryDll': 'Smoke.dll'}))
        mod.writestr('Smoke/Smoke.dll', 'fixture')
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    environment = dict(os.environ, TAURI_WEBVIEW_AUTOMATION='true', XDG_DATA_HOME=str(root / 'data'), XDG_CACHE_HOME=str(root / 'cache'), RUST_LOG='info', GDK_BACKEND='x11')
    log = (args.output / 'native-smoke.log').open('w')
    driver = subprocess.Popen(['WebKitWebDriver', f'--port={port}', '--host=127.0.0.1'], env=environment, stdout=log, stderr=log, start_new_session=True)
    session = None
    def request(method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(f'http://127.0.0.1:{port}{path}', data=data, method=method, headers={'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(req, timeout=30) as response:
                return json.load(response)['value']
        except urllib.error.HTTPError as error:
            raise RuntimeError(error.read().decode()) from error
    def command(method, path, body=None):
        return request(method, f'/session/{session}{path}', body)
    def script(code, *values):
        return command('POST', '/execute/sync', {'script': code, 'args': list(values)})
    def wait_text(text):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            body = script('return document.body.innerText')
            if text in body:
                return body
            time.sleep(0.15)
        raise AssertionError(f'Missing {text!r}; page: {body}')
    def click_text(text):
        script("const button = [...document.querySelectorAll('button,a')].find(el => el.textContent.trim() === arguments[0]); if (!button) throw new Error('Missing button: ' + arguments[0]); button.click();", text)
    def fill(selector, value):
        # WebKitGTK's driver does not implement keyboard input on every build.
        # Use the native DOM setter and input event so React handles a real edit.
        script("const el = document.querySelector(arguments[0]); if (!el) throw new Error('Missing input'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, arguments[1]); el.dispatchEvent(new Event('input', {bubbles: true}));", selector, str(value))
    def screenshot(name):
        (args.output / name).write_bytes(base64.b64decode(command('GET', '/screenshot')))
    try:
        for _ in range(50):
            try:
                request('GET', '/status')
                break
            except (urllib.error.URLError, ConnectionError):
                time.sleep(0.1)
        response = request('POST', '/session', {'capabilities': {'alwaysMatch': {'webkitgtk:browserOptions': {'binary': str(args.binary.resolve()), 'args': []}}}})
        session = response['sessionId']
        wait_text('Locate Stardew Valley')
        url = command('GET', '/url')
        assert ':1420' not in url and url.startswith(('tauri:', 'http://tauri.localhost')), url
        assert script('return !!window.__TAURI_INTERNALS__')
        screenshot('native-onboarding.png')
        fill('input[aria-label="Game installation folder"]', game)
        click_text('Validate & Continue')
        wait_text('Install SMAPI')
        click_text('Install SMAPI')
        wait_text('Ready to Mod!')
        click_text('Go to Dashboard')
        wait_text('Ready to Play')
        click_text('Mods')
        wait_text('Installed Mods')
        fill('input[placeholder^="Or paste path"]', archive)
        click_text('Inspect')
        wait_text('Review mod installation')
        click_text('Install mod')
        wait_text('1 mod(s) in active profile')
        wait_text('Native smoke mod')
        screenshot('native-installed-mod.png')
        script('document.querySelector(\'button[title="Remove mod"]\').click()')
        wait_text('Review mod removal')
        click_text('Remove mod')
        wait_text('No user mods installed yet')
        click_text('Profiles')
        click_text('New Profile')
        fill('input[placeholder^="Profile name"]', 'Native smoke profile')
        click_text('Create')
        wait_text('Native smoke profile')
        screenshot('native-profiles.png')
        result = {'status': 'passed', 'url': url, 'checks': ['embedded frontend startup', 'real IPC available', 'manual game registration and default profile', 'onboarding completion', 'mod ZIP preview and install', 'mod removal', 'profile creation and cache refresh']}
        (args.output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        for name in ['native-failure.png', 'native-failure.txt']:
            (args.output / name).unlink(missing_ok=True)
        print(json.dumps(result, indent=2), flush=True)
    except Exception:
        if session:
            try:
                screenshot('native-failure.png')
                (args.output / 'native-failure.txt').write_text(script('return document.body.innerText'))
            except Exception:
                pass
        raise
    finally:
        if session:
            try:
                request('DELETE', f'/session/{session}')
            except Exception:
                pass
        try:
            os.killpg(driver.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            driver.wait(timeout=5)
        except subprocess.TimeoutExpired:
            driver.kill()
            driver.wait()
        time.sleep(0.2)
        log.close()
