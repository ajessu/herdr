#!/usr/bin/env bash
#
# Personal helper for albjessu: swap the running herdr server to the freshly
# built ~/.local/bin/herdr binary by stopping and cold-restarting it.
#
# WHY a stop/restart instead of live-handoff: a cold restart deterministically
# boots the new on-disk binary and restores the session snapshot, avoiding the
# subtle state carried across a live-handoff.
#
# SAFETY: this script MUST run detached (setsid) so that `herdr server stop`,
# which tears down herdr panes, does not kill the script before it restarts the
# server. Launch it like:
#
#   setsid bash scripts/swap-restart.sh >/tmp/herdr-swap.log 2>&1 < /dev/null &
#
# Then watch /tmp/herdr-swap.log. The script sleeps SLEEP_SECS between stop and
# start so the old server fully exits and releases the API socket.
#
# The session snapshot (~/.config/herdr/session.json) is restored on cold boot,
# so workspaces/tabs/panes come back and agent panes resume into their native
# sessions (resume_agents_on_restore defaults true). Note: resumed agents come
# back WITHOUT their original launch flags (known herdr limitation).

set -uo pipefail

# Clear any inherited herdr socket overrides so we talk to the real stable
# server, not a debug instance.
unset HERDR_ENV HERDR_SOCKET_PATH HERDR_CLIENT_SOCKET_PATH 2>/dev/null || true

HERDR_BIN="${HERDR_BIN:-$HOME/.local/bin/herdr}"
SLEEP_SECS="${SLEEP_SECS:-15}"

log() { printf '%s  %s\n' "$(date '+%H:%M:%S')" "$*"; }

log "=== herdr swap-restart starting ==="
log "binary: $HERDR_BIN"
"$HERDR_BIN" --version 2>&1 | sed 's/^/  /'

# 1. Stop the running server.
log "stopping server..."
"$HERDR_BIN" server stop 2>&1 | sed 's/^/  /' || log "server stop returned non-zero (may already be down)"

# 2. Wait for the API socket to actually free up (bounded by SLEEP_SECS).
log "waiting ${SLEEP_SECS}s for old server to exit..."
sleep "$SLEEP_SECS"

# 3. Wait for a server to come back. An attached herdr client auto-respawns the
#    server when it sees the socket gone, and that respawn loads the current
#    on-disk binary — which is exactly the swap we want. So first give the
#    auto-respawn a chance; only start one ourselves as a fallback if nothing
#    comes back. (Starting unconditionally races the respawn and fails with
#    "server is already running".)
log "waiting for server to come back (client auto-respawn)..."
up=0
for i in $(seq 1 20); do
    if "$HERDR_BIN" status server >/dev/null 2>&1; then
        up=1
        log "server is up via auto-respawn (after ${i}s)"
        break
    fi
    sleep 1
done

if [ "$up" -ne 1 ]; then
    log "no auto-respawn — starting server ourselves (detached)..."
    setsid "$HERDR_BIN" server >/tmp/herdr-server-boot.log 2>&1 < /dev/null &
    disown 2>/dev/null || true
    for i in $(seq 1 20); do
        if "$HERDR_BIN" status server >/dev/null 2>&1; then
            up=1
            log "server is up via explicit start (after ${i}s)"
            break
        fi
        sleep 1
    done
fi

if [ "$up" -ne 1 ]; then
    log "ERROR: server did not come up; check /tmp/herdr-server-boot.log"
    exit 1
fi

log "=== done ==="
