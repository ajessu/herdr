#!/bin/sh
# Create a herdr tunnel to a remote host without inheriting herdr env vars.
# Usage: herdr-tunnel.sh <host> [herdr --remote flags...]
set -e

if [ $# -lt 1 ]; then
    echo "usage: herdr-tunnel.sh <host> [--session <name>] [--handoff]" >&2
    exit 1
fi

# Strip all HERDR_* env vars to prevent guard/session contamination.
unset_args=""
for var in $(env | grep '^HERDR_' | cut -d= -f1); do
    unset_args="$unset_args -u $var"
done

# shellcheck disable=SC2086
exec env $unset_args herdr --remote "$@"
