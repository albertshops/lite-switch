"""Previous-window action/UI/persistence checks on the isolated desktop."""
import json
from pathlib import Path
import subprocess
import time


def check_actions(binary, env, executable):
    assert env.get('LITE_SWITCH_ISOLATED_X11') == '1'
    def invoke(*args, check=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=10)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result
    def wait(callback, message):
        deadline = time.monotonic()+10
        while time.monotonic()<deadline:
            if callback():
                return
            time.sleep(.05)
        raise AssertionError(message)
    def windows():
        return json.loads(invoke('windows').stdout)
    def window(title):
        return next((w for w in windows() if w['title']==title),None)
    def panel():
        return window('Lite Switch')
    def key(chord):
        subprocess.run([executable('xdotool'),'key','--clearmodifiers',chord],env=env,check=True)
    def query(text):
        key('ctrl+a')
        subprocess.run([executable('xdotool'),'type','--clearmodifiers','--delay','1',text],env=env,check=True)
        time.sleep(.15)
    def snapshot():
        return subprocess.check_output(['xfconf-query','-c','xfce4-keyboard-shortcuts','-lv'],env=env)

    available = json.loads(invoke('actions').stdout)
    assert len(available)==9 and any(a['id']=='close' for a in available) and not any(a['id']=='tile' for a in available)
    assert invoke('action','fill;kill',check=False).returncode != 0
    original = window('Live Renamed Target')
    assert original
    other_rect = window('Cycle Second')['rect']
    before_shortcuts = snapshot()
    invoke('assignments')  # start a hidden resident before remote show commands
    invoke('focus',str(original['x11_id']))
    invoke('show')
    wait(lambda: panel() and panel()['focused'],'action search panel')
    query('Move to Right Half')
    key('Return')
    wait(lambda: panel() is None and window('Live Renamed Target')['focused'],'previous-window action did not dismiss/focus')
    right = window('Live Renamed Target')['rect']
    assert right['x']>=640 and right['x']+right['width']==1280 and right['height']<=900,right
    assert window('Cycle Second')['rect']==other_rect
    left = json.loads(invoke('action','left-half').stdout)
    assert left['x11_id']==original['x11_id'] and left['actual']['x']==0 and left['actual']['width']<=640,left
    filled = json.loads(invoke('action','fill').stdout)
    assert filled['requested']==dict(x=0,y=0,width=1280,height=900),filled
    actual = filled['actual']
    assert actual['width']>1200 and actual['height']>800 and actual['width']<=1280 and actual['height']<=900,actual
    assert not window('Live Renamed Target')['fullscreen']
    invoke('assign-action','f','fill')
    invoke('assign-action','g','fill')
    targets = json.loads(invoke('assignments').stdout)
    assert 'f' not in targets and targets['g']=={'type':'action','action':'fill'}
    invoke('show')
    wait(lambda: panel() and panel()['focused'],'direct action while panel focused')
    invoke('activate','g')
    wait(lambda: panel() is None,'assigned action did not dismiss panel')
    assert window('Live Renamed Target')['rect']['width']>1200
    invoke('show')
    wait(lambda: panel() and panel()['focused'],'action key capture panel')
    assert panel()['rect']['width']==680 and panel()['rect']['height']==560
    query('Center Window')
    subprocess.run([executable('xdotool'),'mousemove','--window',str(panel()['x11_id']),'620','125','click','1'],env=env,check=True)
    key('h')
    wait(lambda: json.loads(invoke('assignments').stdout).get('h')=={'type':'action','action':'center'},'action key capture')
    invoke('activate','h')
    centered = window('Live Renamed Target')['rect']
    assert abs(centered['x']-(1280-centered['width'])//2)<=1 and abs(centered['y']-(900-centered['height'])//2)<=1,centered
    missing = invoke('action','next-output',check=False)
    assert missing.returncode!=0 and 'no other active output' in missing.stderr
    assert window('Live Renamed Target')['rect']==centered
    persisted = Path(env['XDG_CONFIG_HOME'])/'lite-switch/assignments.json'
    assert json.loads(persisted.read_text())['assignments']['g']['type']=='action'
    invoke('quit')
    time.sleep(.2)
    invoke('activate','g')
    assert json.loads(invoke('assignments').stdout)['h']['action']=='center'
    assert window('Live Renamed Target')['rect']['width']>1200
    assert window('Cycle Second')['rect']==other_rect
    assert snapshot()==before_shortcuts
    # Use a disposable client, never a window belonging to the real session.
    probe = subprocess.Popen([executable('xterm'), '-title', 'Close Action Probe'], env=env,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait(lambda: window('Close Action Probe'), 'close probe missing')
        invoke('focus', str(window('Close Action Probe')['x11_id']))
        invoke('assign-action', 'c', 'close')
        invoke('show')
        wait(lambda: panel() and panel()['focused'], 'close action panel missing')
        invoke('activate', 'c')
        wait(lambda: window('Close Action Probe') is None and panel() is None,
             'close did not target previous window')
        assert window('Cycle Second')['rect']==other_rect
        invoke('unassign', 'c')
    finally:
        if probe.poll() is None:
            probe.terminate()
        probe.wait(timeout=5)
    invoke('unassign','g')
    invoke('unassign','h')
    invoke('quit')
    print('PASS: isolated previous-window action search/key capture, half/fill/center with terminal size hints, persistent action shortcuts, no-output safety and unchanged other windows/desktop shortcuts')
