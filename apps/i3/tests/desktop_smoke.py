#!/usr/bin/env python3
"""Opt-in isolated X11/i3/GTK smoke test. Run under dbus-run-session.

Does not touch the user's i3 config, bindings, state, display, or autostart.
Needs Xvfb, i3, xterm, xdotool, and a GUI build of lite-switch.
"""
import ctypes
import json
import os
from pathlib import Path
import select
import shutil
import shlex
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
SDK = Path(os.environ.get("LITE_SWITCH_GTK_SDK", Path.home() / ".local/share/lite-switch/gtk4-sdk"))
BINARY = Path(os.environ.get("LITE_SWITCH_BINARY", ROOT / "target/release/lite-switch"))


def executable(name):
    path = shutil.which(name)
    if path:
        return path
    path = SDK / "usr/bin" / name
    if path.is_file():
        return str(path)
    raise RuntimeError(f"missing test dependency: {name}")


def wait_for(callback, message):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        value = callback()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError(f"timed out: {message}")


def all_windows(node):
    result = [node] if node.get("window") else []
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        result.extend(all_windows(child))
    return result


def physical_keycode(display_name, keysym):
    # Resolve against the live private server, not an assumed US keymap.
    x11 = ctypes.CDLL("libX11.so.6")
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(display_name.encode())
    assert display, "cannot open private X11 display"
    try:
        code = x11.XKeysymToKeycode(display, keysym)
        assert code, "test keysym has no physical keycode"
        return code
    finally:
        x11.XCloseDisplay(display)


