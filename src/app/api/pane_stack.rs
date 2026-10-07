//! Fork: `pane.stack` / `pane.unstack` for stacked panes (`Node::Stack`).

use ratatui::layout::Direction;

use crate::api::schema::{PaneStackResult, PaneTarget, ResponseResult};
use crate::app::App;
use crate::layout::{PaneId, TileLayout};

use super::responses::{encode_error, encode_success};

#[derive(Clone, Copy)]
pub(super) enum StackEdit {
    Stack,
    Unstack,
}

impl App {
    pub(super) fn handle_pane_stack_edit(
        &mut self,
        id: String,
        target: PaneTarget,
        edit: StackEdit,
    ) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&target.pane_id) else {
            return pane_not_found(id, &target.pane_id);
        };
        let Some(tab_idx) = self.state.workspaces[ws_idx].find_tab_index_for_pane(pane_id) else {
            return pane_not_found(id, &target.pane_id);
        };
        let Some(pane_public_id) = self.public_pane_id(ws_idx, pane_id) else {
            return pane_not_found(id, &target.pane_id);
        };

        let changed = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
            .is_some_and(|tab| apply(&mut tab.layout, pane_id, edit));
        if changed {
            self.schedule_session_save();
        }

        let Some(layout) = self.pane_layout_snapshot(ws_idx, tab_idx) else {
            return encode_error(id, "pane_layout_unavailable", "pane layout unavailable");
        };
        let focused_pane_id = layout.focused_pane_id.clone();
        if changed {
            self.emit_layout_updated_snapshot(layout.clone());
        }

        let stack = PaneStackResult {
            changed,
            pane_id: pane_public_id,
            focused_pane_id,
            layout,
        };
        encode_success(
            id,
            match edit {
                StackEdit::Stack => ResponseResult::PaneStack { stack },
                StackEdit::Unstack => ResponseResult::PaneUnstack { unstack: stack },
            },
        )
    }
}

fn apply(layout: &mut TileLayout, pane_id: PaneId, edit: StackEdit) -> bool {
    match edit {
        StackEdit::Stack => layout.stack_pane(pane_id),
        // Unstacked panes land below the stack's remaining members, split evenly.
        StackEdit::Unstack => layout.unstack_pane(pane_id, Direction::Vertical, 0.5),
    }
}

fn pane_not_found(id: String, pane_id: &str) -> String {
    encode_error(id, "pane_not_found", format!("pane {pane_id} not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{ErrorResponse, EventData, Method, Request, SuccessResponse};
    use crate::config::Config;
    use crate::workspace::Workspace;

    fn app_with_two_panes() -> (App, PaneId, PaneId) {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![Workspace::test_new("stack")];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        let root = app.state.workspaces[0].tabs[0].root_pane;
        let below = app.state.workspaces[0].test_split(Direction::Vertical);
        app.state.ensure_test_terminals();
        (app, root, below)
    }

    fn root_is_stack(app: &App) -> bool {
        matches!(
            app.state.workspaces[0].tabs[0].layout.root(),
            crate::layout::Node::Stack { .. }
        )
    }

    fn call(app: &mut App, method: Method) -> String {
        app.handle_api_request(Request {
            id: "req".into(),
            method,
        })
    }

    fn result(response: &str) -> PaneStackResult {
        let success: SuccessResponse = serde_json::from_str(response).expect("success response");
        match success.result {
            ResponseResult::PaneStack { stack } => stack,
            ResponseResult::PaneUnstack { unstack } => unstack,
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn stack_and_unstack_a_non_focused_pane_keep_focus() {
        let (mut app, root, below) = app_with_two_panes();
        app.state.workspaces[0].tabs[0].layout.focus_pane(root);
        let below_public = app.public_pane_id(0, below).unwrap();
        let root_public = app.public_pane_id(0, root).unwrap();

        let stacked = result(&call(
            &mut app,
            Method::PaneStack(PaneTarget {
                pane_id: below_public.clone(),
            }),
        ));
        assert!(stacked.changed);
        assert_eq!(stacked.pane_id, below_public);
        assert_eq!(stacked.focused_pane_id, root_public);
        assert!(root_is_stack(&app));
        assert!(matches!(
            &app.event_hub
                .events_after(0)
                .last()
                .expect("layout event")
                .1
                .data,
            EventData::LayoutUpdated { .. }
        ));
        app.state.workspaces[0].assert_invariants_for_test();
        assert_eq!(app.state.workspaces[0].tabs[0].layout.focused(), root);

        let unstacked = result(&call(
            &mut app,
            Method::PaneUnstack(PaneTarget {
                pane_id: below_public.clone(),
            }),
        ));
        assert!(unstacked.changed);
        assert_eq!(unstacked.focused_pane_id, root_public);
        assert!(!root_is_stack(&app));
        app.state.workspaces[0].assert_invariants_for_test();
    }

    #[test]
    fn unchanged_edits_report_changed_false_without_a_layout_event() {
        let (mut app, root, _below) = app_with_two_panes();
        let root_public = app.public_pane_id(0, root).unwrap();
        let events_before = app.event_hub.events_after(0).len();

        let unstacked = result(&call(
            &mut app,
            Method::PaneUnstack(PaneTarget {
                pane_id: root_public,
            }),
        ));
        assert!(!unstacked.changed);
        assert_eq!(app.event_hub.events_after(0).len(), events_before);
    }

    #[test]
    fn unknown_pane_is_pane_not_found() {
        let (mut app, _, _) = app_with_two_panes();
        let response = call(
            &mut app,
            Method::PaneStack(PaneTarget {
                pane_id: "nope".into(),
            }),
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "pane_not_found");
    }
}
