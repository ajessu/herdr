//! Fork: resolution and validation of the modal layer configured in
//! `config/modal_keys.rs` and the fork's flat fields on `KeysConfig`.
//!
//! Entry keys and the extra direct shortcuts share upstream's main keyspace, so
//! they go through the same `BindingRegistry` as every flat binding (first wins,
//! user beats default, the prefix stays reserved). Each per-mode table is its
//! own keyspace where bare keys are allowed.

use crossterm::event::{KeyCode, KeyModifiers};
use tracing::warn;

use super::{
    parse_action_bindings, parse_binding_string, reject_binding, ActionKeybinds, BindingConfig,
    BindingRegistry, BindingSource, Keybinds, ParsedBinding, ResolvedBinding,
};
use crate::config::modal_keys::{
    MoveModeKeysConfig, PaneModeKeysConfig, ResizeModeKeysConfig, SessionModeKeysConfig,
    TabModeKeysConfig,
};
use crate::config::Config;
use crate::input::{KeybindAction, TerminalKey};

/// Mode the client starts in, from `keys.default_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DefaultMode {
    #[default]
    Modal,
    Locked,
}

/// A sticky mode: it stays active after each action until it is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StickyMode {
    Pane,
    Tab,
    Resize,
    Move,
    Session,
}

impl StickyMode {
    pub const ALL: [StickyMode; 5] = [
        StickyMode::Pane,
        StickyMode::Tab,
        StickyMode::Resize,
        StickyMode::Move,
        StickyMode::Session,
    ];
}

/// What a per-mode key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalAction {
    Action(KeybindAction),
    /// Move the Session-mode workspace selection without focusing it.
    SessionWorkspaceUp,
    SessionWorkspaceDown,
}

#[derive(Debug, Clone)]
pub struct ModeKeybind {
    /// Dotted config key, e.g. `keys.pane.split_auto`.
    #[allow(dead_code)] // tests use it to tie each entry to its config key
    pub field: &'static str,
    pub description: &'static str,
    pub(crate) action: ModalAction,
    pub bindings: ActionKeybinds,
}

/// One per-mode table, in declaration order.
#[derive(Debug, Clone, Default)]
pub struct ModeKeybinds {
    pub entries: Vec<ModeKeybind>,
}

impl ModeKeybinds {
    pub(crate) fn resolve(&self, key: &TerminalKey) -> Option<ModalAction> {
        self.entries
            .iter()
            .find(|entry| entry.bindings.matches_direct_key(key))
            .map(|entry| entry.action)
    }
}

/// Mode-entry keys (`keys.mode_*`).
#[derive(Debug, Clone, Default)]
pub struct ModeEntryKeybinds {
    pub pane: ActionKeybinds,
    pub tab: ActionKeybinds,
    pub resize: ActionKeybinds,
    pub move_: ActionKeybinds,
    pub session: ActionKeybinds,
    pub locked: ActionKeybinds,
}

impl ModeEntryKeybinds {
    pub fn for_mode(&self, mode: StickyMode) -> &ActionKeybinds {
        match mode {
            StickyMode::Pane => &self.pane,
            StickyMode::Tab => &self.tab,
            StickyMode::Resize => &self.resize,
            StickyMode::Move => &self.move_,
            StickyMode::Session => &self.session,
        }
    }
}

/// Resolved modal layer, carried on `Keybinds::modal`.
#[derive(Debug, Clone, Default)]
pub struct ModalKeybinds {
    pub default_mode: DefaultMode,
    pub entry: ModeEntryKeybinds,
    pub split_auto: ActionKeybinds,
    pub move_tab_left: ActionKeybinds,
    pub move_tab_right: ActionKeybinds,
    pub resize_grow: ActionKeybinds,
    pub resize_shrink: ActionKeybinds,
    pub pane: ModeKeybinds,
    pub tab: ModeKeybinds,
    pub resize: ModeKeybinds,
    pub move_: ModeKeybinds,
    pub session: ModeKeybinds,
}

