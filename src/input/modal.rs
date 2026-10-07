//! Fork: key resolution for the modal layer (see `config/keybinds/modal.rs`).

use std::borrow::Cow;

use crate::config::{ActionKeybinds, Keybinds, ModalAction, StickyMode};

use super::keybind_help::{KeybindHelpEntry, KeybindHelpGroup};
use super::keybindings::{action_matches, generated_character_key, KeybindAction, KeybindDispatch};
use super::TerminalKey;

/// The fork's direct shortcuts. Upstream's resolver falls through to this
/// after its own table, so an upstream binding on the same key wins.
pub(super) fn resolve_modal_direct_action(
    keybinds: &Keybinds,
    key: &TerminalKey,
    dispatch: KeybindDispatch,
) -> Option<KeybindAction> {
    let modal = &keybinds.modal;
    [
        (&keybinds.stack_pane, KeybindAction::StackPane),
        (&keybinds.unstack_pane, KeybindAction::UnstackPane),
        (&modal.split_auto, KeybindAction::SplitAuto),
        (&modal.move_tab_left, KeybindAction::MoveTabLeft),
        (&modal.move_tab_right, KeybindAction::MoveTabRight),
        (&modal.resize_grow, KeybindAction::ResizeGrow),
        (&modal.resize_shrink, KeybindAction::ResizeShrink),
    ]
    .into_iter()
    .find(|(bindings, _)| action_matches(bindings, key, dispatch))
    .map(|(_, action)| action)
}

/// What a mode-entry key (`keys.mode_*`) asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeEntry {
    Locked,
    Sticky(StickyMode),
}

pub(crate) fn resolve_mode_entry(keybinds: &Keybinds, key: &TerminalKey) -> Option<ModeEntry> {
    let entry = &keybinds.modal.entry;
    StickyMode::ALL
        .into_iter()
        .find(|mode| entry.for_mode(*mode).matches_direct_key(key))
        .map(ModeEntry::Sticky)
        .or_else(|| {
            entry
                .locked
                .matches_direct_key(key)
                .then_some(ModeEntry::Locked)
        })
}

/// Look `key` up in a mode's table. Retries with the character the terminal
/// generated, because a pane in report-all keyboard mode makes the host send
/// keys like `?` or `[` as shifted escape sequences.
pub(crate) fn resolve_sticky_action(
    keybinds: &Keybinds,
    mode: StickyMode,
    key: &TerminalKey,
) -> Option<ModalAction> {
    let table = keybinds.modal.mode(mode);
    table
        .resolve(key)
        .or_else(|| generated_character_key(key).and_then(|generated| table.resolve(&generated)))
}

fn label(bindings: &ActionKeybinds) -> String {
    bindings.label().unwrap_or_else(|| "unset".to_owned())
}

fn entry(key: String, description: &'static str) -> KeybindHelpEntry {
    (key, Cow::Borrowed(description))
}

/// Help-overlay groups: the mode-entry keys and fork shortcuts, then one group
/// per mode table.
pub(crate) fn modal_help_groups(keybinds: &Keybinds) -> Vec<KeybindHelpGroup> {
    let modal = &keybinds.modal;
    let mut groups = vec![(
        "modes",
        vec![
            entry(label(&modal.entry.pane), "pane mode"),
            entry(label(&modal.entry.tab), "tab mode"),
            entry(label(&modal.entry.resize), "resize mode"),
            entry(label(&modal.entry.move_), "move mode"),
            entry(label(&modal.entry.session), "session mode"),
            entry(label(&modal.entry.locked), "lock / unlock"),
            entry("esc / enter".to_owned(), "leave mode"),
            entry(label(&modal.split_auto), "split pane"),
            entry(label(&keybinds.stack_pane), "stack pane"),
            entry(label(&keybinds.unstack_pane), "unstack pane"),
            entry(label(&modal.move_tab_left), "move tab left"),
            entry(label(&modal.move_tab_right), "move tab right"),
            entry(label(&modal.resize_grow), "grow pane"),
            entry(label(&modal.resize_shrink), "shrink pane"),
        ],
    )];
    for mode in StickyMode::ALL {
        let name = match mode {
            StickyMode::Pane => "pane mode",
            StickyMode::Tab => "tab mode",
            StickyMode::Resize => "resize mode",
            StickyMode::Move => "move mode",
            StickyMode::Session => "session mode",
        };
        let mut entries: Vec<KeybindHelpEntry> = modal
            .mode(mode)
            .entries
            .iter()
            .map(|binding| entry(label(&binding.bindings), binding.description))
            .collect();
        if matches!(mode, StickyMode::Tab | StickyMode::Session) {
            entries.push(entry("1..9".to_owned(), "switch tab"));
        }
        if mode == StickyMode::Session {
            entries.push(entry("enter".to_owned(), "open selected workspace"));
        }
        groups.push((name, entries));
    }
    groups
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::input::{resolve_direct_binding, resolve_prefix_binding, KeybindMatch};

    fn action(found: Option<KeybindMatch>) -> Option<KeybindAction> {
        match found {
            Some(KeybindMatch::Action(action)) => Some(action),
            _ => None,
        }
    }

    #[test]
    fn fork_direct_shortcuts_resolve_through_the_shared_resolver() {
        let keybinds = Keybinds::default();
        let alt = |ch| TerminalKey::new(KeyCode::Char(ch), KeyModifiers::ALT);
        assert_eq!(
            action(resolve_direct_binding(&keybinds, &alt('n'))),
            Some(KeybindAction::SplitAuto)
        );
        assert_eq!(
            action(resolve_direct_binding(&keybinds, &alt('i'))),
            Some(KeybindAction::MoveTabLeft)
        );
        assert_eq!(
            action(resolve_direct_binding(&keybinds, &alt('o'))),
            Some(KeybindAction::MoveTabRight)
        );
        assert_eq!(
            action(resolve_direct_binding(&keybinds, &alt('='))),
            Some(KeybindAction::ResizeGrow)
        );
        assert_eq!(
            action(resolve_direct_binding(&keybinds, &alt('-'))),
            Some(KeybindAction::ResizeShrink)
        );
        assert_eq!(
            action(resolve_direct_binding(&keybinds, &alt('h'))),
            Some(KeybindAction::FocusPaneLeft)
        );
    }

    #[test]
    fn stack_and_unstack_resolve_under_the_prefix() {
        let keybinds = Keybinds::default();
        let shift = |ch| TerminalKey::new(KeyCode::Char(ch), KeyModifiers::SHIFT);
        assert_eq!(
            action(resolve_prefix_binding(&keybinds, &shift('s'))),
            Some(KeybindAction::StackPane)
        );
        assert_eq!(
            action(resolve_prefix_binding(&keybinds, &shift('u'))),
            Some(KeybindAction::UnstackPane)
        );
    }

    #[test]
    fn help_lists_entry_keys_and_every_mode_table() {
        let groups = modal_help_groups(&Keybinds::default());
        let names: Vec<_> = groups.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "modes",
                "pane mode",
                "tab mode",
                "resize mode",
                "move mode",
                "session mode"
            ]
        );
        let modes = &groups[0].1;
        assert!(modes
            .iter()
            .any(|(key, label)| key == "ctrl+p" && label == "pane mode"));
        let pane = &groups[1].1;
        assert!(pane
            .iter()
            .any(|(key, label)| key == "n" && label == "split"));
    }
}
