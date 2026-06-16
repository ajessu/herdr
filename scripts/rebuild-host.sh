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

FORK_REPO=/workplace/albjessu/herdr
UPSTREAM_REPO=/workplace/albjessu/herdr-upstream
INSTALL=/home/albjessu/.local/bin/herdr
TARGET=x86_64-unknown-linux-musl
ZIG_VERSION=0.15.2
RUST_IMAGE=rust:1-bookworm

usage() {
    cat <<EOF >&2
usage: rebuild-host.sh <upstream|fork>

  upstream  fast-forward $UPSTREAM_REPO (master) from origin/master, build
  fork      fast-forward $FORK_REPO     (main)   from ajessu/main,   build

Output: $INSTALL
Build artifacts go to <repo>/target/ (cargo) and <repo>/.local/zig-cache/.
EOF
    exit 1
}

[[ $# -eq 1 ]] || usage

case "$1" in
    upstream) REPO="$UPSTREAM_REPO" REMOTE=origin BRANCH=master ;;
    fork)     REPO="$FORK_REPO"     REMOTE=ajessu BRANCH=main ;;
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

docker run --rm \
    -v "$REPO":/src \
    -w /src \
    -e ZIG_VERSION="$ZIG_VERSION" \
    -e TARGET="$TARGET" \
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
        rm -rf .zig-cache vendor/libghostty-vt/.zig-cache vendor/libghostty-vt/zig-out
        export LIBGHOSTTY_VT_OPTIMIZE=ReleaseFast
        export LIBGHOSTTY_VT_SIMD=true
        mkdir -p /src/.local/zig-cache
        export ZIG_GLOBAL_CACHE_DIR=/src/.local/zig-cache
        cargo build --release --locked --target "${TARGET}"
    '

BIN="$REPO/target/$TARGET/release/herdr"
rm -f "$INSTALL"
cp "$BIN" "$INSTALL"
"$INSTALL" --version
