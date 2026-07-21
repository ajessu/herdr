#!/bin/sh
# installed by herdr
# managed by herdr; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# HERDR_INTEGRATION_ID=claude
#
# Claude Code statusLine wrapper. Reports the active model to herdr and then
# chains the user's pre-existing statusLine command (recorded in
# HERDR_STATUSLINE_CHAIN by the installer). Reporting never affects the chained
# output, and the wrapper always emits a statusline so a user's bar is never
# blanked.

set -eu

# Report TTL (must match the --ttl-ms below). The heartbeat window is derived
# from it: re-report when the last confirmed report is older than TTL/3, so a
# stable model never expires mid-session. One source of truth (TTL_MS); do not
# hardcode the heartbeat seconds independently.
TTL_MS=900000
HEARTBEAT_SECS=$(( TTL_MS / 1000 / 3 ))

TAB="$(printf '\t')"

debug() {
  [ "${HERDR_STATUSLINE_DEBUG:-}" = "1" ] || return 0
  printf 'herdr-statusline: %s\n' "$*" >&2
}

is_uint() {
  case "${1:-}" in
    '' | *[!0-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

# Bound the report so a hung herdr can never stall a statusLine render. GNU
# coreutils `timeout` is absent on stock macOS (installed as `gtimeout`), so
# fall back to it, and finally to a POSIX watchdog (background the command, kill
# it after 1s) rather than an unbounded call — the herdr CLI does not apply its
# own timeout on this path, so a bare call could block indefinitely.
if command -v timeout >/dev/null 2>&1; then
  run_report() { timeout 1 "$@"; }
elif command -v gtimeout >/dev/null 2>&1; then
  run_report() { gtimeout 1 "$@"; }
else
  run_report() {
    "$@" &
    _rr_cmd=$!
    ( sleep 1; kill -TERM "$_rr_cmd" 2>/dev/null || true ) &
    _rr_dog=$!
    _rr_rc=0
    wait "$_rr_cmd" 2>/dev/null || _rr_rc=$?
    kill -TERM "$_rr_dog" 2>/dev/null || true
    wait "$_rr_dog" 2>/dev/null || true
    return "$_rr_rc"
  }
fi

# --- buffer stdin exactly once ------------------------------------------------
# The buffered JSON feeds both model extraction and the chained command; stdin
# is read only once.
input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-statusline.XXXXXX" 2>/dev/null || true)"
if [ -n "$input_file" ]; then
  trap 'rm -f "$input_file"' EXIT HUP INT TERM
  cat >"$input_file" 2>/dev/null || true
fi

# --- extract model.display_name (python3 heredoc; no jq/grep/sed) -------------
# Independent of the herdr env guards: the model is part of the display, which
# the wrapper always owns. Any failure (no python3, parse error, missing/empty
# key) leaves MODEL empty, which skips reporting but never blocks chaining.
MODEL=""
if [ -n "$input_file" ] && command -v python3 >/dev/null 2>&1; then
  MODEL="$(HERDR_STATUSLINE_INPUT_FILE="$input_file" python3 - <<'PY' 2>/dev/null || true
import json
import os

path = os.environ.get("HERDR_STATUSLINE_INPUT_FILE")
try:
    with open(path, encoding="utf-8") as handle:
        data = json.load(handle)
    model = data.get("model") if isinstance(data, dict) else None
    display_name = model.get("display_name") if isinstance(model, dict) else None
    if isinstance(display_name, str) and display_name.strip():
        # Strip control chars (tab/newline included): they would corrupt the
        # tab-separated `<model>\t<epoch>` state line and defeat the heartbeat
        # cache, and have no place in a display name.
        cleaned = "".join(ch for ch in display_name if ch.isprintable()).strip()
        if cleaned:
            print(cleaned, end="")
except Exception:
    pass
PY
)"
fi

# --- reporting (gated on the herdr env; success advances the heartbeat) -------
# Reporting is skipped entirely unless we are inside a herdr pane and have a
# model to report. Chaining below runs regardless.
can_report=1
[ "${HERDR_ENV:-}" = "1" ] || can_report=0
[ -n "${HERDR_SOCKET_PATH:-}" ] || can_report=0
[ -n "${HERDR_PANE_ID:-}" ] || can_report=0
[ -n "$MODEL" ] || can_report=0

if [ "$can_report" = "1" ]; then
  now="$(date +%s 2>/dev/null || echo 0)"

  # Server generation: pane ids restart from 1 on server boot and restore
  # reallocates them, so ids are reused across restarts. Key the state file to
  # the current server generation to make stale hits from a prior generation
  # impossible. Prefer an explicit id; otherwise derive it from the socket
  # file's mtime (GNU `stat -c` or BSD `stat -f`).
  server_gen="${HERDR_SERVER_ID:-}"
  if [ -z "$server_gen" ]; then
    server_gen="$(stat -c %Y "$HERDR_SOCKET_PATH" 2>/dev/null \
      || stat -f %m "$HERDR_SOCKET_PATH" 2>/dev/null || true)"
    # A failed stat coerces to a stable literal so the state name stays
    # well-formed. An explicit HERDR_SERVER_ID is herdr-controlled and used
    # verbatim (not required to be numeric).
    is_uint "$server_gen" || server_gen=0
  fi

  my_uid="$(id -u 2>/dev/null || true)"

  # State dir: per-user runtime dir, created 0700. XDG_RUNTIME_DIR is already
  # per-user; the /tmp fallback is shared, so the dir name is uid-qualified
  # (herdr-statusline-<uid>) to keep users from colliding on one dir — without
  # it, the first user to create /tmp/herdr-statusline would lock everyone else
  # out via the ownership check below. On the /tmp fallback we still verify the
  # dir is uid-owned and not a symlink before trusting it; on mismatch, skip
  # reporting (still chain) rather than risk a hostile pre-created dir.
  using_tmp_fallback=0
  runtime_base="${XDG_RUNTIME_DIR:-}"
  if [ -z "$runtime_base" ]; then
    runtime_base="${TMPDIR:-/tmp}"
    using_tmp_fallback=1
  fi
  if [ "$using_tmp_fallback" = "1" ] && [ -n "$my_uid" ]; then
    state_dir="$runtime_base/herdr-statusline-$my_uid"
  else
    state_dir="$runtime_base/herdr-statusline"
  fi
  (umask 077 && mkdir -p "$state_dir") 2>/dev/null || state_dir=""

  if [ -n "$state_dir" ] && [ "$using_tmp_fallback" = "1" ]; then
    if [ -L "$state_dir" ]; then
      debug "state dir is a symlink; skipping report"
      state_dir=""
    else
      dir_owner="$(stat -c %u "$state_dir" 2>/dev/null \
        || stat -f %u "$state_dir" 2>/dev/null || true)"
      if [ -z "$dir_owner" ] || [ -z "$my_uid" ] || [ "$dir_owner" != "$my_uid" ]; then
        debug "state dir not uid-owned; skipping report"
        state_dir=""
      fi
    fi
  fi

  if [ -n "$state_dir" ]; then
    state_file="$state_dir/$server_gen-$HERDR_PANE_ID"

    # Read recorded "<model>\t<epoch>". A missing file, a line with no tab, a
    # non-integer epoch, or a future-dated epoch is all treated as absent:
    # report and overwrite, never an error.
    recorded_model=""
    recorded_epoch=""
    state_present=0
    if [ -f "$state_file" ]; then
      line="$(head -n1 "$state_file" 2>/dev/null || true)"
      case "$line" in
        *"$TAB"*)
          recorded_model="${line%%"$TAB"*}"
          recorded_epoch="${line#*"$TAB"}"
          if is_uint "$recorded_epoch" && [ "$recorded_epoch" -le "$now" ]; then
            state_present=1
          fi
          ;;
      esac
    fi

    # Report on model change, on a heartbeat-expired epoch, or when state is
    # absent/corrupt/future. Otherwise short-circuit before any herdr call: the
    # steady-state path does one python3 parse plus a handful of cheap local
    # builtins/stats and never forks `herdr` or touches the network (FR9c).
    #
    # This read-decide-write is intentionally unlocked. Claude Code can fire
    # concurrent renders for one pane, so at first render after a (re)start or
    # at simultaneous heartbeat expiry several invocations may each decide to
    # report and fork `herdr` at once. That burst is benign: the report is
    # idempotent and TTL-bounded, the state write is atomic (mktemp + mv -f, so
    # no torn line), and it self-heals once any write lands. A future hardening
    # (an advisory mkdir-lock around this block plus a concurrency stress test)
    # is tracked in .local/KNOWN-ISSUES.md; steady-state correctness does not
    # depend on it.
    should_report=0
    if [ "$state_present" != "1" ]; then
      should_report=1
    elif [ "$recorded_model" != "$MODEL" ]; then
      should_report=1
    elif [ "$(( now - recorded_epoch ))" -ge "$HEARTBEAT_SECS" ]; then
      should_report=1
    fi

    if [ "$should_report" = "1" ]; then
      # The state-file timestamp advances ONLY on a confirmed report, so a
      # heartbeat fired while the server is briefly down never records a
      # phantom success. The report exit status is never propagated.
      if run_report herdr pane report-metadata "$HERDR_PANE_ID" \
        --source herdr:claude-statusline --agent claude \
        --model "$MODEL" --ttl-ms "$TTL_MS" >/dev/null 2>&1; then
        tmp_state="$(mktemp "$state_dir/.state.XXXXXX" 2>/dev/null || true)"
        if [ -n "$tmp_state" ]; then
          if printf '%s%s%s\n' "$MODEL" "$TAB" "$now" >"$tmp_state" 2>/dev/null; then
            mv -f "$tmp_state" "$state_file" 2>/dev/null || rm -f "$tmp_state" 2>/dev/null || true
          else
            rm -f "$tmp_state" 2>/dev/null || true
          fi
        fi
        debug "reported model=$MODEL"
      else
        debug "report failed; heartbeat not advanced"
      fi

      # Cleanup sweep runs only on report-sending invocations (off the FR9c hot
      # path): drop state files not touched in 24h (1440 minutes).
      find "$state_dir" -maxdepth 1 -type f -mmin +1440 -exec rm -f {} + 2>/dev/null || true
    else
      debug "short-circuit: model unchanged and inside heartbeat window"
    fi
  fi
fi

# --- chain (last; its output is the wrapper's own) ----------------------------
# The recorded prior statusLine command is fed the buffered JSON and its stdout
# becomes ours. With no recorded command, print the model when known, else
# nothing. Reporting above never changes what is emitted here.
if [ -n "${HERDR_STATUSLINE_CHAIN:-}" ]; then
  if [ -n "$input_file" ] && [ -f "$input_file" ]; then
    sh -c "$HERDR_STATUSLINE_CHAIN" <"$input_file" || true
  else
    # mktemp failed, so stdin was never buffered; pass the process's own stdin
    # straight through rather than feeding the chain an empty stream.
    sh -c "$HERDR_STATUSLINE_CHAIN" || true
  fi
elif [ -n "$MODEL" ]; then
  printf '%s\n' "$MODEL"
fi
