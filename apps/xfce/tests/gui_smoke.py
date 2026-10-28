"""GTK checks called only inside desktop_smoke.py's private display/session bus."""
import json
from pathlib import Path
import subprocess
import time


def check_gui(binary, env, root, processes, log, executable):
    def wait(callback, message):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            value = callback()
            if value:
                return value
            time.sleep(.05)
        raise AssertionError(message)

    def invoke(*args, check=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=10)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result

    def windows():
        return json.loads(invoke('windows').stdout)

    def window(title):
        return next((w for w in windows() if w['title'] == title), None)

    def panel():
        return window('Lite Switch')

    def key(value):
        subprocess.run([executable('xdotool'), 'key', '--clearmodifiers', value], env=env, check=True)

    def query(text):
        key('ctrl+a')
        subprocess.run([executable('xdotool'), 'type', '--clearmodifiers', '--delay', '1', text], env=env, check=True)
        time.sleep(.15)

    applications = Path(env['XDG_DATA_HOME']) / 'applications'
    applications.mkdir(parents=True, exist_ok=True)
    (applications / 'cycle-test.desktop').write_text('[Desktop Entry]\nType=Application\nName=Cycle Test\nExec=xterm -class CycleTest -T "Unexpected Duplicate"\nStartupWMClass=CycleTest\n')
    processes.append(subprocess.Popen(['xterm', '-class', 'CycleTest', '-T', 'Smoke Target'], env=env, stdout=log, stderr=log))
    original = wait(lambda: window('Smoke Target'), 'original xterm')
    invoke('focus', str(original['x11_id']))
    original_rect = original['rect']
    processes.append(subprocess.Popen([str(binary), 'show'], env=env, stdout=log, stderr=log))
    wait(lambda: panel() and panel()['focused'], 'GTK launcher did not focus')
    wait(lambda: panel()['rect']['width'] == 680 and panel()['rect']['height'] == 560, 'launcher size')
    wait(lambda: abs(panel()['rect']['x'] - 300) <= 5 and abs(panel()['rect']['y'] - 170) <= 5, 'launcher not centered')
    assert window('Smoke Target')['rect'] == original_rect
    invoke('show')
    assert len([w for w in windows() if w['title'] == 'Lite Switch']) == 1
    key('Escape')
    wait(lambda: panel() is None and window('Smoke Target')['focused'], 'Escape/previous focus')
    invoke('toggle')
    wait(lambda: panel() and panel()['focused'], 'toggle show')
    invoke('toggle')
    wait(lambda: panel() is None and window('Smoke Target')['focused'], 'toggle dismiss')
    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'search panel')
    query('Smoke Target')
    key('Return')
    wait(lambda: panel() is None and window('Smoke Target')['focused'], 'window activation')

    # An initially absent result must appear from a live title event, not from
    # reopening the launcher or periodic tree polling.
    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'live-title panel')
    query('Live Renamed Target')
    subprocess.run([executable('xdotool'), 'set_window', '--name', 'Live Renamed Target', str(original['x11_id'])], env=env, check=True)
    wait(lambda: window('Live Renamed Target'), 'window title update')
    time.sleep(.15)
    key('Return')
    wait(lambda: panel() is None and window('Live Renamed Target')['focused'], 'live result did not update in GTK')

    processes.append(subprocess.Popen(['xterm', '-class', 'CycleTest', '-T', 'Cycle Second'], env=env, stdout=log, stderr=log))
    second = wait(lambda: window('Cycle Second'), 'second cycling window')
    invoke('focus', str(original['x11_id']))
    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'outside-focus panel')
    invoke('focus', str(second['x11_id']))
    wait(lambda: panel() is None and window('Cycle Second')['focused'], 'outside focus must not restore previous window')
    invoke('focus', str(original['x11_id']))
    invoke('cycle', 'cycle-test.desktop')
    wait(lambda: window('Cycle Second')['focused'], 'app cycle')
    # An app-named window must not shadow the application/cycling row.
    subprocess.run([executable('xdotool'), 'set_window', '--name', 'Cycle Test', str(original['x11_id'])], env=env, check=True)
    wait(lambda: window('Cycle Test'), 'app-named window fixture')
    invoke('focus', str(original['x11_id']))
    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'deduplicated application search')
    query('Cycle Test')
    key('Return')
    wait(lambda: panel() is None and window('Cycle Second')['focused'], 'app query selected a duplicate window instead of cycling')
    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'explicit window search')
    query('windows: CycleTest')
    key('Return')
    wait(lambda: panel() is None and window('Cycle Test')['focused'], 'explicit window mode did not select the individual window')
    subprocess.run([executable('xdotool'), 'set_window', '--name', 'Live Renamed Target', str(original['x11_id'])], env=env, check=True)
    wait(lambda: window('Live Renamed Target'), 'restore title fixture')
    invoke('assign-window', 'a', str(original['x11_id']))
    invoke('assign-app', 'b', 'cycle-test.desktop')
    invoke('activate', 'a')
    wait(lambda: window('Live Renamed Target')['focused'], 'assigned-window activation')
    invoke('activate', 'b')
    wait(lambda: window('Cycle Second')['focused'], 'cycle excludes assigned window')
    invoke('assign-window', 'c', str(second['x11_id']))
    blocked = invoke('activate', 'b', check=False)
    assert blocked.returncode != 0 and 'individually assigned' in blocked.stderr
    assert window('Unexpected Duplicate') is None

    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'key capture panel')
    query('Live Renamed Target')
    panel_id = str(panel()['x11_id'])
    subprocess.run([executable('xdotool'), 'mousemove', '--window', panel_id, '620', '125', 'click', '1'], env=env, check=True)
    key('d')
    saved = json.loads(invoke('assignments').stdout)
    assert 'd' in saved and 'a' not in saved, saved
    subprocess.run([executable('xdotool'), 'mousemove', '--window', panel_id, '620', '125', 'click', '1'], env=env, check=True)
    key('BackSpace')
    assert 'd' not in json.loads(invoke('assignments').stdout)
    key('Escape')
    wait(lambda: panel() is None, 'key capture dismissal')
    invoke('set-modifier', 'Super')
    assert any(p['shortcut'] == '<Super>b' for p in json.loads(invoke('bindings').stdout))
    assert invoke('set-launcher', 'Super+b', check=False).returncode != 0

    # GIO directory monitoring, safe desktop-entry launching and private history.
    invoke('show')
    wait(lambda: panel() and panel()['focused'], 'app-discovery panel')
    query('Late Added Probe')
    (applications / 'late-probe.desktop').write_text('[Desktop Entry]\nType=Application\nName=Late Added Probe\nExec=xterm -class LaunchProbe -T "Launched Probe"\n')
    time.sleep(1)
    key('Return')
    wait(lambda: window('Launched Probe'), 'live desktop-entry discovery/launch')
    wait(lambda: panel() is None, 'launch did not dismiss panel')
    history = Path(env['XDG_STATE_HOME']) / 'lite-switch/usage.json'
    assert json.loads(history.read_text())['apps']['late-probe.desktop']['count'] == 1
    assert history.stat().st_mode & 0o777 == 0o600
    persisted = Path(env['XDG_CONFIG_HOME']) / 'lite-switch/assignments.json'
    assert all(a['type'] == 'application' for a in json.loads(persisted.read_text())['assignments'].values())
    invoke('quit')
    wait(lambda: panel() is None, 'quit')
    time.sleep(.2)
    processes.append(subprocess.Popen([str(binary), 'show'], env=env, stdout=log, stderr=log))
    wait(lambda: panel() and panel()['focused'], 'restarted launcher')
    restored = json.loads(invoke('assignments').stdout)
    assert 'b' in restored and 'a' not in restored and 'c' not in restored
    key('Escape')
    wait(lambda: panel() is None, 'restart dismissal')
    invoke('quit')
    processes[-1].wait(timeout=5)
    # A first-use CLI command must return while keeping its session target alive
    # in a hidden resident server; it must not hang the caller's terminal.
    invoke('assign-window', 'z', str(original['x11_id']))
    assert panel() is None
    assert 'z' in json.loads(invoke('assignments').stdout)
    invoke('activate', 'z')
    wait(lambda: window('Live Renamed Target')['focused'], 'cold CLI resident assignment')
    # Real WM replacement on this private server invalidates session assignments
    # without losing persistent app targets or killing the GTK resident process.
    processes.append(subprocess.Popen([executable('xfwm4'), '--replace', '--compositor=off'], env=env, stdout=log, stderr=log))
    def reconnected():
        result = invoke('assignments', check=False)
        if result.returncode:
            return False
        targets = json.loads(result.stdout)
        return 'z' not in targets and 'b' in targets
    wait(reconnected, 'WM replacement did not invalidate/reconnect resident targets')
    invoke('quit')
    time.sleep(.2)
    old_book = persisted.read_bytes()
    persisted.write_text('invalid JSON')
    try:
        assert invoke('assignments', check=False).returncode != 0
        assert persisted.read_text() == 'invalid JSON'
    finally:
        persisted.write_bytes(old_book)
    print('PASS: isolated GTK single instance, centering, unchanged geometry, focus/toggle/outside dismissal, live title/app discovery, safe launch/history, cycling, assignments/key capture, cold CLI and WM reconnection')
