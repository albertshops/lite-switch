"""Mutating shortcut tests run only on desktop_smoke.py's private desktop."""
import ctypes
import json
from pathlib import Path
import subprocess
import time


def check_bindings(binary, env, root, executable):
    channel = 'xfce4-keyboard-shortcuts'
    def invoke(*args, check=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result
    def query(*args, check=True):
        result = subprocess.run(['xfconf-query', '-c', channel, *args], env=env, capture_output=True, text=True, timeout=5)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result
    def value(property):
        return query('-p', property).stdout.strip()
    def set_value(property, text, kind='string'):
        existing = query('-p', property, check=False).returncode == 0
        if existing and kind == 'string':
            query('-p', property, '-r')
            existing = query('-p', property, check=False).returncode == 0
        query('-p', property, *([] if existing else ['-n', '-t', kind]), '-s', text)
    def snapshot():
        rows = {}
        for line in query('-lv').stdout.splitlines():
            if line.startswith('/'):
                parts = line.split(None, 1)
                rows[parts[0]] = parts[1] if len(parts) == 2 else ''
        return rows
    def wait(callback, message):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if callback():
                return
            time.sleep(.05)
        raise AssertionError(message)
    def windows():
        return json.loads(invoke('windows').stdout)
    def panel():
        return next((w for w in windows() if w['title'] == 'Lite Switch'), None)
    def focused(title):
        return any(w['title'] == title and w['focused'] for w in windows())
    def key(chord):
        subprocess.run([executable('xdotool'), 'key', '--clearmodifiers', chord], env=env, check=True)

    assert env.get('LITE_SWITCH_ISOLATED_X11') == '1'
    invoke('quit')
    alias = root / "launcher ' ; alias"
    alias.symlink_to(binary)
    env['LITE_SWITCH_LAUNCHER'] = str(alias)
    old_b, old_e, old_alias = root / 'old-b', root / 'old-e', root / 'old-alias'
    set_value('/commands/custom/<Super>b', f'touch {old_b}')
    set_value('/commands/custom/<Super>b/startup-notify', 'true', 'bool')
    set_value('/commands/custom/<Super>e', f'touch {old_e}')
    baseline = snapshot()
    original = next(w for w in windows() if w['title'] == 'Live Renamed Target')
    invoke('assign-window', 'e', str(original['x11_id']))
    invoke('set-modifier', 'Super')
    invoke('set-launcher', 'Alt+space')
    manifest = Path(env['XDG_CONFIG_HOME']) / 'lite-switch/xfce-binding-installation.json'
    backups = manifest.parent / 'backups'
    rejected = invoke('bindings-install', check=False)
    assert rejected.returncode != 0 and '--approve-xfce-shortcuts' in rejected.stderr
    assert not manifest.exists() and snapshot() == baseline
    preview = invoke('bindings-check').stdout
    assert '3 existing binding(s) displaced' in preview, preview
    assert snapshot() == baseline and not manifest.exists()
    installed = invoke('bindings-install', '--approve-xfce-shortcuts').stdout
    assert '3 existing binding(s) displaced' in installed, installed
    assert manifest.stat().st_mode & 0o777 == 0o600
    assert backups.stat().st_mode & 0o777 == 0o700
    assert value('/commands/custom/<Super>b/startup-notify') == 'false'
    assert value('/xfwm4/custom/<Alt>F4') == baseline['/xfwm4/custom/<Alt>F4']
    status = json.loads(invoke('bindings-status').stdout)
    assert status['installed'] and status['intact'] and not status['pending_changes'], status
    count = len(list(backups.glob('xfce-bindings-*.json')))
    assert 'already up to date' in invoke('bindings-sync', '--approve-xfce-shortcuts').stdout
    assert len(list(backups.glob('xfce-bindings-*.json'))) == count
    invoke('focus', str(original['x11_id']))
    key('alt+space')
    wait(lambda: panel() and panel()['focused'], 'installed launcher grab')
    key('alt+space')
    time.sleep(.3)  # do not mistake a transient focus-loss dismissal for toggle
    assert panel() is None, 'second real shortcut reopened instead of toggling closed'
    assert focused('Live Renamed Target'), 'toggle did not restore previous focus'
    key('alt+space')
    wait(lambda: panel() and panel()['focused'], 'shortcut opens again after closing')
    key('Escape')
    wait(lambda: panel() is None, 'launcher dismissal')
    invoke('set-launcher', 'Super+space')
    invoke('bindings-sync', '--approve-xfce-shortcuts')
    key('super+space')
    wait(lambda: panel() and panel()['focused'], 'Super+Space opens')
    key('super+space')
    time.sleep(.3)
    assert panel() is None and focused('Live Renamed Target'), 'Super+Space must close and restore focus'
    key('super+space')
    wait(lambda: panel() and panel()['focused'], 'Super+Space opens again')
    key('Escape')
    wait(lambda: panel() is None, 'Super+Space launcher Escape dismissal')
    key('super+b')
    wait(lambda: focused('Cycle Second'), 'installed application grab')
    key('super+e')
    wait(lambda: focused('Live Renamed Target'), 'installed window grab')
    assert not old_b.exists() and not old_e.exists()

    # A new alias-form collision is displaced on sync; the old WM key comes back.
    alias_property = '/commands/custom/<Control><Alt>space'
    set_value(alias_property, f'touch {old_alias}')
    invoke('set-launcher', 'Control+Alt+space')
    assert json.loads(invoke('bindings-status').stdout)['pending_changes']
    before_preview = snapshot()
    invoke('bindings-check')
    assert snapshot() == before_preview
    invoke('bindings-sync', '--approve-xfce-shortcuts')
    assert value('/xfwm4/custom/<Alt>space') == baseline['/xfwm4/custom/<Alt>space']
    key('ctrl+alt+space')
    wait(lambda: panel() and panel()['focused'], 'synced launcher replaces alias collision')
    key('Escape')
    wait(lambda: panel() is None, 'synced dismissal')
    assert not old_alias.exists()
    invoke('set-modifier', 'Alt')
    invoke('bindings-sync', '--approve-xfce-shortcuts')
    assert value('/commands/custom/<Super>b/startup-notify') == 'true'
    key('super+b')
    wait(old_b.exists, 'old application key restored after moving modifier')
    key('alt+e')
    wait(lambda: focused('Live Renamed Target'), 'moved direct key')

    # Hand-edited ownership must fail without overwriting that edit or other keys.
    owned = '/commands/custom/<Primary><Alt>space'
    own_command = value(owned)
    set_value(owned, 'touch /tmp/never-executed-user-edit')
    before = (snapshot(), manifest.read_bytes())
    refused = invoke('bindings-uninstall', '--approve-xfce-shortcuts', check=False)
    assert refused.returncode != 0 and 'edited' in refused.stderr, refused
    assert (snapshot(), manifest.read_bytes()) == before
    set_value(owned, own_command)
    invoke('bindings-uninstall', '--approve-xfce-shortcuts')
    assert not manifest.exists()
    restored = snapshot()
    assert restored.pop(alias_property) == f'touch {old_alias}'
    assert restored == baseline, (restored, baseline)
    key('ctrl+alt+space')
    wait(old_alias.exists, 'alias command restored on uninstall')
    assert not json.loads(invoke('bindings-status').stdout)['installed']
    assert list(backups.glob('xfce-bindings-*.json'))
    for path in backups.glob('xfce-bindings-*.json'):
        assert path.stat().st_mode & 0o777 == 0o600
        backup = json.loads(path.read_text())
        assert backup['channel'] == channel and 'before' in backup and 'after' in backup
    # A grab owned outside Xfce must cause a safe failure, not a falsely
    # successful install. Release of another conflicting key must roll back.
    set_value('/commands/custom/<Alt>b', f'touch {root / "old-alt-b"}')
    invoke('set-launcher', 'Control+Alt+Return')
    before_failure = snapshot()
    x11 = ctypes.CDLL('libX11.so.6')
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XGrabKey.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_int]
    x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(env['DISPLAY'].encode())
    assert display
    try:
        code = x11.XKeysymToKeycode(display, 0xff0d)  # Return
        x11.XGrabKey(display, code, 12, x11.XDefaultRootWindow(display), 0, 1, 1)
        x11.XSync(display, 0)
        failed = invoke('bindings-install', '--approve-xfce-shortcuts', check=False)
        assert failed.returncode != 0 and 'still grabbed' in failed.stderr, failed
        assert 'prior shortcut properties restored' in failed.stderr, failed
        assert snapshot() == before_failure and not manifest.exists()
    finally:
        x11.XCloseDisplay(display)
    invoke('quit')
    print('PASS: isolated approved Xfce shortcut priority, real grabs/quoted launcher, aliases, metadata, sync/restoration, idempotency, ownership protection, external-grab rollback and private recovery snapshots')
