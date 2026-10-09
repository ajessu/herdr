# herdr


<p align="center">
  <img src="assets/logo.png" alt="herdr" width="100" />
</p>

<p align="center">
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/herdrdev/herdr/releases"><img src="https://img.shields.io/github/downloads/herdrdev/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/herdrdev/herdr/stargazers"><img src="https://img.shields.io/github/stars/herdrdev/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/herdrdev/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/herdrdev/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

## this fork

This is [ajessu/herdr](https://github.com/ajessu/herdr), a personal fork of
[herdrdev/herdr](https://github.com/herdrdev/herdr) kept in sync with upstream releases
(see [`UPSTREAM.md`](./UPSTREAM.md) for the sync process). On top of everything upstream
ships, this fork adds the items below. The chrome features are off by default; turn them on
under `[ui]`:

```toml
[ui]
hint_bar = "full"
tabs.style = "zellij"
sidebar.style = "zellij"
show_tab_status = "all"   # or "attention"
```

- **modal keybindings**: zellij-style sticky modes on top of upstream's flat `[keys]`
  (`mode_pane`/`mode_tab`/`mode_resize`/`mode_move`/`mode_session` entry keys with
  per-mode `[keys.pane]`, `[keys.tab]`, ... tables, plus a `mode_locked` pass-through
  mode), and direct `alt+` shortcuts (focus, split, close, zoom, new/rename tab,
  non-wrapping tab moves, grow/shrink) alongside the prefix layer.
- **stacked panes**: group panes into a stack within a split (`Node::Stack`), with
  `pane.stack`/`pane.unstack` API methods, keybinds, resize, and persistence.
- **contextual hint bar** (`ui.hint_bar`): a zellij-style row below the panes showing the
  active mode's keys from the live keymap, with an Alt-shortcut section.
- **zellij-style tab bar** (`ui.tabs.style`): Powerline tiles centered on the active tab,
  `+N` overflow tiles that count hidden blocked/working/finished agents and jump to the
  most urgent one, per-tab status dots (`ui.show_tab_status`), middle-click close,
  double-click rename, and Move left/right in the tab context menu.
- **zellij-style sidebar** (`ui.sidebar.style`): a 7-column collapsed rail with attention
  markers and attention-gated agent clicks, `+N` overflow badges on the rail and the
  expanded lists, agent labels that stay bright while pending and dim once settled, and
  heavier scrollbars.
- **nested-launch and remote hygiene**: `experimental.allow_nested` defaults to `true`
  with a same-server recursion guard (rather than blocking all nesting), and
  remote/tunnel subprocesses have every `HERDR_*` runtime env var scrubbed, not just the
  socket path. Set `allow_nested = false` to restore upstream's blocking behavior.

Two former fork features now live in the private
[ajessu/herdr-plugins](https://github.com/ajessu/herdr-plugins) repo instead of core:
the Claude Code statusLine model reporter (`claude-statusline` plugin) and recipes for
break-pane-to-tab and labeled, status-filtered agent lists built from the stock CLI.

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **detach without stopping work** — herdr keeps terminals running in a background server when you close the client or lose your SSH connection. after a server or machine restart, herdr restores the saved layout and can resume supported agent sessions; the original processes do not survive. [session state →](https://herdr.dev/docs/session-state/)
- **several machines, one window** — keep local work and saved ssh machines together, with a combined agent list and independent reconnects. [remote machines →](https://herdr.dev/docs/connecting-machines/)
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so.
- **agent-native** — agents drive herdr through the cli and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. herdr doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [endpoint-protected Windows](https://herdr.dev/docs/windows-beta/) · [binaries](https://github.com/herdrdev/herdr/releases)

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [connecting machines](https://herdr.dev/docs/connecting-machines/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@herdr.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/herdrdev/herdr
cd herdr
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
