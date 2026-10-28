#!/usr/bin/env python3
"""Private Xvfb/xfwm4/D-Bus tests; never uses the user's display or settings."""
import configparser
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/release/lite-switch-xfce'


def executable(name):
    path = shutil.which(name)
    if path:
        return path
    path = Path(os.environ.get('LITE_SWITCH_GTK_SDK', Path.home() / '.local/share/lite-switch/gtk4-sdk')) / 'usr/bin' / name
    if path.is_file():
        return str(path)
    raise RuntimeError(f'missing dependency: {name}')


def main():
    processes = []
    with tempfile.TemporaryDirectory(prefix='lite-switch-xfce-display-') as directory:
        root = Path(directory)
        env = os.environ.copy()
        env.pop('SESSION_MANAGER', None)
        env.update(GDK_BACKEND='x11', GTK_A11Y='none', NO_AT_BRIDGE='1', LITE_SWITCH_LAUNCHER=str(BINARY))
        env['DBUS_SYSTEM_BUS_ADDRESS'] = f'unix:path={root}/no-system-bus'
        env['LITE_SWITCH_ISOLATED_X11'] = '1'
        with open(root / 'session.log', 'w+') as log:
            read_fd, write_fd = os.pipe()
            try:
                processes.append(subprocess.Popen([executable('Xvfb'), '-displayfd', str(write_fd), '-screen', '0', '1280x900x24', '-nolisten', 'tcp'], pass_fds=(write_fd,), stdout=log, stderr=log))
                os.close(write_fd)
                assert select.select([read_fd], [], [], 10)[0], 'Xvfb did not start'
                display = os.read(read_fd, 32).decode().strip()
                os.close(read_fd)
                assert display.isdigit()
                env['DISPLAY'] = ':' + display
                # The wrapper bus has no service directories. Start only xfconf,
                # never portals, audio daemons or the real user's settings daemon.
                service = configparser.ConfigParser()
                service.read('/usr/share/dbus-1/services/org.xfce.Xfconf.service')
                command = shlex.split(service['D-BUS Service']['Exec'])
                processes.append(subprocess.Popen(command, env=env, stdout=log, stderr=log))
                absent = subprocess.run([str(BINARY), 'doctor'], env=env, capture_output=True, text=True, timeout=5)
                assert absent.returncode != 0 and 'window manager' in absent.stderr, absent
                processes.append(subprocess.Popen([executable('xfwm4'), '--compositor=off'], env=env, stdout=log, stderr=log))
                deadline = time.monotonic() + 10
                while True:
                    result = subprocess.run([str(BINARY), 'doctor'], env=env, capture_output=True, text=True, timeout=5)
                    if result.returncode == 0:
                        break
                    assert time.monotonic() < deadline, result.stderr
                    time.sleep(.05)
                assert json.loads(result.stdout)['window_manager'] == 'Xfwm4'
                processes.append(subprocess.Popen([executable('xfsettingsd'), '--no-daemon'], env=env, stdout=log, stderr=log))
                deadline = time.monotonic() + 10
                while True:
                    ready = subprocess.run(['xfconf-query', '-c', 'xfce4-keyboard-shortcuts', '-p', '/commands/custom/override'], env=env, capture_output=True, text=True)
                    if ready.returncode == 0 and ready.stdout.strip() == 'true':
                        break
                    assert time.monotonic() < deadline, ready.stderr
                    time.sleep(.05)
                subprocess.run(['cargo', 'test', '--locked', '--test', 'x11_smoke', '--', '--ignored', '--test-threads=1'], cwd=ROOT, env=env, check=True, timeout=90)
                rejected = subprocess.run([str(BINARY), 'focus', '4294967295'], env=env, capture_output=True, text=True, timeout=5)
                assert rejected.returncode != 0
                if not os.environ.get('LITE_SWITCH_NO_GUI_TEST'):
                    from gui_smoke import check_gui
                    check_gui(BINARY, env, root, processes, log, executable)
                    from bindings_smoke import check_bindings
                    check_bindings(BINARY, env, root, executable)
                    from matching_smoke import check_matching
                    check_matching(BINARY, env)
                    from actions_smoke import check_actions
                    check_actions(BINARY, env, executable)
                    from auto_sync_smoke import check_auto_sync
                    check_auto_sync(BINARY, env, root, executable)
                print('PASS: isolated xfwm4 discovery, Unicode/class/geometry, outputs, cross-workspace focus, minimized restore, dock exclusion and exact XID reuse protection')
            except BaseException:
                log.seek(0)
                print(log.read())
                raise
            finally:
                for process in reversed(processes):
                    if process.poll() is None:
                        process.terminate()
                for process in reversed(processes):
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == '__main__':
    if '--isolated-bus' in sys.argv:
        main()
    else:
        with tempfile.TemporaryDirectory(prefix='lite-switch-xfce-bus-') as directory:
            root = Path(directory)
            env = os.environ.copy()
            for variable in ['DBUS_SESSION_BUS_ADDRESS', 'SESSION_MANAGER', 'I3SOCK']:
                env.pop(variable, None)
            for variable, leaf in [('XDG_CONFIG_HOME', 'config'), ('XDG_STATE_HOME', 'state'), ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'), ('XDG_RUNTIME_DIR', 'runtime')]:
                path = root / leaf
                path.mkdir(mode=0o700)
                env[variable] = str(path)
            config = root / 'bus.conf'
            config.write_text(f'<busconfig><type>session</type><listen>unix:tmpdir={root}</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
            subprocess.run(['dbus-run-session', f'--config-file={config}', '--', sys.executable, str(Path(__file__).resolve()), '--isolated-bus'], env=env, check=True, timeout=110)