impl ModalKeybinds {
    pub fn mode(&self, mode: StickyMode) -> &ModeKeybinds {
        match mode {
            StickyMode::Pane => &self.pane,
            StickyMode::Tab => &self.tab,
            StickyMode::Resize => &self.resize,
            StickyMode::Move => &self.move_,
            StickyMode::Session => &self.session,
        }
    }

    fn mode_mut(&mut self, mode: StickyMode) -> &mut ModeKeybinds {
        match mode {
            StickyMode::Pane => &mut self.pane,
            StickyMode::Tab => &mut self.tab,
            StickyMode::Resize => &mut self.resize,
            StickyMode::Move => &mut self.move_,
            StickyMode::Session => &mut self.session,
        }
    }
}

fn push_diagnostic(diagnostics: &mut Vec<String>, diag: String) {
    warn!(message = %diag, "config diagnostic");
    diagnostics.push(diag);
}

/// Parse the entry keys and the fork's direct shortcuts for one source pass.
/// Called first in each pass of `validated_keybinds`, so entry keys claim their
/// combos before any other flat field of the same source.
pub(super) fn apply_modal_direct_bindings(
    config: &Config,
    modal: &mut ModalKeybinds,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) {
    let keys = &config.keys;
    let field_source = |field: &str| {
        if keys.key_field_is_user_configured(field) {
            BindingSource::User
        } else {
            BindingSource::Default
        }
    };

    let entries: [(&str, &BindingConfig, &mut ActionKeybinds); 6] = [
        ("mode_pane", &keys.mode_pane, &mut modal.entry.pane),
        ("mode_tab", &keys.mode_tab, &mut modal.entry.tab),
        ("mode_resize", &keys.mode_resize, &mut modal.entry.resize),
        ("mode_move", &keys.mode_move, &mut modal.entry.move_),
        ("mode_session", &keys.mode_session, &mut modal.entry.session),
        ("mode_locked", &keys.mode_locked, &mut modal.entry.locked),
    ];
    for (name, value, target) in entries {
        if field_source(name) == source {
            *target = parse_entry_bindings(
                &format!("keys.{name}"),
                value,
                registry,
                diagnostics,
                source,
            );
        }
    }

    let direct: [(&str, &BindingConfig, &mut ActionKeybinds); 5] = [
        ("split_auto", &keys.split_auto, &mut modal.split_auto),
        (
            "move_tab_left",
            &keys.move_tab_left,
            &mut modal.move_tab_left,
        ),
        (
            "move_tab_right",
            &keys.move_tab_right,
            &mut modal.move_tab_right,
        ),
        ("resize_grow", &keys.resize_grow, &mut modal.resize_grow),
        (
            "resize_shrink",
            &keys.resize_shrink,
            &mut modal.resize_shrink,
        ),
    ];
    for (name, value, target) in direct {
        if field_source(name) == source {
            *target = parse_action_bindings(
                &format!("keys.{name}"),
                value,
                registry,
                diagnostics,
                source,
            );
        }
    }
}

fn parse_entry_bindings(
    field: &str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> ActionKeybinds {
    let mut bindings = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) => {
                if binding.trigger.is_prefix() {
                    push_diagnostic(
                        diagnostics,
                        format!(
                            "mode entry keybinding must be a direct key: {field} = {raw:?}; disabling binding"
                        ),
                    );
                    continue;
                }
                if reject_binding(field, &binding, registry, diagnostics, source) {
                    continue;
                }
                registry.register(&binding, field, source);
                bindings.push(binding);
            }
            Some(ParsedBinding::Range(_)) | None => push_diagnostic(
                diagnostics,
                format!("invalid keybinding: {field} = {raw:?}; disabling binding"),
            ),
        }
    }
    ActionKeybinds { bindings }
}

struct ModeField<'a> {
    field: &'static str,
    description: &'static str,
    action: ModalAction,
    value: &'a BindingConfig,
    default: &'a BindingConfig,
}

