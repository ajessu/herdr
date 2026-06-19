#!/usr/bin/env bash
#
# Personal host script for albjessu's Cloud Desktop (AL2 / glibc 2.26).
# Runs the herdr test suite in a Debian Bookworm container, because the host's
# glibc 2.26 is too old to link the vendored libghostty-vt (the zig std lib
# references copy_file_range, added in glibc 2.27). Mirrors the container setup
# in rebuild-host.sh.
#
# Build artifacts go to $REPO/target/ (cargo) and $REPO/.local/zig-cache/
# (zig), both already gitignored by upstream. Not part of upstream's
# release/CI flow.
#
# Usage:
#   scripts/test-host.sh                  # full suite (cargo nextest)
#   scripts/test-host.sh -E 'test(foo)'   # pass extra args through to nextest
#
# Note: the live_handoff::live_server_holds_one_pty_master_fd_per_pane test
# fails inside Docker because /dev/ptmx is bind-mounted from the host, so the
# /proc/<pid>/fd targets don't match the literal "/dev/ptmx" the test greps
# for. It passes on a real host. Exclude it when you want a clean run:
#   scripts/test-host.sh -E 'not test(live_server_holds_one_pty_master_fd_per_pane)'

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)

# A worktree's .git is a file pointing at the parent repo's gitdir; bind-mount
# that gitdir too so git works inside the container (cargo/nextest read it).
GIT_COMMON_DIR=$(git -C "$REPO" rev-parse --path-format=absolute --git-common-dir)

ZIG_VERSION=0.15.2
RUST_IMAGE=rust:1-bookworm
HOST_UID=$(id -u)
HOST_GID=$(id -g)

docker_args=(
    --rm
    -v "$REPO":/src
    -w /src
    -e ZIG_VERSION="$ZIG_VERSION"
    -e HOST_UID="$HOST_UID"
    -e HOST_GID="$HOST_GID"
)
# Mount the real gitdir at its own path so the worktree's .git pointer resolves.
if [[ "$GIT_COMMON_DIR" != "$REPO/.git" ]]; then
    docker_args+=(-v "$GIT_COMMON_DIR":"$GIT_COMMON_DIR")
fi

docker run "${docker_args[@]}" \
    "$RUST_IMAGE" \
    bash -euo pipefail -c '
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends \
            cmake ninja-build musl-tools curl xz-utils ca-certificates pkg-config git \
            >/dev/null
        if ! command -v zig >/dev/null; then
            cd /tmp
            curl -sSL -o zig.tar.xz \
                "https://ziglang.org/download/${ZIG_VERSION}/zig-x86_64-linux-${ZIG_VERSION}.tar.xz"
            tar xf zig.tar.xz
            ln -sf "/tmp/zig-x86_64-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig
            rm zig.tar.xz
        fi
        zig version
        if ! command -v cargo-nextest >/dev/null; then
            cargo install cargo-nextest --locked
        fi
        git config --global --add safe.directory /src
        git config --global --add safe.directory "*"
        cd /src
        # See rebuild-host.sh: force a build.rs rerun if zig-out was wiped, or
        # cargo skips it on its cached fingerprint and the link fails.
        if [ ! -d vendor/libghostty-vt/zig-out/lib ]; then
            echo "zig-out missing — forcing build.rs rerun"
            touch build.rs
        fi
        export LIBGHOSTTY_VT_OPTIMIZE=ReleaseFast
        export LIBGHOSTTY_VT_SIMD=true
        mkdir -p /src/.local/zig-cache
        export ZIG_GLOBAL_CACHE_DIR=/src/.local/zig-cache
        status=0
        cargo nextest run --locked "$@" || status=$?
        # Match build outputs to host ownership so the next host/container run
        # can reuse them without sudo chown (container runs as root).
        chown -R "${HOST_UID}:${HOST_GID}" \
            /src/target /src/.local /src/vendor/libghostty-vt/zig-out /src/vendor/libghostty-vt/.zig-cache \
            2>/dev/null || true
        exit $status
    ' bash "$@"
