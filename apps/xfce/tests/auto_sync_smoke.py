"""Opt-in automatic shortcut lifecycle on the isolated Xfce desktop only."""
import json
from pathlib import Path
import subprocess
import time


def check_auto_sync(binary, env, root, executable):
    assert env.get('LITE_SWITCH_ISOLATED_X11') == '1'
    channel = 'xfce4-keyboard-shortcuts'
    def invoke(*args, check=True):
        result = subprocess.run([str(binary),*args],env=env,capture_output=True,text=True,timeout=15)
        if check:
            assert result.returncode == 0,(args,result.stdout,result.stderr)
        return result
    def xf(*args):
        return subprocess.run(['xfconf-query','-c',channel,*args],env=env,capture_output=True,text=True,timeout=5)
    def prop(key):
        result = xf('-p',f'/commands/custom/<Super>{key}')
        return result.stdout.strip() if result.returncode == 0 else None
    def set_prop(key, value):
        args = ['-p',f'/commands/custom/<Super>{key}']
        if prop(key) is not None:
            assert xf(*args,'-r').returncode == 0
        args += ['-n','-t','string']
        assert xf(*args,'-s',value).returncode == 0
        time.sleep(.15)  # let xfsettingsd settle the external edit's native grab
    def snapshot():
        return xf('-lv').stdout
    def wait(callback,message):
        deadline = time.monotonic()+10
        while time.monotonic()<deadline:
            if callback():
                return
            time.sleep(.05)
        raise AssertionError(message)
    def windows():
        return json.loads(invoke('windows').stdout)
    def panel():
        return next((w for w in windows() if w['title']=='Lite Switch'),None)
    def key(chord):
        subprocess.run([executable('xdotool'),'key','--clearmodifiers',chord],env=env,check=True)
    def query(text):
        key('ctrl+a')
        subprocess.run([executable('xdotool'),'type','--clearmodifiers','--delay','1',text],env=env,check=True)
        time.sleep(.15)
    def capture(chord):
        subprocess.run([executable('xdotool'),'mousemove','--window',str(panel()['x11_id']),'620','125','click','1'],env=env,check=True)
        key(chord)
        # Xfconf properties appear before grab verification and GTK row rebuild
        # finish. A resident command is a main-loop barrier, then allow layout.
        invoke('assignments')
        time.sleep(.15)
    manifest = Path(env['XDG_CONFIG_HOME'])/'lite-switch/xfce-binding-installation.json'
    initial = snapshot()
    refused = invoke('set-shortcut-sync','on',check=False)
    assert refused.returncode != 0 and '--approve-xfce-shortcuts' in refused.stderr
    assert not json.loads(invoke('settings').stdout)['auto_sync_shortcuts']
    invoke('set-launcher','Super+space')
    invoke('set-modifier','Super')
    invoke('set-shortcut-sync','on','--approve-xfce-shortcuts')
    invoke('assign-action','j','center')
    assert not manifest.exists() and snapshot()==initial
    invoke('unassign','j')
    invoke('bindings-install','--approve-xfce-shortcuts')
    invoke('assign-app','q','cycle-test.desktop')
    assert prop('q') and 'activate q' in prop('q')
    assert 'old-b' in prop('b')  # moving an assignment restores the displaced key
    original = next(w for w in windows() if w['title']=='Live Renamed Target')
    invoke('focus',str(original['x11_id']))
    key('super+q')
    wait(lambda: any(w['title']=='Cycle Second' and w['focused'] for w in windows()),'new app key was not immediately active')

    marker = root/'auto-restored-m'
    set_prop('m',f'touch {marker}')
    key('super+space')
    wait(lambda: panel() and panel()['focused'],'automatic capture panel')
    query('Center Window')
    capture('m')
    wait(lambda: 'activate m' in (prop('m') or ''),'UI assignment was not synced')
    capture('n')
    wait(lambda: 'activate n' in (prop('n') or ''),'UI reassignment was not synced')
    assert prop('m')==f'touch {marker}'
    key('super+n')
    wait(lambda: panel() is None,'newly captured action key was not active')
    key('super+space')
    wait(lambda: panel() and panel()['focused'],'removal panel')
    query('Center Window')
    capture('BackSpace')
    wait(lambda: prop('n') is None,'UI removal did not release the key')
    key('super+m')
    wait(marker.exists,'displaced shortcut was not restored')
    key('Escape')
    wait(lambda: panel() is None,'capture panel dismissal')

    owned = prop('q')
    set_prop('q','touch /tmp/private-test-outside-edit')
    before, before_manifest = snapshot(),manifest.read_bytes()
    failed = invoke('assign-action','z','fill',check=False)
    assert failed.returncode != 0 and 'Preferences saved, but shortcut sync failed' in failed.stderr,failed
    assert snapshot()==before and manifest.read_bytes()==before_manifest
    assert json.loads(invoke('assignments').stdout)['z']['action']=='fill'
    set_prop('q',owned)
    invoke('assign-action','z','fill')
    assert prop('z') and 'activate z' in prop('z')
    invoke('unassign','z')
    assert prop('z') is None

    invoke('set-modifier','Alt')
    assert prop('q') is None
    assert xf('-p','/commands/custom/<Alt>q').returncode == 0
    invoke('set-modifier','Super')
    assert prop('q') and xf('-p','/commands/custom/<Alt>q').returncode != 0
    invoke('set-launcher','Control+Alt+Return')
    assert prop('space') is None
    key('ctrl+alt+Return')
    wait(lambda: panel() and panel()['focused'],'launcher setting was not automatically synced')
    key('Escape')
    wait(lambda: panel() is None,'updated launcher dismissal')
    invoke('set-launcher','Super+space')

    probe = subprocess.Popen([executable('xterm'),'-class','SessionShortcutProbe','-T','Session Shortcut Probe'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        wait(lambda: any(w['title']=='Session Shortcut Probe' for w in windows()),'session probe missing')
        target = next(w for w in windows() if w['title']=='Session Shortcut Probe')
        invoke('assign-window','y',str(target['x11_id']))
        assert prop('y') and 'activate y' in prop('y')
        invoke('quit')
        time.sleep(.2)
        assert 'y' not in json.loads(invoke('assignments').stdout)
        wait(lambda: prop('y') is None,'restart did not remove expired session shortcut')
        assert json.loads(invoke('settings').stdout)['auto_sync_shortcuts']
        invoke('assign-window','y',str(target['x11_id']))
        probe.terminate()
        probe.wait(timeout=5)
        wait(lambda: prop('y') is None,'window close did not release its shortcut automatically')
    finally:
        if probe.poll() is None:
            probe.terminate()
            probe.wait(timeout=5)
    invoke('bindings-uninstall','--approve-xfce-shortcuts')
    removed = snapshot()
    invoke('assign-action','z','fill')
    assert snapshot()==removed and not manifest.exists(),'auto sync reinstalled uninstalled shortcuts'
    invoke('unassign','z')
    invoke('set-shortcut-sync','off')
    invoke('assign-action','z','fill')
    assert snapshot()==removed
    invoke('unassign','z')
    invoke('quit')
    print('PASS: isolated opt-in auto sync, real app/action keys, UI assign/move/remove, conflict restoration, outside-edit safety, session close/restart cleanup, disable and uninstall protection')