macro_rules! mode_fields {
    ($config:expr, $defaults:expr, $table:literal; $($field:ident => $action:expr, $description:literal;)*) => {
        vec![$(ModeField {
            field: concat!("keys.", $table, ".", stringify!($field)),
            description: $description,
            action: $action,
            value: &$config.$field,
            default: &$defaults.$field,
        }),*]
    };
}

fn act(action: KeybindAction) -> ModalAction {
    ModalAction::Action(action)
}

fn mode_fields<'a>(
    config: &'a Config,
    defaults: &'a ModeDefaults,
    mode: StickyMode,
) -> Vec<ModeField<'a>> {
    use KeybindAction as A;
    let keys = &config.keys;
    match mode {
        StickyMode::Pane => mode_fields!(keys.pane, defaults.pane, "pane";
            focus_left => act(A::FocusPaneLeft), "focus left";
            focus_down => act(A::FocusPaneDown), "focus down";
            focus_up => act(A::FocusPaneUp), "focus up";
            focus_right => act(A::FocusPaneRight), "focus right";
            split_auto => act(A::SplitAuto), "split";
            split_down => act(A::SplitHorizontal), "split down";
            split_right => act(A::SplitVertical), "split right";
            stack => act(A::StackPane), "stack";
            close => act(A::ClosePane), "close";
            zoom => act(A::Zoom), "zoom";
            rename => act(A::RenamePane), "rename";
            cycle_next => act(A::CyclePaneNext), "next pane";
        ),
        StickyMode::Tab => mode_fields!(keys.tab, defaults.tab, "tab";
            previous => act(A::PreviousTab), "previous tab";
            next => act(A::NextTab), "next tab";
            new => act(A::NewTab), "new tab";
            close => act(A::CloseTab), "close tab";
            rename => act(A::RenameTab), "rename tab";
            last_pane => act(A::LastPane), "last pane";
        ),
        // decrease_<side> shrinks from that side, which moves the border the
        // opposite way.
        StickyMode::Resize => mode_fields!(keys.resize, defaults.resize, "resize";
            increase_left => act(A::ResizePaneLeft), "resize left";
            increase_down => act(A::ResizePaneDown), "resize down";
            increase_up => act(A::ResizePaneUp), "resize up";
            increase_right => act(A::ResizePaneRight), "resize right";
            decrease_left => act(A::ResizePaneRight), "shrink from left";
            decrease_down => act(A::ResizePaneUp), "shrink from below";
            decrease_up => act(A::ResizePaneDown), "shrink from above";
            decrease_right => act(A::ResizePaneLeft), "shrink from right";
            grow => act(A::ResizeGrow), "grow";
            shrink => act(A::ResizeShrink), "shrink";
        ),
        StickyMode::Move => mode_fields!(keys.move_, defaults.move_, "move";
            swap_left => act(A::SwapPaneLeft), "swap left";
            swap_down => act(A::SwapPaneDown), "swap down";
            swap_up => act(A::SwapPaneUp), "swap up";
            swap_right => act(A::SwapPaneRight), "swap right";
            cycle_next => act(A::CyclePaneNext), "next pane";
            cycle_previous => act(A::CyclePanePrevious), "previous pane";
        ),
        StickyMode::Session => mode_fields!(keys.session, defaults.session, "session";
            workspace_up => ModalAction::SessionWorkspaceUp, "select workspace above";
            workspace_down => ModalAction::SessionWorkspaceDown, "select workspace below";
            focus_left => act(A::FocusPaneLeft), "focus left";
            focus_right => act(A::FocusPaneRight), "focus right";
            cycle_next => act(A::CyclePaneNext), "next pane";
            goto => act(A::OpenNavigator), "navigator";
            workspace_picker => act(A::WorkspacePicker), "workspace picker";
            new_workspace => act(A::NewWorkspace), "new workspace";
            new_worktree => act(A::NewWorktree), "new worktree";
            rename_workspace => act(A::RenameWorkspace), "rename workspace";
            close_workspace => act(A::CloseWorkspace), "close workspace";
            settings => act(A::Settings), "settings";
            help => act(A::Help), "help";
            detach => act(A::Detach), "detach";
            previous_agent => act(A::PreviousAgent), "previous agent";
            next_agent => act(A::NextAgent), "next agent";
        ),
    }
}

