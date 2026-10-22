# SPDX-License-Identifier: Apache-2.0
"""Turn raw/*.json into summary.md (README-ready tables). Run via run.sh (python3 -I)."""
import glob
import json
import os
import statistics
import sys

TOOLS = [("claude-rs", "claude-rs"), ("claude", "Claude Code")]
MIB, ROLE_ORDER = 1024 * 1024, ["ui", "bridge", "engine", "ui+engine", "other", "total"]


def load(out):
    runs = {}
    for f in sorted(glob.glob(os.path.join(out, "raw", "*.json"))):
        r = json.load(open(f))
        runs.setdefault((r.get("scenario"), r.get("tool")), []).append(r)
    return runs


def dig(d, path):
    for p in path:
        if not isinstance(d, dict) or p not in d:
            return None
        d = d[p]
    return d


def vals(rs, path, scale=1.0):
    out = [dig(r, path) for r in rs if r.get("status") == "ok"]
    return [v / scale for v in out if isinstance(v, (int, float))]


def cell(v, nd):
    if not v:
        return "n/a"
    if len(v) == 1:
        return f"{v[0]:.{nd}f}"
    return f"{statistics.median(v):.{nd}f} ({min(v):.{nd}f}-{max(v):.{nd}f})"


def row(label, runs, scen, path, scale=1.0, nd=1, unit=""):
    a = vals(runs.get((scen, "claude-rs"), []), path, scale)
    b = vals(runs.get((scen, "claude"), []), path, scale)
    ratio = f"{statistics.median(a) / statistics.median(b):.2f}x" if a and b and statistics.median(b) else "n/a"
    return f"| {label}{' (' + unit + ')' if unit else ''} | {cell(a, nd)} | {cell(b, nd)} | {ratio} |", \
        (statistics.median(a) / statistics.median(b) if a and b and statistics.median(b) else None)


def table(title, spec, runs, scen, losses=None):
    head = [f"**{title}**", "", "| Metric | claude-rs | Claude Code | claude-rs / Claude Code |", "|---|---|---|---|"]
    for label, path, scale, nd, unit in spec:
        line, ratio = row(label, runs, scen, path, scale, nd, unit)
        head.append(line)
        if losses is not None and ratio is not None and ratio > 1.05:
            losses.append(f"{title.split(' (')[0]} / {label}: claude-rs {ratio:.2f}x")
    return head + [""]


def roles_present(runs, scen, root_path):
    seen = set()
    for t, _ in TOOLS:
        for r in runs.get((scen, t), []):
            d = dig(r, root_path)
            if isinstance(d, dict):
                seen.update(d.keys())
    return [r for r in ROLE_ORDER if r in seen]


def role_table(title, runs, scen, root, metrics, nd=1):
    """root: path to dict keyed by role; metrics: [(label, subpath, scale, unit)]"""
    lines = [f"**{title}**", "", "| Process | Metric | claude-rs | Claude Code |", "|---|---|---|---|"]
    for role in roles_present(runs, scen, root):
        for label, sub, scale, unit in metrics:
            a = vals(runs.get((scen, "claude-rs"), []), root + [role] + sub, scale)
            b = vals(runs.get((scen, "claude"), []), root + [role] + sub, scale)
            lines.append(f"| {role} | {label} ({unit}) | {cell(a, nd)} | {cell(b, nd)} |")
    return lines + [""]


def human(n):
    return "n/a" if n is None else f"{n / MIB:.1f} MiB"


def tree_block(runs):
    out = ["### Validated process trees (idle, first run)", ""]
    for t, name in TOOLS:
        rs = [r for r in runs.get(("main", t), []) if r.get("tree_at_idle")]
        if not rs:
            continue
        tree = rs[0]["tree_at_idle"]
        by = {p["pid"]: p for p in tree}
        kids = {}
        for p in tree:
            kids.setdefault(p["ppid"], []).append(p)
        out += [f"`{name}`", "", "```", "pid    ppid   role       rss(MiB) fp(MiB) thr  command"]

        def walk(pid, depth):
            for p in sorted(kids.get(pid, []), key=lambda x: x["pid"]):
                out.append(f'{p["pid"]:<6} {p["ppid"]:<6} {p["role"]:<10} {p["rss"]/MIB:8.1f} {p["fp"]/MIB:7.1f} {p["threads"] or 0:<4} '
                           f'{"  " * depth}{os.path.basename(p["exe"])}')
                walk(p["pid"], depth + 1)
        roots = [p for p in tree if p["ppid"] not in by]
        for r in roots:
            out.append(f'{r["pid"]:<6} {r["ppid"]:<6} {r["role"]:<10} {r["rss"]/MIB:8.1f} {r["fp"]/MIB:7.1f} {r["threads"] or 0:<4} {os.path.basename(r["exe"])}')
            walk(r["pid"], 1)
        out += ["```", ""]
    return out