def main():
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        raise RuntimeError("run with dbus-run-session -- python3 tests/desktop_smoke.py")
    processes = []
    with tempfile.TemporaryDirectory(prefix="lite-switch-desktop-") as tmp:
        tmp = Path(tmp)
        env = os.environ.copy()
        env.update(XDG_DATA_HOME=str(tmp / "data"), XDG_STATE_HOME=str(tmp / "state"),
                   XDG_CONFIG_HOME=str(tmp / "config"), XDG_RUNTIME_DIR=str(tmp / "runtime"),
                   GTK_A11Y="none", GDK_BACKEND="x11", LITE_SWITCH_LAUNCHER=str(BINARY))
        Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
        read_fd, write_fd = os.pipe()
        logs = open(tmp / "session.log", "w+")
        try:
            xvfb = subprocess.Popen([executable("Xvfb"), "-displayfd", str(write_fd),
                                     "-screen", "0", "1280x900x24", "-nolisten", "tcp"],
                                    pass_fds=(write_fd,), env=env, stdout=logs, stderr=logs)
            processes.append(xvfb)
            os.close(write_fd)
            assert select.select([read_fd], [], [], 10)[0], "Xvfb did not start"
            display = os.read(read_fd, 32).decode().strip()
            assert display.isdigit(), "invalid Xvfb display"
            os.close(read_fd)
            env["DISPLAY"] = f":{display}"
            quoted_launcher = tmp / "launcher \' ;, alias"
            quoted_launcher.symlink_to(BINARY)
            env["LITE_SWITCH_LAUNCHER"] = str(quoted_launcher)
            config = tmp / "i3 source \' config.conf"
            config_target = tmp / "dotfiles/i3.conf"
            config_target.parent.mkdir()
            config.symlink_to(config_target)
            config.write_text("# i3 config file (v4)\nfont pango:monospace 10\nfocus_follows_mouse no\n")
            env["I3SOCK"] = str(tmp / "ipc")
            old_launch = tmp / "old-launcher"
            old_cycle = tmp / "old-cycle"
            old_code = tmp / "old-code"
            old_release = tmp / "old-release"
            old_mode = tmp / "old-mode"
            defaults = tmp / "defaults ' directory/main.conf"
            nested = defaults.parent / "nested/keys.conf"
            nested.parent.mkdir(parents=True)
            defaults.write_text("set $cycle_key b\nbindsym $old_mod+$cycle_key \\\n exec --no-startup-id touch " + str(old_cycle) + "\ninclude nested/*.conf\n")
            nested.write_text(f"bindcode $old_mod+26 exec --no-startup-id touch {old_code}\nbindsym --release $old_mod+e exec --no-startup-id touch {old_release}\nbindsym $old_mod+z nop untouched\n")
            original_defaults = defaults.read_bytes()
            original_nested = nested.read_bytes()
            config.write_text(config.read_text() + f"ipc-socket {env['I3SOCK']}\nset $old_mod Mod4\ninclude {shlex.quote(str(defaults))}\nmode \"preserved\" {{\nbindsym $old_mod+space exec --no-startup-id touch {old_mode}\n}}\n")
            processes.append(subprocess.Popen([executable("i3"), "-c", str(config)],
                                              env=env, stdout=logs, stderr=logs))
            wait_for(lambda: Path(env["I3SOCK"]).exists(), "nested i3 socket")

            def tree():
                return json.loads(subprocess.check_output(["i3-msg", "-t", "get_tree"], env=env))

            def panel():
                return next((w for w in all_windows(tree()) if w.get("name") == "Lite Switch"), None)

            target_xid = None

            def target():
                return next((w for w in all_windows(tree()) if
                             (w.get("window") == target_xid if target_xid is not None
                              else w.get("name") == "Lite Switch Smoke Target")), None)

            def invoke(*commands):
                return subprocess.run([str(BINARY), *commands], env=env, capture_output=True,
                                      text=True, timeout=10, check=True)

            def key(*keys):
                subprocess.run([executable("xdotool"), "key", "--clearmodifiers", *keys], env=env, check=True)

            def type_query(text):
                subprocess.run([executable("xdotool"), "type", "--clearmodifiers", "--delay", "1", text],
                               env=env, check=True)
                time.sleep(0.2)

            processes.append(subprocess.Popen([executable("xterm"), "-class", "LiteSwitchSmokeTest", "-title", "Lite Switch Smoke Target"],
                                              env=env, stdout=logs, stderr=logs))
            original = wait_for(target, "test target window")
            target_xid = original["window"]
            wait_for(lambda: target().get("focused"), "target focus")
            marker = tmp / "launched"
            app_dir = Path(env["XDG_DATA_HOME"]) / "applications"
            app_dir.mkdir(parents=True)
            (app_dir / "lite-switch-smoke.desktop").write_text(
                "[Desktop Entry]\nType=Application\nName=Lite Switch Smoke Launch\n"
                f"Exec=/usr/bin/touch {marker}\nIcon=application-x-executable\n")
            processes.append(subprocess.Popen([str(BINARY), "show"], env=env, stdout=logs, stderr=logs))
            resident = processes[-1]
            wait_for(panel, "GTK panel")
            wait_for(lambda: panel() and panel().get("floating") in ("user_on", "auto_on"), "floating panel")
            assert target()["rect"] == original["rect"], "opening panel changed tiled geometry"
            assert panel()["rect"]["width"] == 680, panel()["rect"]
            wait_for(lambda: panel() and panel().get("focused"), "panel focus")
            invoke("show")
            assert len([w for w in all_windows(tree()) if w.get("name") == "Lite Switch"]) == 1
            key("Escape")
            wait_for(lambda: panel() is None, "Escape dismissal")
            wait_for(lambda: target().get("focused"), "previous focus restoration")
            invoke("toggle")
            wait_for(panel, "toggle open")
            invoke("toggle")
            wait_for(lambda: panel() is None, "toggle close")
            invoke("show")
            wait_for(lambda: panel() and panel().get("focused"), "search panel")
            type_query("Smoke Target")
            key("Return")
            wait_for(lambda: panel() is None and target().get("focused"), "window activation")
            invoke("show")
            wait_for(lambda: panel() and panel().get("focused"), "app launch panel")
            type_query("Lite Switch Smoke Launch")
            key("Return")
            wait_for(marker.exists, "desktop-entry launch")
            wait_for(lambda: panel() is None, "hide after app launch")
            usage_file = Path(env["XDG_STATE_HOME"]) / "lite-switch/usage.json"
            history = json.loads(usage_file.read_text())
            assert history["apps"]["lite-switch-smoke.desktop"]["count"] == 1
            assert usage_file.stat().st_mode & 0o777 == 0o600
            # Search before a title exists, then rename while the panel remains
            # visible. Enter can only work if the subscription refreshed results.
            invoke("show")
            wait_for(lambda: panel() and panel().get("focused"), "live-update panel")
            type_query("Live Renamed Target")
            subprocess.run([executable("xdotool"), "set_window", "--name", "Live Renamed Target", str(target_xid)], env=env, check=True)
            wait_for(lambda: target().get("name") == "Live Renamed Target", "i3 title change")
            time.sleep(0.3)
            key("Return")
            wait_for(lambda: panel() is None and target().get("focused"), "event-driven result refresh")

            # A desktop entry added after the panel opened must become searchable
            # through GIO's application monitor, without reopening the panel.
            invoke("show")
            wait_for(lambda: panel() and panel().get("focused"), "app-monitor panel")
            type_query("Live Application Added")
            added_marker = tmp / "added-app-launched"
            (app_dir / "live-added.desktop").write_text(
                "[Desktop Entry]\nType=Application\nName=Live Application Added\n"
                f"Exec=/usr/bin/touch {added_marker}\n")
            time.sleep(1.0)
            key("Return")
            wait_for(added_marker.exists, "event-driven installed-app discovery")
            wait_for(lambda: panel() is None, "hide after live-added app launch")

            unexpected_launch = tmp / "should-not-launch"
            (app_dir / "cycle-test.desktop").write_text(
                "[Desktop Entry]\nType=Application\nName=Cycle Test\n"
                f"StartupWMClass=LiteSwitchSmokeTest\nExec=/usr/bin/touch {unexpected_launch}\n")
            processes.append(subprocess.Popen([executable("xterm"), "-class", "LiteSwitchSmokeTest", "-title", "Cycle Second"],
                                              env=env, stdout=logs, stderr=logs))
            second = wait_for(lambda: next((w for w in all_windows(tree()) if w.get("name") == "Cycle Second"), None), "second app window")
            wait_for(lambda: next(w for w in all_windows(tree()) if w["window"] == second["window"]).get("focused"), "second window focus")
            invoke("cycle", "cycle-test.desktop")
            wait_for(lambda: target().get("focused"), "cycle to first window")
            # Move the second window without changing the current workspace.
            subprocess.run(["i3-msg", f"[con_id={second['id']}] move container to workspace 2"], env=env, check=True, stdout=subprocess.DEVNULL)
            invoke("cycle", "cycle-test.desktop")
            wait_for(lambda: next(w for w in all_windows(tree()) if w["window"] == second["window"]).get("focused"), "cross-workspace cycle")
            invoke("cycle", "cycle-test.desktop")
            wait_for(lambda: target().get("focused"), "cycle wrap to first workspace")
            assert not unexpected_launch.exists(), "cycling launched a duplicate app"
            # Assignment book: application bindings persist; window bindings
            # live only in the resident process and are excluded from app cycling.
            invoke("assign-window", "a", str(original["id"]))
            invoke("assign-app", "b", "cycle-test.desktop")
            assignments = json.loads(invoke("assignments").stdout)
            assert assignments["a"]["type"] == "window"
            assert assignments["b"]["launch_if_needed"] is False
            invoke("activate", "b")
            wait_for(lambda: next(w for w in all_windows(tree()) if w["window"] == second["window"]).get("focused"), "cycle excludes assigned first window")
            invoke("activate", "a")
            wait_for(lambda: target().get("focused"), "direct window assignment activation")
            invoke("assign-window", "c", str(second["id"]))
            blocked = subprocess.run([str(BINARY), "activate", "b"], env=env, capture_output=True, text=True, timeout=10)
            assert blocked.returncode != 0 and "individually assigned" in blocked.stderr
            assert not unexpected_launch.exists()

            # Capture/reassign/remove a shortcut using the actual GTK row button.
            invoke("show")
            wait_for(lambda: panel() and panel().get("focused"), "assignment panel")
            type_query("Live Renamed Target")
            panel_xid = str(panel()["window"])
            subprocess.run([executable("xdotool"), "mousemove", "--window", panel_xid, "620", "125", "click", "1"], env=env, check=True)
            key("d")
            saved = json.loads(invoke("assignments").stdout)
            assert "d" in saved and "a" not in saved, saved
            subprocess.run([executable("xdotool"), "mousemove", "--window", panel_xid, "620", "125", "click", "1"], env=env, check=True)
            key("BackSpace")
            assert "d" not in json.loads(invoke("assignments").stdout)
            key("Escape")
            wait_for(lambda: panel() is None, "assignment panel dismissed")
            invoke("assign-window", "e", str(original["id"]))
            invoke("unassign", "c")

            # Approve managed installation only in this throwaway session.
            # The user's i3 files, bindings, and session are never touched.
            saved_config = config.read_text()
            saved_mode = config.stat().st_mode & 0o777
            state_dir = Path(env["XDG_CONFIG_HOME"]) / "lite-switch"
            include = state_dir / "i3-bindings.conf"
            manifest = state_dir / "binding-installation.json"
            assert json.loads(invoke("bindings-status").stdout)["installed"] is False
            unapproved = subprocess.run([str(BINARY), "bindings-install"], env=env, capture_output=True, text=True, timeout=10)
            assert unapproved.returncode != 0 and "--approve-i3-config" in unapproved.stderr
            assert config.read_text() == saved_config and not include.exists()
            invoke("set-launcher", "Super+space")
            invoke("set-modifier", "Super")
            assert json.loads(invoke("settings").stdout)["launcher_shortcut"] == "Mod4+space"
            assert "passed" in invoke("bindings-check").stdout
            code = physical_keycode(env["DISPLAY"], ord("e"))
            nested.write_text(nested.read_text().replace("bindcode $old_mod+26 ", f"bindcode $old_mod+{code} "))
            original_nested = nested.read_bytes()
            # Lite Switch wins over a newly added root binding, variable-based
            # bindings in nested/glob includes, physical bindcode, and release.
            config.write_text(saved_config + f"bindsym $old_mod+space exec --no-startup-id touch {old_launch}\n")
            saved_config = config.read_text()
            before_check = (config.read_bytes(), defaults.read_bytes(), nested.read_bytes())
            preview = invoke("bindings-check").stdout
            assert "4 existing binding(s) displaced" in preview, preview
            assert before_check == (config.read_bytes(), defaults.read_bytes(), nested.read_bytes())
            installed = invoke("bindings-install", "--approve-i3-config").stdout
            assert "4 existing binding(s) displaced" in installed, installed
            installed_root = config.read_text()
            assert config.is_symlink() and config.stat().st_mode & 0o777 == saved_mode
            state = json.loads(invoke("bindings-status").stdout)
            assert state["installed"] and state["intact"] and not state["pending_changes"], state
            assert state["priority"] == "lite-switch-first" and len(state["displaced"]) == 4
            assert defaults.read_bytes() != original_defaults and nested.read_bytes() != original_nested
            backups = list((state_dir / "backups").glob("*.json"))
            assert backups and all(p.stat().st_mode & 0o777 == 0o600 for p in backups)
            assert "already up to date" in invoke("bindings-install", "--approve-i3-config").stdout
            assert list((state_dir / "backups").glob("*.json")) == backups
            assert "passed" in invoke("bindings-check").stdout
            assert config.read_text() == installed_root
            key("super+space")
            wait_for(lambda: panel() and panel().get("focused"), "exported launcher hotkey")
            key("Escape")
            wait_for(lambda: panel() is None, "hotkey panel dismissal")
            key("super+b")
            wait_for(lambda: next(w for w in all_windows(tree()) if w["window"] == second["window"]).get("focused"), "exported app hotkey")
            key("super+e")
            wait_for(lambda: target().get("focused"), "exported window hotkey")
            assert not any(path.exists() for path in [old_launch, old_cycle, old_code, old_release])
            subprocess.run(["i3-msg", "mode preserved"], env=env, check=True, stdout=subprocess.DEVNULL)
            key("super+space")
            wait_for(old_mode.exists, "non-default mode binding preserved")
            assert panel() is None
            subprocess.run(["i3-msg", "mode default"], env=env, check=True, stdout=subprocess.DEVNULL)
            invoke("set-launcher", "Control+Alt+space")
            invoke("set-modifier", "Alt")
            assert json.loads(invoke("bindings-status").stdout)["pending_changes"]
            invoke("bindings-sync", "--approve-i3-config")
            assert not json.loads(invoke("bindings-status").stdout)["pending_changes"]
            assert defaults.read_bytes() == original_defaults and nested.read_bytes() == original_nested
            key("super+space")
            wait_for(old_launch.exists, "old launcher restored after moving new chord")
            assert panel() is None
            assert config.read_text().count("BEGIN LITE SWITCH MANAGED BINDINGS") == 1
            key("ctrl+alt+space")
            wait_for(lambda: panel() and panel().get("focused"), "updated launcher hotkey")
            key("Escape")
            wait_for(lambda: panel() is None, "updated hotkey panel dismissal")
            key("alt+b")
            wait_for(lambda: next(w for w in all_windows(tree()) if w["window"] == second["window"]).get("focused"), "updated app modifier")
            key("alt+e")
            wait_for(lambda: target().get("focused"), "updated window modifier")

            # A later colliding user binding is displaced on sync as well, then
            # restored when Lite Switch moves away. Unrelated invalid syntax
            # and hand-edited owned blocks remain protected, not silently ignored.
            invoke("set-launcher", "Super+F11")
            old_f11 = tmp / "old-f11"
            manual_line = f"bindsym Mod4+F11 exec --no-startup-id touch {old_f11}\n"
            config.write_text(config.read_text() + manual_line)
            update = invoke("bindings-sync", "--approve-i3-config").stdout
            assert "1 existing binding(s) displaced" in update, update
            key("super+F11")
            wait_for(lambda: panel() and panel().get("focused"), "new chord replaces later conflict")
            assert not old_f11.exists()
            key("Escape")
            wait_for(lambda: panel() is None, "priority update panel dismissal")
            invoke("set-launcher", "Control+Alt+space")
            invoke("bindings-sync", "--approve-i3-config")
            key("super+F11")
            wait_for(old_f11.exists, "later displaced binding restored")
            valid_root = config.read_bytes()
            config.write_bytes(valid_root + b"this-is-not-valid-i3-config\n")
            before = (config.read_bytes(), include.read_bytes(), manifest.read_bytes())
            rejected = subprocess.run([str(BINARY), "bindings-sync", "--approve-i3-config"], env=env, capture_output=True, text=True, timeout=10)
            assert rejected.returncode != 0
            assert before == (config.read_bytes(), include.read_bytes(), manifest.read_bytes())
            config.write_bytes(valid_root)
            original_include = include.read_bytes()
            include.write_bytes(original_include + b"# manual edit\n")
            rejected = subprocess.run([str(BINARY), "bindings-sync", "--approve-i3-config"], env=env, capture_output=True, text=True, timeout=10)
            assert rejected.returncode != 0 and "edited" in rejected.stderr
            assert include.read_bytes().endswith(b"# manual edit\n")
            include.write_bytes(original_include)

            # Closing a window invalidates its session assignment immediately.
            invoke("assign-window", "c", str(second["id"]))
            subprocess.run(["i3-msg", f"[con_id={second['id']}] kill"], env=env, check=True, stdout=subprocess.DEVNULL)
            wait_for(lambda: "c" not in json.loads(invoke("assignments").stdout), "closed-window assignment cleanup")
            invoke("assign-app", "l", "lite-switch-smoke.desktop")
            marker.unlink()
            blocked = subprocess.run([str(BINARY), "activate", "l"], env=env, capture_output=True, text=True, timeout=10)
            assert blocked.returncode != 0 and "launch-if-needed is disabled" in blocked.stderr
            invoke("launch-if-needed", "l", "on")
            invoke("activate", "l")
            wait_for(marker.exists, "launch-if-needed preference")
            persisted = json.loads((Path(env["XDG_CONFIG_HOME"]) / "lite-switch/assignments.json").read_text())
            assert all(value["type"] == "application" for value in persisted["assignments"].values())
            invoke("quit")
            resident.wait(timeout=5)
            processes.append(subprocess.Popen([str(BINARY), "show"], env=env, stdout=logs, stderr=logs))
            resident = processes[-1]
            wait_for(panel, "restart with saved assignments")
            restored = json.loads(invoke("assignments").stdout)
            assert "e" not in restored and restored["b"]["type"] == "application"
            assert restored["l"]["launch_if_needed"] is True
            assert json.loads(invoke("settings").stdout)["launcher_shortcut"] == "Control+Mod1+space"
            before_uninstall = config.read_bytes()
            unapproved = subprocess.run([str(BINARY), "bindings-uninstall"], env=env, capture_output=True, text=True, timeout=10)
            assert unapproved.returncode != 0 and config.read_bytes() == before_uninstall
            invoke("bindings-uninstall", "--approve-i3-config")
            assert config.read_text() == saved_config + manual_line
            assert defaults.read_bytes() == original_defaults and nested.read_bytes() == original_nested
            assert config.is_symlink() and config.stat().st_mode & 0o777 == saved_mode
            assert not include.exists() and not manifest.exists()
            assert json.loads(invoke("bindings-status").stdout)["installed"] is False
            invoke("quit")
            resident.wait(timeout=5)
            assert target()["rect"] == original["rect"], "closing panel changed tiled geometry"
            print("PASS: isolated GTK/i3 geometry, single instance, focus, toggle, activation,")
            print("      live updates, cross-workspace cycling, UI key capture, global bindings,")
            print("      binding priority across variable/glob includes, bindcodes/releases/modes,")
            print("      exact restoration, configurable chords, and opt-in binding lifecycle")
        except BaseException:
            logs.flush()
            print((tmp / "session.log").read_text())
            try:
                print("Failure windows:", [(w.get("name"), w.get("focused"), w.get("window")) for w in all_windows(tree())])
            except Exception as diagnostic_error:
                print("Could not collect failure windows:", diagnostic_error)
            raise
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
            logs.close()


if __name__ == "__main__":
    if sys.argv[1:] == ["--isolated-bus"]:
        main()
    else:
        # Never share the user's session bus/application instance, even when
        # invoked from a normal desktop terminal.
        with tempfile.TemporaryDirectory(prefix="lite-switch-test-bus-") as bus_dir:
            config = Path(bus_dir) / "bus.conf"
            config.write_text(
                f'<busconfig><type>session</type><listen>unix:tmpdir={bus_dir}</listen>'
                '<auth>EXTERNAL</auth><policy context="default">'
                '<allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>'
                '</policy></busconfig>')
            # Deliberately omit service directories: testing the launcher must
            # not start portals/audio services from the host's desktop session.
            subprocess.run(["dbus-run-session", f"--config-file={config}", "--", sys.executable,
                            str(Path(__file__).resolve()), "--isolated-bus"], check=True)