#[derive(Default)]
struct ModeDefaults {
    pane: PaneModeKeysConfig,
    tab: TabModeKeysConfig,
    resize: ResizeModeKeysConfig,
    move_: MoveModeKeysConfig,
    session: SessionModeKeysConfig,
}

fn combo(code: KeyCode) -> (KeyCode, KeyModifiers) {
    (code, KeyModifiers::empty())
}

/// Resolve the per-mode tables and `default_mode`. Called once after both
/// source passes, so `main` holds every flat, entry and custom binding.
pub(super) fn finish_modal_keybinds(
    config: &Config,
    keybinds: &mut Keybinds,
    main: &BindingRegistry,
    diagnostics: &mut Vec<String>,
) {
    let defaults = ModeDefaults::default();
    for mode in StickyMode::ALL {
        let mut registry = BindingRegistry::new(main.prefix_combos.clone(), main.prefix_source);
        for code in [KeyCode::Esc, KeyCode::Enter] {
            registry.reserve_direct(combo(code), "mode exit keys", BindingSource::Default);
        }
        if matches!(mode, StickyMode::Tab | StickyMode::Session) {
            for digit in '1'..='9' {
                registry.reserve_direct(
                    combo(KeyCode::Char(digit)),
                    "tab digit keys",
                    BindingSource::Default,
                );
            }
        }
        if mode == StickyMode::Resize {
            // Upstream's resize mode also exits on its own entry key.
            for binding in &keybinds.resize_mode.bindings {
                registry.reserve_direct(
                    binding.trigger.combo(),
                    "keys.resize_mode",
                    BindingSource::Default,
                );
            }
        }

        let fields = mode_fields(config, &defaults, mode);
        let mut entries: Vec<ModeKeybind> = fields
            .iter()
            .map(|field| ModeKeybind {
                field: field.field,
                description: field.description,
                action: field.action,
                bindings: ActionKeybinds::default(),
            })
            .collect();
        for source in [BindingSource::User, BindingSource::Default] {
            for (field, entry) in fields.iter().zip(entries.iter_mut()) {
                let field_source = if field.value == field.default {
                    BindingSource::Default
                } else {
                    BindingSource::User
                };
                if field_source == source {
                    entry.bindings = parse_mode_bindings(
                        field.field,
                        field.value,
                        &mut registry,
                        main,
                        diagnostics,
                        source,
                    );
                }
            }
        }
        keybinds.modal.mode_mut(mode).entries = entries;
    }

    keybinds.modal.default_mode = match config.keys.default_mode.trim() {
        mode if mode.eq_ignore_ascii_case("modal") => DefaultMode::Modal,
        mode if mode.eq_ignore_ascii_case("locked") => DefaultMode::Locked,
        other => {
            push_diagnostic(
                diagnostics,
                format!("invalid keys.default_mode = {other:?}; falling back to \"modal\""),
            );
            DefaultMode::Modal
        }
    };
    if keybinds.modal.default_mode == DefaultMode::Locked
        && keybinds.modal.entry.locked.bindings.is_empty()
    {
        push_diagnostic(
            diagnostics,
            "keys.default_mode = \"locked\" but keys.mode_locked has no usable key; falling back to \"modal\" so the client cannot start stuck in locked mode".to_string(),
        );
        keybinds.modal.default_mode = DefaultMode::Modal;
    }
}

