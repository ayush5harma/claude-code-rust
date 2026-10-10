# shellcheck shell=bash
# shellcheck disable=SC2154,SC2034 # harness globals (SCREEN, REG, WORK, ...) are shared with run.sh
# The session registry: an interactive claude-rs session appears in
# `claude agents --json` with its name, status and pid, as stock's does. The
# agent view itself (the footer's `← N agents` hint, Left on an empty prompt
# opening `claude agents`) is no longer part of claude-rs; those two stock
# behaviours are recorded as gap.agents_hint and gap.agent_view.

def agents.registry agents "Session in \`claude agents --json\`" "Interactive entry with name and status"
chk_agents_registry() {
  ensure_main || { res FAIL "main session did not start"; return; }
  REG=$(registry_entry "$(cwd_of main)")
  if [[ -n $REG ]] && jq -e '.name and .status and .pid' <<<"$REG" >/dev/null; then
    res PASS "$(jq -c '{kind, name, status, pid}' <<<"$REG")"
  else
    res FAIL "no interactive registry entry for the main cwd"
  fi
}
