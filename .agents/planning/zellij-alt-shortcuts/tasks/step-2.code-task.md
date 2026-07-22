# Task: Document Alt Shortcuts in Unreleased Docs

## Description
Add documentation for the new default Alt+key shortcuts to the unreleased docs, covering the binding table, known limitations, configuration shape change, destructive behavior warning, and troubleshooting guidance. Update the changelog with the feature entry.

## Background
Step 1 added direct Alt+key keybindings that mirror Zellij's ergonomics: Alt+h/j/k/l for pane focus, Alt+n for auto-split, Alt+x for close pane, Alt+z for zoom, Alt+=/- for resize, and Alt+i/o for tab reorder. These bindings work in terminal mode without entering prefix mode. The implementation is complete; this task documents the feature for the next release.

## Technical Requirements
1. Add an "Alt shortcuts" section to `docs/next/website/src/content/docs/keyboard.mdx`
2. Document three known limitations: unconditional silent interception, terminal-mode-only scope, outer multiplexer consumption
3. Add a dedicated warning for Alt+x destructive behavior
4. Document the config shape change in `docs/next/website/src/content/docs/configuration.mdx`
5. Add troubleshooting guidance for "my Alt key does nothing"
6. Update `docs/next/CHANGELOG.md`

## Acceptance Criteria
- Alt binding table documented with all default bindings
- Known limitations clearly stated
- Alt+x destructive behavior called out with admonition
- Config shape change documented (arrays for dual-bound, single-string still parses)
- Troubleshooting guidance present
- Changelog updated