fn parse_mode_bindings(
    field: &'static str,
    config: &BindingConfig,
    registry: &mut BindingRegistry,
    main: &BindingRegistry,
    diagnostics: &mut Vec<String>,
    source: BindingSource,
) -> ActionKeybinds {
    let mut bindings: Vec<ResolvedBinding> = Vec::new();
    for raw in config.values() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let binding = match parse_binding_string(raw) {
            Some(ParsedBinding::Single(binding)) if binding.trigger.is_direct() => binding,
            Some(ParsedBinding::Single(_)) => {
                push_diagnostic(
                    diagnostics,
                    format!("mode keybinding must not include prefix: {field} = {raw:?}; disabling binding"),
                );
                continue;
            }
            Some(ParsedBinding::Range(_)) | None => {
                push_diagnostic(
                    diagnostics,
                    format!("invalid keybinding: {field} = {raw:?}; disabling binding"),
                );
                continue;
            }
        };
        if let Some(first) = registry.conflict(&binding) {
            if !(source == BindingSource::Default && first.source == BindingSource::User) {
                push_diagnostic(
                    diagnostics,
                    format!("{}: kept {}, disabled {field}", binding.label, first.field),
                );
            }
            continue;
        }
        if let Some(shadow) = main.conflict(&binding) {
            // A direct binding the user chose silently displaces a default
            // table key, matching how user bindings displace defaults upstream.
            if source == BindingSource::Default && shadow.source == BindingSource::User {
                continue;
            }
            // Otherwise keep it: the direct binding wins while the mode is
            // active, and the diagnostic explains why this key does nothing.
            push_diagnostic(
                diagnostics,
                format!(
                    "{}: {field} is shadowed by {} while its mode is active",
                    binding.label, shadow.field
                ),
            );
        }
        registry.register(&binding, field, source);
        bindings.push(binding);
    }
    ActionKeybinds { bindings }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::super::BindingTrigger;
    use super::*;
    use crate::config::is_keybinding_config_diagnostic;

    fn load(toml_src: &str) -> (Keybinds, Vec<String>) {
        let config: Config = toml::from_str(toml_src).expect("config parses");
        let diagnostics = config.collect_diagnostics();
        (config.keybinds(), diagnostics)
    }

    fn triggers(bindings: &ActionKeybinds) -> Vec<BindingTrigger> {
        bindings
            .bindings
            .iter()
            .map(|binding| binding.trigger)
            .collect()
    }

    fn direct(code: KeyCode, modifiers: KeyModifiers) -> BindingTrigger {
        BindingTrigger::Direct((code, modifiers))
    }

    fn ctrl(ch: char) -> BindingTrigger {
        direct(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn bare(ch: char) -> BindingTrigger {
        direct(KeyCode::Char(ch), KeyModifiers::empty())
    }

    fn mode_triggers(keybinds: &Keybinds, mode: StickyMode, field: &str) -> Vec<BindingTrigger> {
        let entry = keybinds
            .modal
            .mode(mode)
            .entries
            .iter()
            .find(|entry| entry.field == field)
            .unwrap_or_else(|| panic!("{field} is not in the {} table", table_name(mode)));
        triggers(&entry.bindings)
    }

    fn table_name(mode: StickyMode) -> &'static str {
        match mode {
            StickyMode::Pane => "pane",
            StickyMode::Tab => "tab",
            StickyMode::Resize => "resize",
            StickyMode::Move => "move",
            StickyMode::Session => "session",
        }
    }

    fn has_diag(diagnostics: &[String], needles: &[&str]) -> bool {
        diagnostics
            .iter()
            .any(|diag| needles.iter().all(|needle| diag.contains(needle)))
    }

    #[test]
    fn default_config_has_no_conflict_diagnostics() {
        let (keybinds, diagnostics) = load("");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(keybinds.modal.default_mode, DefaultMode::Modal);
    }

    #[test]
    fn mode_entry_defaults_match_the_zellij_keymap() {
        let (keybinds, _) = load("");
        let entry = &keybinds.modal.entry;
        assert_eq!(triggers(&entry.pane), vec![ctrl('p')]);
        assert_eq!(triggers(&entry.tab), vec![ctrl('t')]);
        assert_eq!(triggers(&entry.resize), vec![ctrl('n')]);
        assert_eq!(triggers(&entry.move_), vec![ctrl('h')]);
        assert_eq!(triggers(&entry.session), vec![ctrl('o')]);
        assert_eq!(triggers(&entry.locked), vec![ctrl('g')]);
    }

    #[test]
    fn per_mode_defaults_match_the_zellij_keymap() {
        let (keybinds, _) = load("");
        let left = direct(KeyCode::Left, KeyModifiers::empty());
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.focus_left"),
            vec![bare('h'), left]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.split_auto"),
            vec![bare('n')]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Tab, "keys.tab.last_pane"),
            vec![direct(KeyCode::Tab, KeyModifiers::empty())]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Resize, "keys.resize.decrease_left"),
            vec![direct(KeyCode::Char('h'), KeyModifiers::SHIFT)]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Resize, "keys.resize.grow"),
            vec![bare('+'), bare('=')]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Session, "keys.session.help"),
            vec![bare('?')]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Session, "keys.session.new_worktree"),
            vec![direct(KeyCode::Char('n'), KeyModifiers::SHIFT)]
        );
        for mode in StickyMode::ALL {
            for entry in &keybinds.modal.mode(mode).entries {
                assert!(
                    !entry.bindings.bindings.is_empty(),
                    "{} lost its default",
                    entry.field
                );
            }
        }
    }

    #[test]
    fn fork_flat_defaults_append_alt_shortcuts_to_upstream_defaults() {
        let (keybinds, _) = load("");
        let alt = |ch| direct(KeyCode::Char(ch), KeyModifiers::ALT);
        assert_eq!(
            triggers(&keybinds.focus_pane_left),
            vec![
                BindingTrigger::Prefix((KeyCode::Char('h'), KeyModifiers::empty())),
                alt('h')
            ]
        );
        assert!(triggers(&keybinds.close_pane).contains(&alt('x')));
        assert!(triggers(&keybinds.zoom).contains(&alt('z')));
        assert!(triggers(&keybinds.new_tab).contains(&alt('t')));
        assert!(triggers(&keybinds.rename_tab).contains(&alt('r')));
        assert!(triggers(&keybinds.detach).contains(&ctrl('q')));
        assert_eq!(triggers(&keybinds.modal.split_auto), vec![alt('n')]);
        assert_eq!(triggers(&keybinds.modal.move_tab_left), vec![alt('i')]);
        assert_eq!(triggers(&keybinds.modal.move_tab_right), vec![alt('o')]);
        assert_eq!(
            triggers(&keybinds.modal.resize_grow),
            vec![alt('='), alt('+')]
        );
        assert_eq!(triggers(&keybinds.modal.resize_shrink), vec![alt('-')]);
    }

    #[test]
    fn user_flat_field_replaces_the_fork_default_too() {
        let (keybinds, diagnostics) = load("[keys]\nfocus_pane_left = \"prefix+h\"\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            triggers(&keybinds.focus_pane_left),
            vec![BindingTrigger::Prefix((
                KeyCode::Char('h'),
                KeyModifiers::empty()
            ))]
        );
    }

    #[test]
    fn each_mode_has_an_independent_keyspace() {
        let (keybinds, _) = load("");
        for (mode, field) in [
            (StickyMode::Pane, "keys.pane.focus_left"),
            (StickyMode::Resize, "keys.resize.increase_left"),
            (StickyMode::Move, "keys.move.swap_left"),
            (StickyMode::Session, "keys.session.focus_left"),
        ] {
            assert!(mode_triggers(&keybinds, mode, field).contains(&bare('h')));
        }
    }

    #[test]
    fn user_mode_key_displaces_a_default_mode_key_silently() {
        let (keybinds, diagnostics) = load("[keys.pane]\nclose = \"k\"\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.close"),
            vec![bare('k')]
        );
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.focus_up"),
            vec![direct(KeyCode::Up, KeyModifiers::empty())]
        );
    }

    #[test]
    fn conflicting_user_mode_keys_are_first_wins_with_a_diagnostic() {
        let (keybinds, diagnostics) = load("[keys.pane]\nclose = \"m\"\nzoom = \"m\"\n");
        assert!(
            has_diag(
                &diagnostics,
                &["kept keys.pane.close", "disabled keys.pane.zoom"]
            ),
            "{diagnostics:?}"
        );
        assert!(mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.zoom").is_empty());
    }

    #[test]
    fn mode_tables_reserve_exit_digit_and_resize_mode_keys() {
        let (_, diagnostics) = load(
            "[keys.pane]\nclose = \"esc\"\n[keys.tab]\nnew = \"1\"\n[keys.resize]\ngrow = \"r\"\n",
        );
        assert!(has_diag(
            &diagnostics,
            &["kept mode exit keys", "keys.pane.close"]
        ));
        assert!(has_diag(
            &diagnostics,
            &["kept tab digit keys", "keys.tab.new"]
        ));
        assert!(has_diag(
            &diagnostics,
            &["kept keys.resize_mode", "keys.resize.grow"]
        ));
    }

    #[test]
    fn mode_keys_reject_prefix_syntax() {
        let (keybinds, diagnostics) = load("[keys.pane]\nclose = \"prefix+x\"\n");
        assert!(has_diag(
            &diagnostics,
            &["must not include prefix", "keys.pane.close"]
        ));
        assert!(mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.close").is_empty());
    }

    #[test]
    fn user_mode_entry_key_displaces_another_default_entry_silently() {
        let (keybinds, diagnostics) = load("[keys]\nmode_pane = \"ctrl+t\"\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(triggers(&keybinds.modal.entry.pane), vec![ctrl('t')]);
        assert!(keybinds.modal.entry.tab.bindings.is_empty());
    }

    #[test]
    fn duplicate_user_mode_entry_keys_are_first_wins_with_a_diagnostic() {
        let (keybinds, diagnostics) =
            load("[keys]\nmode_pane = \"ctrl+y\"\nmode_tab = \"ctrl+y\"\n");
        assert!(has_diag(
            &diagnostics,
            &["kept keys.mode_pane", "disabled keys.mode_tab"]
        ));
        assert!(keybinds.modal.entry.tab.bindings.is_empty());
    }

    #[test]
    fn mode_entry_equal_to_the_prefix_is_rejected() {
        let (keybinds, diagnostics) = load("[keys]\nmode_pane = \"ctrl+b\"\n");
        assert!(has_diag(
            &diagnostics,
            &["kept keys.prefix", "disabled keys.mode_pane"]
        ));
        assert!(keybinds.modal.entry.pane.bindings.is_empty());
    }

    #[test]
    fn mode_entry_rejects_prefix_syntax_and_bare_printable_keys() {
        let (keybinds, diagnostics) = load("[keys]\nmode_pane = \"prefix+p\"\nmode_tab = \"t\"\n");
        assert!(has_diag(
            &diagnostics,
            &["must be a direct key", "keys.mode_pane"]
        ));
        assert!(has_diag(
            &diagnostics,
            &["unsafe direct keybinding", "keys.mode_tab"]
        ));
        assert!(keybinds.modal.entry.pane.bindings.is_empty());
        assert!(keybinds.modal.entry.tab.bindings.is_empty());
    }

    #[test]
    fn empty_mode_entry_disables_the_mode() {
        let (keybinds, diagnostics) = load("[keys]\nmode_move = \"\"\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(keybinds.modal.entry.move_.bindings.is_empty());
    }

    #[test]
    fn user_mode_key_shadowed_by_a_direct_binding_is_kept_with_a_diagnostic() {
        let (keybinds, diagnostics) =
            load("[keys.pane]\nsplit_auto = \"ctrl+t\"\nclose = \"alt+x\"\n");
        assert!(has_diag(
            &diagnostics,
            &["keys.pane.split_auto is shadowed by keys.mode_tab"]
        ));
        assert!(has_diag(
            &diagnostics,
            &["keys.pane.close is shadowed by keys.close_pane"]
        ));
        assert_eq!(
            mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.close"),
            vec![direct(KeyCode::Char('x'), KeyModifiers::ALT)]
        );
    }

    #[test]
    fn user_direct_binding_silently_displaces_default_entry_and_mode_keys() {
        let (keybinds, diagnostics) = load("[keys]\nnew_tab = \"ctrl+t\"\nprefix = \"n\"\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(keybinds.modal.entry.tab.bindings.is_empty());
        assert!(mode_triggers(&keybinds, StickyMode::Pane, "keys.pane.split_auto").is_empty());
    }

    #[test]
    fn invalid_default_mode_falls_back_to_modal_with_a_keys_diagnostic() {
        let (keybinds, diagnostics) = load("[keys]\ndefault_mode = \"normal\"\n");
        assert_eq!(keybinds.modal.default_mode, DefaultMode::Modal);
        assert!(has_diag(
            &diagnostics,
            &["keys.default_mode", "falling back"]
        ));
    }

    #[test]
    fn default_mode_locked_is_honoured_when_unlock_is_reachable() {
        let (keybinds, diagnostics) = load("[keys]\ndefault_mode = \"Locked\"\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(keybinds.modal.default_mode, DefaultMode::Locked);
    }

    #[test]
    fn default_mode_locked_without_an_unlock_key_falls_back_to_modal() {
        let (keybinds, diagnostics) =
            load("[keys]\ndefault_mode = \"locked\"\nmode_locked = \"\"\n");
        assert_eq!(keybinds.modal.default_mode, DefaultMode::Modal);
        assert!(has_diag(
            &diagnostics,
            &["keys.mode_locked", "falling back"]
        ));
    }

    #[test]
    fn modal_diagnostics_are_keybinding_diagnostics() {
        let (_, diagnostics) = load(
            "[keys]\ndefault_mode = \"x\"\nmode_pane = \"prefix+p\"\nmode_tab = \"ctrl+t\"\nmode_session = \"ctrl+t\"\n[keys.pane]\nclose = \"prefix+x\"\nzoom = \"q\"\nrename = \"q\"\n",
        );
        assert!(diagnostics.len() >= 4, "{diagnostics:?}");
        for diag in &diagnostics {
            assert!(is_keybinding_config_diagnostic(diag), "{diag}");
        }
    }

    #[test]
    fn every_mode_table_field_is_resolved() {
        let (keybinds, _) = load("");
        let tables: [(StickyMode, toml::Value); 5] = [
            (
                StickyMode::Pane,
                toml::Value::try_from(PaneModeKeysConfig::default()).unwrap(),
            ),
            (
                StickyMode::Tab,
                toml::Value::try_from(TabModeKeysConfig::default()).unwrap(),
            ),
            (
                StickyMode::Resize,
                toml::Value::try_from(ResizeModeKeysConfig::default()).unwrap(),
            ),
            (
                StickyMode::Move,
                toml::Value::try_from(MoveModeKeysConfig::default()).unwrap(),
            ),
            (
                StickyMode::Session,
                toml::Value::try_from(SessionModeKeysConfig::default()).unwrap(),
            ),
        ];
        for (mode, table) in tables {
            let mut config_fields: Vec<String> = table
                .as_table()
                .expect("mode table serializes as a table")
                .keys()
                .map(|key| format!("keys.{}.{key}", table_name(mode)))
                .collect();
            let mut resolved: Vec<String> = keybinds
                .modal
                .mode(mode)
                .entries
                .iter()
                .map(|entry| entry.field.to_string())
                .collect();
            config_fields.sort();
            resolved.sort();
            assert_eq!(config_fields, resolved, "{} table", table_name(mode));
        }
    }

    #[test]
    fn keybinding_profile_publishes_effective_entry_keys() {
        let config: Config = toml::from_str("[keys]\nnew_tab = \"ctrl+t\"\n").unwrap();
        let profile = config.local_keybindings_profile_toml().unwrap();
        assert!(profile.contains("mode_pane = \"ctrl+p\""), "{profile}");
        assert!(profile.contains("mode_tab = \"\""), "{profile}");
        assert!(profile.contains("split_auto = \"alt+n\""), "{profile}");

        let remote = crate::config::keybindings_from_profile_toml(&profile).unwrap();
        assert!(remote.keybinds.modal.entry.tab.bindings.is_empty());
        assert_eq!(triggers(&remote.keybinds.modal.entry.pane), vec![ctrl('p')]);
    }
}
