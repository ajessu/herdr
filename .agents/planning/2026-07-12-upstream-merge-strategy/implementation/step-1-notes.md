# Step-1 execution notes (preflight + web-client archive/drop)

Date: 2026-07-13
Branch: `merge` (based on a1804a8, per criterion 6)

## Preflight gate outcomes

- **Rust 1.96.1 (criterion 1):** container `rust:1-bookworm` base ships 1.96.0,
  but `rustup toolchain install 1.96.1` succeeds in-container
  (`rustc 1.96.1 (31fca3adb 2026-06-26)`). Upstream's `rust-toolchain.toml`
  (channel 1.96.1, clippy+rustfmt) will auto-provision it post-merge. Confirmed.
- **Snapshot format-version field (criterion 2):** already present —
  `src/persist/snapshot.rs` `SNAPSHOT_VERSION = 4`, `version: u32` on
  `SessionSnapshot`/`SessionHistorySnapshot`. Nothing introduced.
- **Snapshot backup + corpus (criterion 2):** live `~/.config/herdr/` backed up
  to `.local/preflight/snapshot-backup-a1804a8/` (rollback floor); real corpus in
  `.local/preflight/snapshot-corpus/` (both gitignored — snapshots carry personal
  workspace paths). See `.local/preflight/README.md` for the real-vs-constructive
  corpus coverage split (step-2 supplies the stack/floating/sidebar-ratio field
  completeness; step-1's real corpus is step-3's silent-data-loss guard).
- **Branch base (criterion 6):** `merge` fast-forwarded 3e93d5f → a1804a8 (clean
  FF; 3e93d5f is a1804a8's parent). a1804a8 is the sidebar-backtrack, the design's
  named recovery anchor.

## Archive (criterion 3)

- Annotated tag `archive/web-client-a1804a8` + branch `archive/web-client`, both
  at pre-drop commit a1804a8, **local-only** (trust-proxy is an auth-delegation
  surface that must not leak to remote).
- Build status recorded in tag message: `cargo check --locked --features web`
  passed clean in container (rustc 1.96.0), 2026-07-13.

## Drop scope — deviation from plan text

The plan/design (disposition table + Phase A step 1) list
`scripts/herdr-tunnel.sh` for deletion "with the web client," on the stated
premise that it "existed to expose the fork web server." **Ground truth
contradicts this:** the actual file (added in 48ea1f8 alongside the allow_nested
default-flip docs) is an SSH-tunnel wrapper for the KEEP `herdr --remote` bridge
that strips `HERDR_*` env vars. It has ZERO web references. Deleting it would
break the KEEP remote/nested-guard workflow.

**Decision (user-confirmed 2026-07-13): KEEP `scripts/herdr-tunnel.sh`.** The
web-specific tunneling that genuinely drops with the web client is the
`tunnel create $WEB_PORT` public-tunnel block inside `swap-restart.sh`, which was
removed. The disposition-table entry "herdr-tunnel script | DROP with the web
client | existed to expose the fork web server" is factually wrong about this
file and should be corrected in FORK.md / any future disposition record.

## What was removed

- Deleted: `src/web/` (7 files), `web-assets/` (12 files), `src/cli/web.rs`,
  `tests/web_client.rs`.
- Cargo.toml: `web` feature; optional deps axum, axum-extra, tower-http,
  rust-embed, subtle, getrandom; base dep `tokio-util` (only web used it);
  dev-deps tokio-tungstenite, futures-util, http (only tests/web_client.rs used
  them); `web-assets/**/*` from the package `include` list. Cargo.lock synced to
  drop the orphaned deps (lock must match manifest for `cargo check --locked`).
- Wiring: `mod web` in main.rs + cli.rs; the `"web"` CLI dispatch arm;
  `Method::WebStart`/`WebStatus` + their `web.start`/`web.status` name arms in
  schema.rs and api/server.rs; `WebStartParams`/`WebStatusParams` (schema/server.rs);
  `WebMode` + `WebStarted`/`WebAlreadyRunning`/`WebStatus` response variants
  (schema/response.rs); the 7 web schema unit tests (schema/tests.rs);
  `WebServerState` struct + `web_state` field/inits + two dispatch arms + the two
  `handle_web_*` fns (headless.rs). Kept `is_false` helper (also used by
  agents.rs). Reworded one incidental log string ("web client" → "client").
- Scripts: `--features web` removed from test-host.sh and rebuild-host.sh; web +
  public-tunnel steps removed from swap-restart.sh (core stop/restart kept).

## Deferred to step-3 (not step-1 scope)

- `herdr-api.schema.json` is not a committed artifact (generated on demand), so
  no schema-JSON edit was needed. Step-3 regenerates it post-merge.
- Stable docs (`website/`) web references are not touched during feature work.
