#!/usr/bin/env bash
#
# Personal host script for albjessu's Cloud Desktop (AL2 / glibc 2.26).
# Builds herdr in a Debian Bookworm container against musl, mirroring upstream
# release config, then installs to ~/.local/bin/herdr.
#
# Build artifacts go to $REPO/target/ (cargo) and $REPO/.local/zig-cache/
# (zig), both already gitignored by upstream. Not part of upstream's
# release/CI flow.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
FORK_REPO=$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)
UPSTREAM_REPO="${FORK_REPO}-upstream"

INSTALL=$(command -v herdr || true)
if [[ -z "$INSTALL" ]]; then
    echo "error: 'herdr' not found on PATH; install once before using this script" >&2
    exit 1
fi
INSTALL=$(readlink -f "$INSTALL")
TARGET=x86_64-unknown-linux-musl
ZIG_VERSION=0.15.2
RUST_IMAGE=rust:1-bookworm

usage() {
    cat <<EOF >&2
usage: rebuild-host.sh <upstream|fork>

  upstream  fast-forward $UPSTREAM_REPO (master) from origin/master, build
  fork      fast-forward $FORK_REPO     (main)   from origin/main,   build

Output: $INSTALL
Build artifacts go to <repo>/target/ (cargo) and <repo>/.local/zig-cache/.
EOF
    exit 1
}

[[ $# -eq 1 ]] || usage

case "$1" in
    upstream) REPO="$UPSTREAM_REPO" REMOTE=origin BRANCH=master ;;
    fork)     REPO="$FORK_REPO"     REMOTE=origin BRANCH=main ;;
    *) usage ;;
esac

cd "$REPO"

current=$(git rev-parse --abbrev-ref HEAD)
if [[ "$current" != "$BRANCH" ]]; then
    echo "error: $REPO is on '$current', expected '$BRANCH'" >&2
    exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: $REPO has uncommitted changes; commit/stash first" >&2
    exit 1
fi

echo "==> fetching $REMOTE"
git fetch "$REMOTE" --prune

echo "==> fast-forwarding $BRANCH to $REMOTE/$BRANCH"
git merge --ff-only "$REMOTE/$BRANCH"

commit=$(git rev-parse --short HEAD)
echo "==> building $REPO @ $commit ($BRANCH)"

HOST_UID=$(id -u)
HOST_GID=$(id -g)

docker run --rm \
    -v "$REPO":/src \
    -w /src \
    -e ZIG_VERSION="$ZIG_VERSION" \
    -e TARGET="$TARGET" \
    -e HOST_UID="$HOST_UID" \
    -e HOST_GID="$HOST_GID" \
    "$RUST_IMAGE" \
    bash -euo pipefail -c '
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends \
            cmake ninja-build musl-tools curl xz-utils ca-certificates pkg-config \
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
        rustup target add "${TARGET}"
        cd /src
        # build.rs rerun-if-changed watches inputs only, never outputs.
        # If vendor/libghostty-vt/zig-out is missing (e.g. wiped by a prior
        # broken run), cargo will skip build.rs based on its cached
        # fingerprint, link will fail with `cannot find -lghostty-vt`.
        # Force a rerun by touching build.rs when zig-out is absent.
        if [ ! -d vendor/libghostty-vt/zig-out/lib ]; then
            echo "zig-out missing — forcing build.rs rerun"
            touch build.rs
        fi
        export LIBGHOSTTY_VT_OPTIMIZE=ReleaseFast
        export LIBGHOSTTY_VT_SIMD=true
        mkdir -p /src/.local/zig-cache
        export ZIG_GLOBAL_CACHE_DIR=/src/.local/zig-cache
        cargo build --release --locked --target "${TARGET}"
        # Match build outputs to host ownership so the next run can reuse
        # them without sudo chown. Container runs as root; without this,
        # target/ and .local/ are root-owned on the host.
        chown -R "${HOST_UID}:${HOST_GID}" /src/target /src/.local /src/vendor/libghostty-vt/zig-out /src/vendor/libghostty-vt/.zig-cache 2>/dev/null || true
    '

BIN="$REPO/target/$TARGET/release/herdr"
rm -f "$INSTALL"
cp "$BIN" "$INSTALL"
"$INSTALL" --version

# The running herdr server is a long-lived process. Replacing the binary on
# disk does NOT restart it — the running session and any active panes keep
# running on whatever binary they were started from.
echo
echo "The running herdr server is still on the previous binary."
echo "restart with: herdr update --handoff   (live swap, panes survive)"
echo "          or: herdr server stop && herdr   (clean restart, kills panes)"
