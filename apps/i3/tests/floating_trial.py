#!/usr/bin/env python3
"""Validate the workspace-9 floating experiment on a private X11/i3 server."""
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile

from desktop_smoke import all_windows, executable, wait_for

# Legacy X11 clients (including xterm) may have window_type=unknown.
# Workspace matching also includes them, while docks have no workspace.
RULE = 'for_window [workspace="^9$"] floating enable\n'


def main():
    processes = []
    with tempfile.TemporaryDirectory(prefix="lite-switch-floating-trial-") as directory:
        root = Path(directory)
        env = os.environ.copy()
        for name in ["DBUS_SESSION_BUS_ADDRESS", "SESSION_MANAGER"]:
            env.pop(name, None)
        env["I3SOCK"] = str(root / "ipc")
        read_fd, write_fd = os.pipe()
        with open(root / "session.log", "w+") as log:
            try:
                processes.append(subprocess.Popen(
                    [executable("Xvfb"), "-displayfd", str(write_fd), "-screen", "0", "1280x900x24", "-nolisten", "tcp"],
                    pass_fds=(write_fd,), stdout=log, stderr=log))
                os.close(write_fd)
                assert select.select([read_fd], [], [], 10)[0], "Xvfb did not start"
                display = os.read(read_fd, 32).decode().strip()
                os.close(read_fd)
                assert display.isdigit()
                env["DISPLAY"] = f":{display}"
                config = root / "config"
                config.write_text('# i3 config file (v4)\nfont pango:monospace 10\nfocus_follows_mouse no\n'
                                  f'ipc-socket {env["I3SOCK"]}\n' + RULE +
                                  'bindsym Mod1+Shift+space floating toggle\n')
                processes.append(subprocess.Popen([executable("i3"), "-c", str(config)], env=env, stdout=log, stderr=log))
                wait_for(lambda: Path(env["I3SOCK"]).exists(), "private i3 socket")

                def command(value):
                    replies = json.loads(subprocess.check_output(["i3-msg", value], env=env))
                    assert replies and all(reply["success"] for reply in replies), replies

                def window(title):
                    tree = json.loads(subprocess.check_output(["i3-msg", "-t", "get_tree"], env=env))
                    return next((w for w in all_windows(tree) if w.get("name") == title), None)

                command("workspace number 1")
                processes.append(subprocess.Popen(["xterm", "-T", "Outside trial"], env=env, stdout=log, stderr=log))
                plain = wait_for(lambda: window("Outside trial"), "ordinary window")
                assert plain["floating"] in ["auto_off", "user_off"]
                original_rect = plain["rect"]
                command("workspace number 9")
                processes.append(subprocess.Popen(["xterm", "-T", "Floating trial"], env=env, stdout=log, stderr=log))
                trial = wait_for(lambda: window("Floating trial"), "trial window")
                assert trial["floating"] == "user_on", trial
                assert window("Outside trial")["rect"] == original_rect
                subprocess.run([executable("xdotool"), "key", "--clearmodifiers", "alt+shift+space"], env=env, check=True)
                wait_for(lambda: window("Floating trial")["floating"] == "user_off", "existing tiling toggle")
                command("workspace number 1")
                assert window("Outside trial")["rect"] == original_rect
                print("PASS: workspace-9 windows float; existing toggle tiles them; other workspace geometry stays unchanged")
            finally:
                for process in reversed(processes):
                    process.terminate()
                for process in reversed(processes):
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