def main(out):
    runs = load(out)
    facts = json.load(open(os.path.join(out, "machine.json")))
    m, v, rc, dk = facts["machine"], facts["versions"], facts["run_config"], facts["disk_bytes"]
    L = ["# claude-rs vs Claude Code: benchmark summary", ""]
    n_runs = {k: len(r) for k, r in runs.items()}
    L += [f"Generated from `{os.path.basename(os.path.abspath(out))}`. Statistic: **median (min-max)** over N runs per cell "
          f"(N={rc['runs']} requested). `claude-rs / Claude Code` is the ratio of medians; below 1.0 means claude-rs used less.", ""]
    loads = [r["ctx"]["loadavg"][0] for rs in runs.values() for r in rs if r.get("ctx")]
    if loads:
        L += [f"> Load average (1 min) measured before each run: min {min(loads):.2f}, median {statistics.median(loads):.2f}, "
              f"max {max(loads):.2f} over {len(loads)} runs. CPU-time metrics are less sensitive to load than wall-clock ones, "
              f"but neither is immune (cache and scheduler contention).", ""]
    L += ["## Machine and versions", "",
          f"- {m['cpu']} ({m['model']}), {m['ncpu']} cores, {m['mem_bytes'] / 2**30:.0f} GiB, macOS {m['macos']} (Darwin {m['kernel']})",
          f"- claude-rs: `{v['claude_rs']}`; official: `{v['claude']}`",
          f"- Agent SDK under claude-rs: {v['sdk']} (its bundled engine: Claude Code {v['sdk_bundled_engine_version']}; binary reports `{v['sdk_bundled_engine_binary']}`)",
          f"- Engine actually used by claude-rs in this run: `{v['engine_used_by_claude_rs']}` reporting `{v['engine_used_version']}`; "
          f"same inode as the official binary: **{v['engine_same_inode_as_official']}** (mode: `--engine {rc['engine']}`)",
          f"- MCP servers: `{rc['mcp']}` (`user` = the MCP servers configured under CLAUDE_CONFIG_DIR, spawned identically by both engines; `none` = `--strict-mcp-config` with an empty set)",
          f"- Model `--model haiku` for both, terminal tmux 120x40, prompt: `{rc['prompt']}`", ""]
    if "resume_transcript" in facts:
        L += [f"- Resume transcript size: {facts['resume_transcript']['bytes'] / 1e6:.2f} MB (contents never printed)", ""]
    losses = []
    if ("main", "claude-rs") in runs or ("main", "claude") in runs:
        L += ["## 1. Cold start (spawn to prompt ready)", ""]
        L += table("Cold start (whole tree)", [
            ("Time to ready marker", ["ready_ms"], 1, 0, "ms"),
            ("Time to first paint of composer", ["first_paint_ms"], 1, 0, "ms"),
            ("Tree CPU from spawn to ready", ["startup_cpu_s"], 1, 2, "s"),
            ("Time until tree CPU settles (<3% of a core over 2 s)", ["settle_after_start_ms"], 1, 0, "ms"),
            ("Tree CPU from spawn to settle", ["cpu_to_settle_after_start_s"], 1, 2, "s")], runs, "main", losses)
        rows = ["**Cold start, CPU by process (spawn to ready)**", "", "| Process | claude-rs (s) | Claude Code (s) |", "|---|---|---|"]
        for role in roles_present(runs, "main", ["startup_cpu_by_role"]):
            a = vals(runs.get(("main", "claude-rs"), []), ["startup_cpu_by_role", role])
            b = vals(runs.get(("main", "claude"), []), ["startup_cpu_by_role", role])
            rows.append(f"| {role} | {cell(a, 2)} | {cell(b, 2)} |")
        L += rows + [""]
        L += ["Ready marker: claude-rs = placeholder `Type a message` AND the engine connected (a real `Session ID` on the welcome, or the context field in the footer); Claude Code = the `❯` input box "
              "(drawn only after init). `first paint` for claude-rs is the placeholder alone, which appears before the engine is connected.", ""]
        L += [f"## 2. Idle ({rc['idle_secs']:.0f} s, sampled every 1 s, starting {rc['grace_s']:.0f} s after ready)", ""]
        L += table("Idle (whole tree)", [
            ("CPU time consumed", ["idle", "total", "cpu_s"], 1, 3, "s"),
            ("Average CPU", ["idle", "total", "cpu_pct_of_core"], 1, 2, "% of one core"),
            ("RSS sum (mean)", ["idle", "total", "rss_mean"], MIB, 0, "MiB"),
            ("Physical footprint sum (mean)", ["idle", "total", "fp_mean"], MIB, 0, "MiB"),
            ("Processes (mean)", ["idle", "total", "nprocs_mean"], 1, 1, "count"),
            ("Threads (mean)", ["idle", "total", "threads_mean"], 1, 0, "count"),
            ("CPU in the 5 s grace window after ready", ["post_ready_grace_cpu_s"], 1, 2, "s")], runs, "main", losses)
        L += role_table("Idle, per process", runs, "main", ["idle"], [
            ("CPU %", ["cpu_pct_of_core"], 1, "% core"), ("RSS", ["rss_mean"], MIB, "MiB"),
            ("Footprint", ["fp_mean"], MIB, "MiB"), ("Threads", ["threads_mean"], 1, "count")], 2)
        L += ["## 3. Streaming turn (400 lines)", ""]
        L += table("Streaming turn (whole tree)", [
            ("Wall time, Enter to last pane change", ["stream", "wall_to_complete_s"], 1, 2, "s"),
            ("Time to first output line", ["stream", "time_to_first_line_s"], 1, 2, "s"),
            ("CPU, Enter to last pane change", ["stream", "cpu_to_complete_s"], 1, 2, "s"),
            ("CPU, Enter to detection (incl. 3 s settle)", ["stream", "cpu_to_detect_s"], 1, 2, "s"),
            ("Peak RSS sum", ["stream", "peaks", "total", "rss_max"], MIB, 0, "MiB"),
            ("Peak footprint sum", ["stream", "peaks", "total", "fp_max"], MIB, 0, "MiB")], runs, "main", losses)
        L += role_table("Streaming turn, CPU to last pane change, per process", runs, "main", ["stream", "by_role"],
                        [("CPU", ["cpu_to_complete_s"], 1, "s")], 2)
        L += role_table("Streaming turn, peak footprint, per process", runs, "main", ["stream", "peaks"],
                        [("Peak footprint", ["fp_max"], MIB, "MiB"), ("Peak RSS", ["rss_max"], MIB, "MiB")], 0)
        L += ["Completion = a line `400` seen, then pane (excluding the bottom 6 status rows) unchanged for 3 s. "
              "Wall and CPU are measured to the *last pane change*, i.e. the 3 s settle window is excluded; the `to detection` row includes it. "
              "Peaks come from 4 Hz sampling and can miss short spikes.", ""]
    if ("resume", "claude-rs") in runs or ("resume", "claude") in runs:
        L += ["## 4. Long transcript resume and scroll", ""]
        L += table("Resume (whole tree)", [
            ("Time to ready marker", ["ready_ms"], 1, 0, "ms"),
            ("Time until tree CPU settles (<3% of a core over 2 s)", ["settle_ms"], 1, 0, "ms"),
            ("Tree CPU from spawn to settle", ["cpu_to_settle_s"], 1, 2, "s"),
            ("RSS sum after load", ["after_load", "total", "rss"], MIB, 0, "MiB"),
            ("Physical footprint sum after load", ["after_load", "total", "fp"], MIB, 0, "MiB"),
            ("20x PageUp: CPU", ["scroll", "cpu_s"], 1, 3, "s"),
            ("20x PageUp: wall to CPU settle", ["scroll", "wall_to_settle_s"], 1, 2, "s")], runs, "resume", losses)
        L += role_table("After resume load, per process", runs, "resume", ["after_load"],
                        [("RSS", ["rss"], MIB, "MiB"), ("Footprint", ["fp"], MIB, "MiB"), ("Threads", ["threads"], 1, "count")], 0)
        pc = {t: [r.get("scroll", {}).get("pane_changed") for r in runs.get(("resume", t), [])] for t, _ in TOOLS}
        L += [f"PageUp changed the visible pane: claude-rs {pc['claude-rs']}, Claude Code {pc['claude']} "
              "(if `False`, the scroll CPU number measures a no-op key press and is not a rendering cost).", ""]
    if ("typing", "claude-rs") in runs or ("typing", "claude") in runs:
        L += ["## 5. Typing burst (300 chars via `tmux send-keys -l`, not submitted)", ""]
        L += table("Typing burst (whole tree)", [
            ("Time until all 300 chars visible", ["typing", "time_to_visible_ms"], 1, 0, "ms"),
            ("CPU until visible", ["typing", "cpu_to_visible_s"], 1, 3, "s"),
            ("CPU until visible + 2 s", ["typing", "cpu_to_visible_plus2s_s"], 1, 3, "s")], runs, "typing", losses)
        L += role_table("Typing burst, CPU until visible + 2 s, per process", runs, "typing", ["typing", "by_role"],
                        [("CPU", ["cpu_to_visible_plus2s_s"], 1, "s")], 3)
    L += ["## 6. Footprint on disk, and idle process/thread counts", "",
          "| Item | Size |", "|---|---|",
          f"| claude-rs UI binary (Rust) | {human(dk['claude_rs_ui_binary'])} |",
          f"| bridge (bundled Bun, `claude-rs-bridge-bun`) | {human(dk['bridge_bun_bundle'])} |",
          f"| agent-sdk JS | {human(dk['agent_sdk_js'])} |",
          f"| node_modules (incl. SDK bundled engine) | {human(dk['node_modules_total'])} |",
          f"| SDK bundled engine binary | {human(dk['sdk_bundled_engine_binary'])} |",
          f"| **claude-rs install dir total** (`du`) | {human(dk['claude_rs_bundle_dir_total'])} |",
          f"| engine binary used in this run | {human(dk['engine_used_binary'])} |",
          f"| official Claude Code binary | {human(dk['official_claude_binary'])} |", "",
          "claude-rs needs an engine in addition to its own bundle: either the SDK-bundled one (already inside the install dir total above) "
          "or the official binary (hard-linked or shared with an existing install, so it may cost no extra disk).", ""]
    L += tree_block(runs)
    cc = [r.get("ps_crosscheck") for rs in runs.values() for r in rs if r.get("ps_crosscheck")]
    if cc:
        L += ["### Accounting validation", "",
              "| Tool | pids | RSS sum libproc vs ps | own-CPU sum libproc vs ps (s) | max per-pid CPU diff (s) |", "|---|---|---|---|---|"]
        for t, name in TOOLS:
            for r in runs.get(("main", t), [])[:1]:
                c = r.get("ps_crosscheck")
                if c:
                    L.append(f"| {name} | {c['n_pids']} | {c['rss_sum_libproc'] / MIB:.1f} vs {c['rss_sum_ps'] / MIB:.1f} MiB | "
                             f"{c['cpu_own_sum_libproc']:.2f} vs {c['cpu_own_sum_ps']:.2f} | {c['max_abs_cpu_diff_s']:.3f} |")
        L += [""]
    fpc = [(t, x) for t, _ in TOOLS for r in runs.get(("main", t), []) for x in r.get("footprint_cli_check", [])]
    if fpc:
        L += ["`footprint -p` (the CLI) vs libproc `ri_phys_footprint`, taken at nearly the same moment:", ""]
        for t, x in fpc:
            L.append(f"- {t} {x['role']}: CLI {x['footprint_cli_bytes'] / MIB:.0f} MiB vs libproc {x['libproc_bytes'] / MIB:.0f} MiB")
        L += [""]
    bad = [(k, r.get("run"), r.get("status"), r.get("stream", {}).get("status"), r.get("typing", {}).get("status"))
           for k, rs in runs.items() for r in rs
           if r.get("status") != "ok" or r.get("stream", {}).get("status", "ok") != "ok" or r.get("typing", {}).get("status", "ok") != "ok"]
    surv = [(k, r.get("run"), r["survivors"]) for k, rs in runs.items() for r in rs if r.get("survivors")]
    L += ["## Run health", "", f"- Runs per cell: " + ", ".join(f"{k[0]}/{k[1]}={n}" for k, n in sorted(n_runs.items())),
          f"- Failed or partial runs (excluded from medians): {bad if bad else 'none'}",
          f"- Processes surviving cleanup (recorded descendants only): {surv if surv else 'none'}", ""]
    if losses:
        L += ["## Where claude-rs is worse in this run (ratio > 1.05)", ""] + [f"- {x}" for x in losses] + [""]
    else:
        L += ["## Where claude-rs is worse in this run (ratio > 1.05)", "", "- none of the headline whole-tree metrics (lower is better for all of them)", ""]
    L += ["## Method notes and caveats", ""] + [f"- {x}" for x in NOTES] + [""]
    open(os.path.join(out, "summary.md"), "w").write("\n".join(L))


