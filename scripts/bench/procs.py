# SPDX-License-Identifier: Apache-2.0
"""Process-tree sampling for the ccbench harness (macOS, stdlib only).

CPU time and memory come from libproc (proc_pid_rusage / proc_pidinfo) via
ctypes, not from `ps`: nanosecond CPU resolution instead of 10 ms, and
ri_phys_footprint (Activity Monitor's "Memory") without spawning `footprint`.

Short-lived children: when a child exits and its parent reaps it, the child's
CPU time is added to the parent's ri_child_user_time/ri_child_system_time.
Summing (own + reaped-children) over the live tree therefore keeps counting CPU
from hook/statusLine/tool processes that start and die between two samples.
Processes reparented away from the tree (daemonised) escape this.
"""
import ctypes
import re
import subprocess
import threading
import time

_libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
_libsys = ctypes.CDLL("/usr/lib/libSystem.B.dylib")


class _Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


_tb = _Timebase()
_libsys.mach_timebase_info(ctypes.byref(_tb))
_TICK_NS = _tb.numer / _tb.denom  # mach absolute ticks -> ns (125/3 on Apple Silicon)

RUSAGE_INFO_V4 = 4
PROC_PIDTASKINFO = 4
# rusage_info_v4 offsets (bytes): uuid[16], then u64 fields
_OFF_USER, _OFF_SYS, _OFF_RES, _OFF_FOOT = 16, 24, 64, 72
_OFF_CUSER, _OFF_CSYS = 96, 104
_OFF_LIFEMAX = 240
_OFF_THREADS = 84  # proc_taskinfo.pti_threadnum (int32)


def _u64(buf, off):
    return int.from_bytes(buf[off:off + 8], "little")


def read_pid(pid):
    """Return dict for one pid or None if it vanished / is not readable."""
    buf = ctypes.create_string_buffer(1024)
    if _libproc.proc_pid_rusage(pid, RUSAGE_INFO_V4, buf) != 0:
        return None
    raw = buf.raw
    tbuf = ctypes.create_string_buffer(128)
    n = _libproc.proc_pidinfo(pid, PROC_PIDTASKINFO, 0, tbuf, 96)
    threads = int.from_bytes(tbuf.raw[_OFF_THREADS:_OFF_THREADS + 4], "little") if n == 96 else None
    pbuf = ctypes.create_string_buffer(4096)
    ln = _libproc.proc_pidpath(pid, pbuf, 4096)
    exe = pbuf.value.decode("utf-8", "replace") if ln > 0 else ""
    ns = lambda off: _u64(raw, off) * _TICK_NS / 1e9
    return {
        "pid": pid,
        "exe": exe,
        "cpu": ns(_OFF_USER) + ns(_OFF_SYS),            # own CPU seconds
        "ccpu": ns(_OFF_CUSER) + ns(_OFF_CSYS),         # reaped-children CPU seconds
        "rss": _u64(raw, _OFF_RES),                      # bytes
        "fp": _u64(raw, _OFF_FOOT),                      # phys_footprint bytes
        "fp_peak": _u64(raw, _OFF_LIFEMAX),              # lifetime max footprint
        "threads": threads,
    }


def ppid_table():
    out = subprocess.run(["/bin/ps", "-axo", "pid=,ppid="], capture_output=True, text=True).stdout
    kids, parent = {}, {}
    for line in out.splitlines():
        a = line.split()
        if len(a) == 2:
            p, pp = int(a[0]), int(a[1])
            parent[p] = pp
            kids.setdefault(pp, []).append(p)
    return kids, parent


def descendants(root, kids):
    seen, stack, order = set(), [root], []
    while stack:
        p = stack.pop()
        if p in seen:
            continue
        seen.add(p)
        order.append(p)
        stack.extend(kids.get(p, []))
    return order


def classify(info, root_pid, parent_role):
    """Role of one process. Rules documented in README of the harness."""
    exe = info["exe"]
    base = exe.rsplit("/", 1)[-1]
    if info["pid"] == root_pid:
        return "ui" if base == "claude-rs" else "ui+engine"
    if "claude-rs-bridge" in base:
        return "bridge"
    # the engine is the bridge's direct child; proc_pidpath() resolves hard links, so the
    # official binary can show up as "claude" or as its version file name ("2.1.306")
    if parent_role == "bridge" and re.fullmatch(r"claude|\d+\.\d+\.\d+.*", base):
        return "engine"
    return "other"


class Sampler(threading.Thread):
    """Samples the tree under `root` until stopped. Samples are kept in memory."""

    def __init__(self, root, interval=0.25):
        super().__init__(daemon=True)
        self.root, self.interval = root, interval
        self.samples = []  # dicts: t (monotonic), wall, procs[list]
        self.known_pids = set()  # every pid ever seen in the tree (for survivor check)
        self._stop_evt = threading.Event()
        self._lock = threading.Lock()

    def snapshot(self):
        t, wall = time.monotonic(), time.time()
        kids, parent = ppid_table()
        pids = descendants(self.root, kids)
        roles, procs = {}, []
        for pid in pids:  # parents come before children in DFS order
            info = read_pid(pid)
            if not info:
                continue
            info["ppid"] = parent.get(pid)
            info["role"] = classify(info, self.root, roles.get(info["ppid"]))
            roles[pid] = info["role"]
            procs.append(info)
        s = {"t": t, "wall": wall, "procs": procs}
        with self._lock:
            self.samples.append(s)
            self.known_pids.update(p["pid"] for p in procs)
        return s

    def run(self):
        while not self._stop_evt.is_set():
            t0 = time.monotonic()
            self.snapshot()
            self._stop_evt.wait(max(0.0, self.interval - (time.monotonic() - t0)))

    def stop(self):
        self._stop_evt.set()
        self.join(timeout=5)

    def window(self, t0, t1):
        with self._lock:
            return [s for s in self.samples if t0 <= s["t"] <= t1]

    def last(self):
        with self._lock:
            return self.samples[-1] if self.samples else None


def tree_cpu(sample, role=None):
    """Inclusive CPU seconds (own + reaped children) of a sample, optionally one role."""
    return sum(p["cpu"] + p["ccpu"] for p in sample["procs"] if role is None or p["role"] == role)


def tree_mem(sample, key, role=None):
    return sum(p[key] for p in sample["procs"] if role is None or p["role"] == role)
