"""Explicit identity override checks on the private xfwm4 desktop."""
import json
from pathlib import Path
import subprocess
import time


def check_matching(binary, env):
    assert env.get('LITE_SWITCH_ISOLATED_X11') == '1'
    def invoke(*args, check=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=10)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result
    def wait(callback, message):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if callback():
                return
            time.sleep(.05)
        raise AssertionError(message)
    def windows():
        return json.loads(invoke('windows').stdout)
    def focused(title):
        return any(w['title'] == title and w['focused'] for w in windows())
    def shortcut_snapshot():
        return subprocess.check_output(['xfconf-query', '-c', 'xfce4-keyboard-shortcuts', '-lv'], env=env)

    clone = Path(env['XDG_DATA_HOME']) / 'applications/cycle-clone.desktop'
    clone.write_text('[Desktop Entry]\nType=Application\nName=Ambiguous Cycle Clone\nExec=xterm -T "Should Never Launch"\nStartupWMClass=CycleTest\n')
    original = next(w for w in windows() if w['title'] == 'Live Renamed Target')
    wait(lambda: invoke('assign-app', 'x', 'cycle-clone.desktop', check=False).returncode == 0, 'clone desktop entry discovery')
    ambiguous = invoke('activate', 'b', check=False)
    assert ambiguous.returncode != 0 and 'ambiguous' in ambiguous.stderr, ambiguous
    assert invoke('cycle', 'cycle-test.desktop', check=False).returncode != 0
    assert not any(w['title'] in ['Should Never Launch', 'Unexpected Duplicate'] for w in windows())
    before = shortcut_snapshot()
    invoke('match-app', 'cycle-test.desktop', '--class', 'CycleTest')
    path = Path(env['XDG_CONFIG_HOME']) / 'lite-switch/matching.json'
    assert path.stat().st_mode & 0o777 == 0o600
    assert json.loads(invoke('matching').stdout)['rules']['cycle-test.desktop']['class'] == 'CycleTest'
    invoke('focus', str(original['x11_id']))
    invoke('activate', 'b')
    wait(lambda: focused('Cycle Second'), 'explicit override did not resolve ambiguous app')
    saved = path.read_bytes()
    refused = invoke('match-app', 'cycle-clone.desktop', '--class', 'CYCLETEST', check=False)
    assert refused.returncode != 0 and 'identical' in refused.stderr
    assert path.read_bytes() == saved
    assert 'cycle-clone.desktop' not in json.loads(invoke('matching').stdout)['rules']
    # Different but overlapping criteria must remain ambiguous, not pick a winner.
    invoke('match-app', 'cycle-clone.desktop', '--instance', original['instance'])
    assert invoke('activate', 'b', check=False).returncode != 0
    invoke('unmatch-app', 'cycle-clone.desktop')
    invoke('quit')
    time.sleep(.2)
    invoke('focus', str(original['x11_id']))
    invoke('activate', 'b')  # cold hidden resident reloads persistent identity rules
    wait(lambda: focused('Cycle Second'), 'persistent identity override did not survive restart')
    assert shortcut_snapshot() == before
    # Detected external file edits must not be overwritten by a running resident.
    saved = path.read_bytes()
    path.write_text('invalid JSON')
    try:
        assert invoke('unmatch-app', 'cycle-test.desktop', check=False).returncode != 0
        assert path.read_text() == 'invalid JSON'
    finally:
        path.write_bytes(saved)
    invoke('unmatch-app', 'cycle-test.desktop')
    assert not json.loads(invoke('matching').stdout)['rules']
    assert invoke('activate', 'b', check=False).returncode != 0
    invoke('unassign', 'x')
    clone.unlink()
    invoke('quit')
    print('PASS: isolated ambiguous app overrides, conjunctive/overlap safety, persistence, duplicate rejection, outside-file protection and unchanged desktop shortcuts')
