# SPDX-License-Identifier: Apache-2.0
"""ccbench driver: claude-rs vs official Claude Code. Run via run.sh (python3 -I)."""
import argparse
import hashlib
import json
import os
import random
import re
import shutil
import signal
import statistics
import subprocess
import sys
import time
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I drops the script dir
import procs
import tmuxctl as tx

SCRATCH = "/private/tmp/ccbench-cwd"          # fixed path: trust is per path; recreated every session
# Pass CLAUDE_CONFIG_DIR through only when set: setting it explicitly, even to ~/.claude, changes where
# Claude Code looks for its credentials and global config.
CONFIG_DIR_ENV = os.environ.get("CLAUDE_CONFIG_DIR")
CONFIG_DIR = CONFIG_DIR_ENV or os.path.expanduser("~/.claude")
PROJECT_DIR = os.path.join(CONFIG_DIR, "projects", "-private-tmp-ccbench-cwd")
PROMPT = "Write the integers from 1 to 400, one per line, with no other text."
TYPING = "".join(f"w{i:03d} " for i in range(1, 61))   # 300 chars, 60 greppable tokens
TYPING_EXPECT = "".join(f"w{i:03d}" for i in range(1, 61))
GRACE_S, IDLE_S, SETTLE_S = 5.0, 30.0, 3.0
SCROLL_WINDOW_S = 4.0
QUIET_WINDOW_S, QUIET_CORE_FRAC = 2.0, 0.03   # "settled" = <3% of one core over a 2 s window


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- tool definitions
def base_env(extra=None):
    env = {"HOME": os.environ["HOME"], "USER": os.environ.get("USER", ""), "LOGNAME": os.environ.get("USER", ""),
           "SHELL": "/bin/zsh", "PATH": os.environ["PATH"], "LANG": "en_US.UTF-8", "TERM": "screen-256color",
           "DISABLE_AUTOUPDATER": "1", "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
    if CONFIG_DIR_ENV:
        env["CLAUDE_CONFIG_DIR"] = CONFIG_DIR_ENV
    env.update(extra or {})
    return env


def tool_spec(tool, a, resume_id=None):
    mcp = ["--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}'] if a.mcp == "none" else []
    if tool == "claude-rs":
        argv = [a.claude_rs, "--model", "haiku", "--no-update-check", *mcp]
        extra = {"CLAUDE_RUST_NO_UPDATE_CHECK": "1"}
        if a.engine_path:
            extra["CLAUDE_CODE_EXECUTABLE"] = a.engine_path
    else:
        argv = [a.claude, "--model", "haiku", *mcp]
        extra = {}
    if resume_id:
        argv += ["--resume", resume_id]
    return argv, base_env(extra)


def is_ready(tool, pane):
    if "Quick safety check" in pane or "Enter to confirm" in pane:
        return False
    if tool == "claude-rs":
        return "Type a message" in pane and re.search(r"ctx [▓░]+", pane) is not None
    return re.search(r"(?m)^\s*❯\s", pane) is not None


def first_paint(tool, pane):
    return ("Type a message" in pane) if tool == "claude-rs" else is_ready(tool, pane)


# ---------------------------------------------------------------- scratch + cleanup
def fresh_scratch():
    shutil.rmtree(SCRATCH, ignore_errors=True)
    os.makedirs(SCRATCH)
    subprocess.run(["git", "init", "-q", SCRATCH], check=True, capture_output=True)


def clean_project_dir():
    """Delete ONLY the scratch cwd's project dir under the config dir (guarded)."""
    target = os.path.realpath(PROJECT_DIR)
    want = os.path.join(os.path.realpath(os.path.join(CONFIG_DIR, "projects")), "-private-tmp-ccbench-cwd")  # projects/ may be a symlink
    for _ in range(4):                      # a dying process can recreate files mid-delete
        if target != want or not os.path.isdir(target):
            return
        shutil.rmtree(target, ignore_errors=True)
        time.sleep(0.3)


def clean_all():
    shutil.rmtree(SCRATCH, ignore_errors=True)
    clean_project_dir()


# ---------------------------------------------------------------- session lifecycle
class Session:
    def __init__(self, tool, a, label, resume_id=None):
        self.tool, self.a, self.label = tool, a, label
        self.name = f"{tx.PREFIX}{tool}-{label}-{random.randrange(16**5):05x}"
        self.argv, self.env = tool_spec(tool, a, resume_id)
        self.sampler = None
        self.root = None
        self.t_spawn = None
        self.survivors = []

    def start(self):
        self.t_spawn = time.monotonic()
        self.root = tx.new_session(self.name, self.argv, SCRATCH, self.env)
        self.sampler = procs.Sampler(self.root, interval=0.25)
        self.sampler.start()
        return self

    def finish(self):
        """Tear down WITHOUT letting the tools exit gracefully.

        A graceful exit fires the user's SessionEnd hooks (which may archive transcripts or call a model),
        so every benchmark session would leak into whatever those hooks do.
        So: take a last snapshot, SIGKILL exactly the recorded descendants (exe re-verified, own pids only),
        then drop the tmux session and verify nothing recorded survives."""
        try:
            self.sampler.snapshot()               # catch anything not yet sampled
        except Exception:
            pass
        self.sampler.stop()
        known = {p["pid"]: p["exe"] for s in self.sampler.samples for p in s["procs"]}
        for pid, exe in known.items():
            if _same_proc(pid, exe):
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        tx.kill_session(self.name)
        end = time.monotonic() + 6
        alive = dict(known)
        while time.monotonic() < end:
            alive = {pid: exe for pid, exe in alive.items() if _same_proc(pid, exe)}
            if not alive:
                break
            time.sleep(0.2)
        self.survivors = [{"pid": p, "exe": e} for p, e in alive.items()]
        tx.kill_server_if_empty()
        return self.survivors


def _same_proc(pid, exe):
    info = procs.read_pid(pid)
    return bool(info) and info["exe"] == exe


# ---------------------------------------------------------------- analysis helpers
ROLES = ("ui", "bridge", "engine", "ui+engine", "other")


def agg(sample):
    out = {"total": _agg_one(sample["procs"])}
    for r in ROLES:
        ps = [p for p in sample["procs"] if p["role"] == r]
        if ps:
            out[r] = _agg_one(ps)
    return out


def _agg_one(ps):
    return {"cpu_incl": sum(p["cpu"] + p["ccpu"] for p in ps), "cpu_live": sum(p["cpu"] for p in ps),
            "rss": sum(p["rss"] for p in ps), "fp": sum(p["fp"] for p in ps),
            "nprocs": len(ps), "threads": sum(p["threads"] or 0 for p in ps)}


def cpu_at(samples, t, key="total", field="cpu_incl"):
    """Linear interpolation of cumulative CPU at monotonic time t."""
    pts = [(s["t"], agg(s).get(key, {}).get(field, 0.0)) for s in samples]
    if not pts:
        return None
    if t <= pts[0][0]:
        return pts[0][1]
    for (t0, c0), (t1, c1) in zip(pts, pts[1:]):
        if t0 <= t <= t1:
            return c0 + (c1 - c0) * (t - t0) / (t1 - t0) if t1 > t0 else c1
    return pts[-1][1]


def window_stats(samples, t0, t1):
    """CPU delta and memory stats between t0 and t1, whole tree + per role."""
    win = [s for s in samples if t0 <= s["t"] <= t1]
    if len(win) < 2:
        return None
    a0, a1 = agg(win[0]), agg(win[-1])
    dur = win[-1]["t"] - win[0]["t"]
    aggs = [agg(s) for s in win]
    out = {"duration_s": dur, "n_samples": len(win)}
    for key in a1:
        c0 = a0.get(key, {}).get("cpu_incl", 0.0)
        l0 = a0.get(key, {}).get("cpu_live", 0.0)
        d = a1[key]["cpu_incl"] - c0
        out[key] = {
            "cpu_s": d, "cpu_live_s": a1[key]["cpu_live"] - l0, "cpu_pct_of_core": 100 * d / dur if dur else None,
            "rss_mean": statistics.mean(x.get(key, {}).get("rss", 0) for x in aggs),
            "rss_max": max(x.get(key, {}).get("rss", 0) for x in aggs),
            "fp_mean": statistics.mean(x.get(key, {}).get("fp", 0) for x in aggs),
            "fp_max": max(x.get(key, {}).get("fp", 0) for x in aggs),
            "nprocs_mean": statistics.mean(x.get(key, {}).get("nprocs", 0) for x in aggs),
            "threads_mean": statistics.mean(x.get(key, {}).get("threads", 0) for x in aggs),
        }
    return out


def wait_quiet(sess, t_from, timeout):
    """Earliest moment >= t_from after which tree CPU stays < QUIET_CORE_FRAC of a core for QUIET_WINDOW_S.
    Blocks until found. Returns (t_quiet_start, cumulative_cpu_then) or (None, None)."""
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        time.sleep(0.25)
        samples = [s for s in sess.sampler.samples[:] if s["t"] >= t_from]
        for i, s0 in enumerate(samples):
            s1 = next((s for s in samples[i:] if s["t"] >= s0["t"] + QUIET_WINDOW_S), None)
            if s1 is None:
                break
            if procs.tree_cpu(s1) - procs.tree_cpu(s0) < QUIET_WINDOW_S * QUIET_CORE_FRAC:
                return s0["t"], procs.tree_cpu(s0)
    return None, None


def timeline(samples, t_base):
    return [{"t": round(s["t"] - t_base, 3), **{k: v for k, v in agg(s).items()}} for s in samples]


def ps_crosscheck(sample):
    """Compare libproc numbers with `ps` for the same pids (validates the accounting)."""
    pids = [p["pid"] for p in sample["procs"]]
    out = subprocess.run(["/bin/ps", "-o", "pid=,rss=,time=", "-p", ",".join(map(str, pids))],
                         capture_output=True, text=True).stdout
    ps = {}
    for line in out.splitlines():
        pid, rss, tm = line.split()
        m, s = tm.split(":")
        ps[int(pid)] = (int(rss) * 1024, int(m) * 60 + float(s))
    rows = [(p, ps[p["pid"]]) for p in sample["procs"] if p["pid"] in ps]
    return {"n_pids": len(rows),
            "rss_sum_ps": sum(v[0] for _, v in rows), "rss_sum_libproc": sum(p["rss"] for p, _ in rows),
            "cpu_own_sum_ps": sum(v[1] for _, v in rows), "cpu_own_sum_libproc": sum(p["cpu"] for p, _ in rows),
            "max_abs_cpu_diff_s": max((abs(p["cpu"] - v[1]) for p, v in rows), default=0.0)}


def footprint_check(sample):
    """`footprint -p` vs libproc ri_phys_footprint for the non-MCP processes (run once, slow)."""
    res = []
    for p in sample["procs"]:
        if p["role"] == "other":
            continue
        r = subprocess.run(["/usr/bin/footprint", "-p", str(p["pid"])], capture_output=True, text=True).stdout
        m = re.search(r"Footprint:\s*([\d.]+)\s*(KB|MB|GB)", r)
        if m:
            mult = {"KB": 1024, "MB": 1024 ** 2, "GB": 1024 ** 3}[m.group(2)]
            res.append({"role": p["role"], "footprint_cli_bytes": float(m.group(1)) * mult, "libproc_bytes": p["fp"]})
    return res


def system_context():
    top = subprocess.run("/bin/ps -Aro pcpu=,comm= | head -6", shell=True, capture_output=True, text=True).stdout
    mp = subprocess.run(["/usr/bin/memory_pressure", "-Q"], capture_output=True, text=True).stdout
    m = re.search(r"free percentage:\s*(\d+)%", mp)
    return {"loadavg": os.getloadavg(), "mem_free_pct": int(m.group(1)) if m else None,
            "top_cpu_procs": [" ".join([l.split(None, 1)[0], os.path.basename(l.split(None, 1)[1].strip())])
                              for l in top.splitlines() if l.strip()][:5]}


def tree_listing(sample):
    return [{"pid": p["pid"], "ppid": p["ppid"], "role": p["role"], "exe": p["exe"], "rss": p["rss"],
             "fp": p["fp"], "threads": p["threads"], "cpu_s": round(p["cpu"], 3)} for p in sample["procs"]]


# ---------------------------------------------------------------- scenarios
def wait_ready(sess, timeout=90):
    tool = sess.tool
    t_paint = None
    deadline = time.monotonic() + timeout
    pane = ""
    while time.monotonic() < deadline:
        pane = tx.capture(sess.name)
        now = time.monotonic()
        if t_paint is None and first_paint(tool, pane):
            t_paint = now
        if "Quick safety check" in pane:
            return None, t_paint, "trust_dialog"
        if is_ready(tool, pane):
            return now, t_paint, "ok"
        time.sleep(0.02)
    return None, t_paint, "timeout"


def start_and_ready(tool, a, label, resume_id=None):
    sess = Session(tool, a, label, resume_id).start()
    t_ready, t_paint, status = wait_ready(sess)
    r = {"status": status}
    if t_ready is None:
        r["error_pane_tail"] = [l for l in tx.capture(sess.name).splitlines() if l.strip()][-6:]
        return sess, r
    snap = sess.sampler.snapshot()
    r.update(ready_ms=(t_ready - sess.t_spawn) * 1000,
             first_paint_ms=(t_paint - sess.t_spawn) * 1000 if t_paint else None,
             startup_cpu_s=procs.tree_cpu(snap),
             startup_cpu_by_role={k: v["cpu_incl"] for k, v in agg(snap).items()},
             t_ready=t_ready)
    return sess, r


def scenario_main(tool, a, run_idx, first_run):
    fresh_scratch()
    sess, res = start_and_ready(tool, a, "main")
    res.update(scenario="main", tool=tool, run=run_idx, ctx=system_context(), root_pid=sess.root)
    try:
        if res["status"] != "ok":
            return res
        t_ready = res.pop("t_ready")
        # ---- idle: grace, then IDLE_S of 1 Hz sampling
        time.sleep(GRACE_S)
        g = window_stats(sess.sampler.samples, t_ready, time.monotonic())
        res["post_ready_grace_cpu_s"] = g["total"]["cpu_s"] if g else None
        tq, cq = wait_quiet(sess, t_ready, 60)            # earliest quiet moment after ready
        res["settle_after_start_ms"] = (tq - sess.t_spawn) * 1000 if tq else None
        res["cpu_to_settle_after_start_s"] = cq
        wait_quiet(sess, time.monotonic(), 60)            # make sure idle sampling starts on a quiet tree
        sess.sampler.interval = 1.0
        t0 = time.monotonic()
        time.sleep(IDLE_S)
        sess.sampler.snapshot()
        t1 = time.monotonic()
        res["idle"] = window_stats(sess.sampler.samples, t0, t1)
        last = sess.sampler.last()
        res["tree_at_idle"] = tree_listing(last)
        res["ps_crosscheck"] = ps_crosscheck(last)
        if first_run:
            res["footprint_cli_check"] = footprint_check(last)
        # ---- streaming turn
        sess.sampler.interval = 0.25
        time.sleep(0.6)
        res["stream"] = stream_turn(sess)
        res["timeline"] = timeline(sess.sampler.samples, sess.t_spawn)
    finally:
        res["survivors"] = sess.finish()
    return res


def stream_turn(sess):
    samples = sess.sampler.samples
    tx.send_literal(sess.name, PROMPT)
    time.sleep(0.15)
    snap0 = sess.sampler.snapshot()
    t0 = time.monotonic()
    tx.send_keys(sess.name, "Enter")
    last_hash, t_change, t_first, seen = None, t0, None, False
    done_re = re.compile(r"(?m)^[\s⏺●│]*400\s*│?\s*$")
    first_re = re.compile(r"(?m)^[\s⏺●│]*\d{1,3}\s*│?\s*$")   # first bare-integer output line
    timeout = t0 + 150
    while time.monotonic() < timeout:
        pane = tx.capture(sess.name)
        now = time.monotonic()
        body = "\n".join(pane.splitlines()[:-6])          # status/footer lines tick (clock, ctx %)
        h = hashlib.md5(body.encode()).hexdigest()
        if h != last_hash:
            last_hash, t_change = h, now
        if t_first is None and first_re.search(body):
            t_first = now
        seen = seen or bool(done_re.search(body))
        if seen and now - t_change >= SETTLE_S:
            break
        time.sleep(0.05)
    else:
        return {"status": "timeout", "pane_tail": [l for l in pane.splitlines() if l.strip()][-8:]}
    t_detect = time.monotonic()
    time.sleep(0.6)
    snap1 = sess.sampler.snapshot()
    allsamp = samples[:]
    cpu0 = procs.tree_cpu(snap0)
    win = [s for s in allsamp if t0 <= s["t"] <= t_detect]
    peaks = {}
    for s in win:
        for k, v in agg(s).items():
            pk = peaks.setdefault(k, {"rss_max": 0, "fp_max": 0})
            pk["rss_max"], pk["fp_max"] = max(pk["rss_max"], v["rss"]), max(pk["fp_max"], v["fp"])
    by_role = {}
    for k in agg(snap1):
        c0 = agg(snap0).get(k, {}).get("cpu_incl", 0.0)
        by_role[k] = {"cpu_to_complete_s": cpu_at(allsamp, t_change, k) - c0,
                      "cpu_to_detect_s": cpu_at(allsamp, t_detect, k) - c0}
    return {"status": "ok", "wall_to_complete_s": t_change - t0, "wall_to_detect_s": t_detect - t0,
            "time_to_first_line_s": (t_first - t0) if t_first else None, "settle_window_s": SETTLE_S,
            "cpu_to_complete_s": cpu_at(allsamp, t_change) - cpu0, "cpu_to_detect_s": cpu_at(allsamp, t_detect) - cpu0,
            "by_role": by_role, "peaks": peaks, "sampler_interval_s": 0.25, "n_samples": len(win)}


def scenario_typing(tool, a, run_idx, first_run):
    fresh_scratch()
    sess, res = start_and_ready(tool, a, "typing")
    res.update(scenario="typing", tool=tool, run=run_idx, ctx=system_context(), root_pid=sess.root)
    try:
        if res["status"] != "ok":
            return res
        res.pop("t_ready")
        time.sleep(GRACE_S)
        wait_quiet(sess, time.monotonic(), 60)
        sess.sampler.interval = 0.1
        time.sleep(0.4)
        snap0 = sess.sampler.snapshot()
        t0 = time.monotonic()
        tx.send_literal(sess.name, TYPING)
        t_vis, pane = tx.poll_until(sess.name, lambda p: TYPING_EXPECT in tx.strip_ui(p), 30, 0.01)
        if t_vis is None:
            res["typing"] = {"status": "not_visible", "pane_tail": [l for l in pane.splitlines() if l.strip()][-6:]}
            return res
        time.sleep(2.0)
        snap1 = sess.sampler.snapshot()
        samples = sess.sampler.samples[:]
        c0 = procs.tree_cpu(snap0)
        res["typing"] = {"status": "ok", "chars": len(TYPING), "time_to_visible_ms": (t_vis - t0) * 1000,
                         "cpu_to_visible_s": cpu_at(samples, t_vis) - c0,
                         "cpu_to_visible_plus2s_s": procs.tree_cpu(snap1) - c0,
                         "by_role": {k: {"cpu_to_visible_s": cpu_at(samples, t_vis, k) - agg(snap0).get(k, {}).get("cpu_incl", 0),
                                         "cpu_to_visible_plus2s_s": v["cpu_incl"] - agg(snap0).get(k, {}).get("cpu_incl", 0)}
                                     for k, v in agg(snap1).items()},
                         "sampler_interval_s": 0.1}
    finally:
        res["survivors"] = sess.finish()
    return res


def pick_session(a):
    if a.session_file:
        return a.session_file
    root = os.path.join(CONFIG_DIR, "projects")
    cands = []
    for d in os.listdir(root):
        dp = os.path.join(root, d)
        if d == "-private-tmp-ccbench-cwd" or not os.path.isdir(dp):
            continue
        for f in os.listdir(dp):
            if f.endswith(".jsonl") and re.fullmatch(r"[0-9a-f-]{36}\.jsonl", f):
                p = os.path.join(dp, f)
                st = os.stat(p)
                if 2e6 <= st.st_size <= 6e6 and time.time() - st.st_mtime > 6 * 3600:
                    cands.append((abs(st.st_size - 4e6), p))
    # worktree sessions carry worktree-state records that pin the session to its worktree: skip them
    for _, p in sorted(cands):
        if "worktrees" not in p and b'"worktree-state"' not in open(p, "rb").read():
            return p
    raise SystemExit("no idle, non-worktree 2-6 MB session found; pass --session-file")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def relocate(data):
    """Point every recorded "cwd" at the scratch dir. claude-rs only resumes a session whose recorded cwd
    matches the launch cwd (a copy still pointing at the original project is 'not found'). Same rewrite for both tools."""
    cwds = set()
    for line in data.splitlines():
        try:
            c = json.loads(line).get("cwd")
        except ValueError:
            continue
        if isinstance(c, str):
            cwds.add(c)
    for c in cwds:
        data = data.replace(b'"cwd":"' + json.dumps(c)[1:-1].encode() + b'"', b'"cwd":"' + SCRATCH.encode() + b'"')
    return data


def scenario_resume(tool, a, run_idx, first_run):
    src = pick_session(a)
    src_sha, src_stat = sha256(src), os.stat(src)
    old_id = os.path.basename(src)[:-6]
    # Fresh copy under a NEW uuid of identical length (byte-for-byte replace, size unchanged). The brief asked
    # for the same uuid, but claude-rs resolves a bare session id across ALL projects and appended metadata
    # lines to the ORIGINAL file (measured 2026-10-22). A uuid that exists only in the scratch dir can never reach it.
    sid = str(uuid.uuid4())
    data = relocate(open(src, "rb").read().replace(old_id.encode(), sid.encode()))
    fresh_scratch()
    clean_project_dir()
    os.makedirs(PROJECT_DIR, exist_ok=True)
    with open(os.path.join(PROJECT_DIR, sid + ".jsonl"), "wb") as f:
        f.write(data)
    size = len(data)
    sess = Session(tool, a, "resume", sid).start()
    res = {"scenario": "resume", "tool": tool, "run": run_idx, "ctx": system_context(), "root_pid": sess.root,
           "transcript_bytes": size}
    try:
        t_ready, t_paint, status = wait_ready(sess, 120)
        res["status"] = status
        if t_ready is None:
            res["error_pane_tail"] = [l for l in tx.capture(sess.name).splitlines() if l.strip()][-8:]
            return res
        res["ready_ms"] = (t_ready - sess.t_spawn) * 1000
        t_q, cpu_q = wait_quiet(sess, t_ready, 120)
        if t_q is None:
            res["status"] = "never_settled"
            return res
        res["settle_ms"] = (t_q - sess.t_spawn) * 1000
        res["cpu_to_settle_s"] = cpu_q
        # let the settle window elapse, then take memory after load
        snap = sess.sampler.snapshot()
        res["after_load"] = {k: {"rss": v["rss"], "fp": v["fp"], "nprocs": v["nprocs"], "threads": v["threads"]}
                             for k, v in agg(snap).items()}
        res["tree_after_load"] = tree_listing(snap)
        # ---- scroll: 20 x PageUp
        before = tx.capture(sess.name)
        snap0 = sess.sampler.snapshot()
        t0 = time.monotonic()
        tx.send_keys(sess.name, *(["PPage"] * 20))
        time.sleep(SCROLL_WINDOW_S)
        sess.sampler.snapshot()
        tq, cq = wait_quiet(sess, t0, 60)
        after = tx.capture(sess.name)
        c0 = procs.tree_cpu(snap0)
        samples = sess.sampler.samples[:]
        res["scroll"] = {"status": "ok" if tq else "never_settled", "keys": 20,
                         "pane_changed": before != after,
                         "cpu_s": cpu_at(samples, t0 + SCROLL_WINDOW_S) - c0,
                         "cpu_by_role_s": {k: cpu_at(samples, t0 + SCROLL_WINDOW_S, k) - agg(snap0).get(k, {}).get("cpu_incl", 0)
                                           for k in agg(snap0)},
                         "wall_to_settle_s": (tq - t0) if tq else None}
        res["timeline"] = timeline(sess.sampler.samples, sess.t_spawn)
        m = re.search(r"(\d+)% of (\d+)k", tx.capture(sess.name))
        res["ctx_meter_hint"] = m.group(0) if m else None     # proves a transcript was loaded, shows no content
    finally:
        res["survivors"] = sess.finish()
        clean_project_dir()
        res["original_untouched"] = (sha256(src) == src_sha and os.stat(src).st_mtime == src_stat.st_mtime)
        if not res["original_untouched"]:
            raise SystemExit(f"ABORT: {tool} modified the original transcript; stopping the harness")
    return res


SCENARIOS = {"main": scenario_main, "typing": scenario_typing, "resume": scenario_resume}


# ---------------------------------------------------------------- setup: trust + warmup
def trust_setup(a):
    """Warm-up launch of each tool; accepts the one-time trust dialog for the fixed scratch path.
    Same procedure for both tools; trust lives in the config dir's .claude.json (keyed by path)."""
    for tool in ("claude", "claude-rs"):
        fresh_scratch()
        sess = Session(tool, a, "warmup").start()
        try:
            end = time.monotonic() + 90
            while time.monotonic() < end:
                pane = tx.capture(sess.name)
                if "Quick safety check" in pane:
                    log(f"{tool}: trust dialog -> accepting 'Yes, I trust this folder'")
                    tx.send_keys(sess.name, "Down")
                    time.sleep(0.3)
                    tx.send_keys(sess.name, "Enter")
                    time.sleep(1.0)
                    continue
                if is_ready(tool, pane):
                    log(f"{tool}: warm-up ready")
                    break
                time.sleep(0.2)
            else:
                raise SystemExit(f"{tool}: warm-up never reached the prompt")
            time.sleep(2)
        finally:
            sv = sess.finish()
            if sv:
                log("WARNING survivors after warmup:", sv)
    clean_all()


# ---------------------------------------------------------------- static facts
def sh(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    return (r.stdout + r.stderr).strip()


def du(path):
    return int(sh(["/usr/bin/du", "-sk", path]).split()[0]) * 1024 if os.path.exists(path) else None


def static_facts(a):
    rs_real = os.path.realpath(a.claude_rs)
    bundle = os.path.dirname(rs_real)
    cc_real = os.path.realpath(a.claude)
    sdkdir = os.path.join(bundle, "node_modules", "@anthropic-ai")
    sdk_pkg = json.load(open(os.path.join(sdkdir, "claude-agent-sdk", "package.json")))
    sdk_engine = os.path.join(sdkdir, "claude-agent-sdk-darwin-arm64", "claude")
    engine_used = a.engine_path or sdk_engine
    inode = lambda p: os.stat(p).st_ino if os.path.exists(p) else None
    size = lambda p: os.path.getsize(p) if os.path.exists(p) else None
    bun = os.path.join(bundle, "claude-rs-bridge-bun")
    facts = {
        "machine": {"model": sh(["sysctl", "-n", "hw.model"]), "cpu": sh(["sysctl", "-n", "machdep.cpu.brand_string"]),
                    "mem_bytes": int(sh(["sysctl", "-n", "hw.memsize"])), "ncpu": int(sh(["sysctl", "-n", "hw.ncpu"])),
                    "macos": sh(["sw_vers", "-productVersion"]), "kernel": sh(["uname", "-r"])},
        "versions": {"claude_rs": sh([a.claude_rs, "--version"]), "claude": sh([a.claude, "--version"]),
                     "sdk": sdk_pkg.get("version"), "sdk_bundled_engine_version": sdk_pkg.get("claudeCodeVersion"),
                     "sdk_bundled_engine_binary": sh([sdk_engine, "--version"]) if os.path.exists(sdk_engine) else None,
                     "engine_used_by_claude_rs": engine_used,
                     "engine_used_version": sh([engine_used, "--version"]),
                     "engine_same_inode_as_official": inode(engine_used) == inode(cc_real)},
        "paths": {"claude_rs_real": rs_real, "claude_real": cc_real, "bundle_dir": bundle},
        "disk_bytes": {"claude_rs_ui_binary": size(rs_real), "bridge_bun_bundle": size(bun),
                       "agent_sdk_js": du(os.path.join(bundle, "agent-sdk")),
                       "node_modules_total": du(os.path.join(bundle, "node_modules")),
                       "sdk_bundled_engine_binary": size(sdk_engine),
                       "claude_rs_bundle_dir_total": du(bundle),
                       "engine_used_binary": size(engine_used),
                       "official_claude_binary": size(cc_real)},
    }
    return facts


# ---------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--claude-rs", required=True)
    ap.add_argument("--claude", required=True)
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--out", required=True)
    ap.add_argument("--engine", default="official", help="engine for claude-rs: 'official' (same binary as --claude), 'bundled' (SDK's own), or a path")
    ap.add_argument("--mcp", choices=["user", "none"], default="user")
    ap.add_argument("--scenarios", default="main,typing,resume")
    ap.add_argument("--session-file")
    ap.add_argument("--skip-setup", action="store_true")
    ap.add_argument("--idle-secs", type=float, default=30.0)
    a = ap.parse_args()
    global IDLE_S
    IDLE_S = a.idle_secs
    a.claude_rs, a.claude = os.path.abspath(a.claude_rs), os.path.abspath(a.claude)
    a.engine_path = {"official": os.path.realpath(a.claude), "bundled": None}.get(a.engine, a.engine)
    os.makedirs(os.path.join(a.out, "raw"), exist_ok=True)
    facts = static_facts(a)
    facts["run_config"] = {"runs": a.runs, "mcp": a.mcp, "engine": a.engine, "scenarios": a.scenarios,
                           "idle_secs": IDLE_S, "grace_s": GRACE_S, "prompt": PROMPT, "typing_chars": len(TYPING),
                           "env_whitelist": sorted(base_env()), "tmux": "private server -L ccbench, 120x40",
                           "started": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    if a.session_file is None and "resume" in a.scenarios:
        a.session_file = pick_session(a)
    if a.session_file:
        facts["resume_transcript"] = {"bytes": os.path.getsize(a.session_file)}
    json.dump(facts, open(os.path.join(a.out, "machine.json"), "w"), indent=2)
    if not a.skip_setup:
        trust_setup(a)
    scen = [s for s in a.scenarios.split(",") if s]
    try:
        for i in range(1, a.runs + 1):
            for sc in scen:
                order = ["claude-rs", "claude"] if i % 2 else ["claude", "claude-rs"]
                for tool in order:
                    time.sleep(3)
                    log(f"run {i}/{a.runs} {sc} {tool} load={os.getloadavg()[0]:.2f}")
                    try:
                        res = SCENARIOS[sc](tool, a, i, i == 1)
                    except Exception as e:                      # keep the harness going, record the failure
                        res = {"scenario": sc, "tool": tool, "run": i, "status": f"harness_error: {e!r}"}
                    with open(os.path.join(a.out, "raw", f"{sc}_{tool}_{i}.json"), "w") as f:
                        json.dump(res, f, indent=1, default=str)
                    if res.get("survivors"):
                        log("WARNING survivors", res["survivors"])
    finally:
        clean_all()
        tx.kill_server_if_empty()


if __name__ == "__main__":
    main()