NOTES = [
    "Process tree = every descendant of the tmux pane's root pid, found with `ps -axo pid,ppid` and refreshed each sample. Roles: `ui` = claude-rs Rust process; "
    "`bridge` = `claude-rs-bridge-bun` child; `engine` = the `claude` binary under the bridge; `ui+engine` = the official `claude` root process (it is both); "
    "`other` = everything else, mostly the configured MCP servers and their `npm exec`/`node`/`uv` wrappers, started identically by both engines.",
    "CPU time comes from `proc_pid_rusage` (user+system, nanosecond resolution), not `ps` (10 ms). It also adds each live process's reaped-children CPU "
    "(`ri_child_*`), so short-lived hook/statusLine/tool children that start and exit between two samples are still counted, provided their parent was in the tree and waited on them. "
    "Processes that daemonise or are reparented to launchd escape the tree and are not counted.",
    "RSS is summed per process and double-counts pages shared between processes (dyld shared cache, shared mappings, two processes mapping the same binary). "
    "Physical footprint (`ri_phys_footprint`, what Activity Monitor calls Memory) is the better number; it includes compressed memory and excludes clean file-backed pages. "
    "Neither includes the tmux server or the terminal emulator.",
    "Every session is a fresh process in a fresh `git init` repo at `/tmp/ccbench-cwd` (path fixed because trust is stored per path). The one-time workspace-trust dialog "
    "is accepted in an unmeasured warm-up launch of each tool (same procedure for both); that writes a `projects[<path>]` entry to the config dir's `.claude.json`, which this harness cannot undo safely.",
    "Environment is whitelisted (`env -i` plus HOME, USER, PATH, LANG, TERM, TMPDIR, CLAUDE_CONFIG_DIR, DISABLE_AUTOUPDATER; claude-rs also gets CLAUDE_CODE_EXECUTABLE when an engine is chosen). "
    "Pass the real binaries, not shell aliases or launch wrappers, so the numbers describe the binaries themselves.",
    "The settings, hooks, statusLine, plugins and MCP servers under CLAUDE_CONFIG_DIR load for both tools in `--mcp user` mode and dominate the `other` role. `--mcp none` isolates the tools themselves.",
    "Warm-cache cold start: the OS file cache is not purged (that needs root), and each tool gets one unmeasured warm-up launch first. Absolute cold-disk start is slower for both.",
    "Runs alternate tool order each iteration. The machine is not otherwise quiet; the load average before each run is recorded in `raw/*.json` (`ctx`).",
    "Streaming CPU is interpolated linearly between 4 Hz samples; peak RSS/footprint can miss spikes shorter than 250 ms.",
    "Resume: before each run a fresh copy of one idle, non-worktree transcript is written into the scratch project dir under a NEW uuid of the same length, with every recorded `cwd` "
    "rewritten to the scratch dir (same copy for both tools). A same-uuid copy is not safe: claude-rs resolves a bare session id across all projects, and in testing it appended three "
    "metadata lines (isolation-latch, custom-title, cost-state) to the ORIGINAL transcript; it also refuses a copy whose recorded cwd differs from the launch cwd ('session not found'). "
    "The harness hashes the original before and after every run and aborts if it changed. 'Settled' = tree CPU below 3% of one core over a 2 s window after the ready marker, checked at 4 Hz.",
    "Teardown is SIGKILL of the recorded descendants (not a graceful exit) so SessionEnd hooks never fire on benchmark sessions. "
    "Consequence: exit-time behaviour (shutdown CPU, orphaned children after a clean quit) is not measured.",
    "Under memory pressure macOS compresses idle pages, which lowers RSS but not physical footprint; the free-memory percentage before each run is in `raw/*.json` (`ctx.mem_free_pct`). "
    "Compare footprint, not RSS, when it is low.",
    "Typing 'time until visible' is ~20-45 ms, close to the tmux poll/capture cost (~10-20 ms per capture), so ratios of those two numbers are noise; the CPU figures are the useful part.",
    "Ready markers are not equivalent: claude-rs shows its composer at once (first paint) but marks ready only when the engine is connected; Claude Code draws its prompt after its own init, while MCP servers may still be starting. "
    "The 'settles' rows (tree CPU quiet) are the like-for-like start-up comparison.",
    "Scroll: 20 `PPage` keys are sent in one tmux call. Whether the visible pane actually changed is reported; a tool that ignores PageUp at the prompt would show near-zero CPU.",
    "Typing: 'visible' = all 60 tokens (`w001`..`w060`, 300 chars) appear in order in the captured pane. Includes tmux capture latency (~10-20 ms poll). The text is never submitted.",
    "Each session also writes to the real config dir (history, session-env, debug logs); only the scratch project dir is removed by the harness.",
]

if __name__ == "__main__":
    main(sys.argv[1])
