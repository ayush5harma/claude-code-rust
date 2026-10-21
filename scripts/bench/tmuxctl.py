# SPDX-License-Identifier: Apache-2.0
"""tmux control on a private server socket (-L ccbench) so the user's own tmux is never touched."""
import re
import shlex
import subprocess
import time

SOCKET = "ccbench"
PREFIX = "ccbench-"
COLS, ROWS = 120, 40


def _tmux(*args, check=False):
    return subprocess.run(["tmux", "-L", SOCKET, "-f", "/dev/null", *args],
                          capture_output=True, text=True, check=check)


def new_session(name, argv, cwd, env):
    """Start `argv` under `env -i <env>` in a detached COLSxROWS session. Returns pane pid."""
    assert name.startswith(PREFIX), "session names must start with ccbench-"
    envargs = [f"{k}={v}" for k, v in env.items()]
    cmd = shlex.join(["/usr/bin/env", "-i", *envargs, *argv])
    r = _tmux("new-session", "-d", "-s", name, "-x", str(COLS), "-y", str(ROWS), "-c", cwd,
              "-P", "-F", "#{pane_pid}", cmd)
    if r.returncode != 0:
        raise RuntimeError(f"tmux new-session failed: {r.stderr.strip()}")
    return int(r.stdout.strip())


def capture(name):
    r = _tmux("capture-pane", "-p", "-t", name)
    return r.stdout if r.returncode == 0 else ""


def send_literal(name, text):
    _tmux("send-keys", "-t", name, "-l", text, check=True)


def send_keys(name, *keys):
    _tmux("send-keys", "-t", name, *keys, check=True)


def session_alive(name):
    return _tmux("has-session", "-t", name).returncode == 0


def kill_session(name):
    assert name.startswith(PREFIX)
    _tmux("kill-session", "-t", name)


def kill_server_if_empty():
    """Only ever the private ccbench server; never the default tmux server."""
    if not _tmux("list-sessions").stdout.strip():
        _tmux("kill-server")


def poll_until(name, predicate, timeout, interval=0.02):
    """Poll capture-pane until predicate(pane) is truthy. Returns (t_monotonic, pane) or (None, pane)."""
    deadline = time.monotonic() + timeout
    pane = ""
    while time.monotonic() < deadline:
        pane = capture(name)
        if predicate(pane):
            return time.monotonic(), pane
        time.sleep(interval)
    return None, pane


def strip_ui(pane):
    return re.sub(r"[\s│❯|]", "", pane)
