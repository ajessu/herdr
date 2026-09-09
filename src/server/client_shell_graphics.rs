use crate::kitty_graphics::surface::{DeliveryCache, SourceFiles};
use crate::protocol::{ClientShellPopupSurface, SurfaceGraphicsScene};

pub(crate) fn collect_retained(
    app: &crate::app::App,
    panes: &[crate::protocol::PaneSurfacePane],
    target: crate::ui::TabSurfaceTarget,
    cell_size: crate::kitty_graphics::HostCellSize,
    delivered: &DeliveryCache,
    client_id: u64,
) -> Option<(SurfaceGraphicsScene, DeliveryCache, SourceFiles)> {
    let rect = |rect: crate::protocol::SurfaceRect| {
        ratatui::layout::Rect::new(rect.x, rect.y, rect.width, rect.height)
    };
    let pane_infos = panes
        .iter()
        .map(|pane| {
            let (workspace_index, id) = app.parse_pane_id(&pane.pane_id)?;
            if workspace_index != target.workspace_index {
                return None;
            }
            // Fork: the surface carries no stack state; collapsed members are
            // title rows and must not get image placements.
            let collapsed = app
                .state
                .workspaces
                .get(workspace_index)
                .and_then(|workspace| workspace.tabs.get(target.tab_index))
                .is_some_and(|tab| tab.layout.is_collapsed_stack_member(id));
            Some(crate::layout::PaneInfo {
                id,
                rect: rect(pane.rect),
                inner_rect: rect(pane.inner_rect),
                scrollbar_rect: pane.scrollbar_rect.map(rect),
                borders: ratatui::widgets::Borders::NONE,
                is_focused: pane.focused,
                stack: collapsed.then_some(crate::layout::StackMember {
                    collapsed: true,
                    position: 0,
                    count: 0,
                }),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    // Image clipping uses the retained content rectangles, not borders or split handles.
    Some(collect(
        app,
        &pane_infos,
        &[],
        None,
        Some(target),
        cell_size,
        delivered,
        client_id,
    ))
}

pub(crate) fn collect(
    app: &crate::app::App,
    pane_infos: &[crate::layout::PaneInfo],
    split_borders: &[crate::layout::SplitBorder],
    popup: Option<&ClientShellPopupSurface>,
    target: Option<crate::ui::TabSurfaceTarget>,
    cell_size: crate::kitty_graphics::HostCellSize,
    delivered: &DeliveryCache,
    client_id: u64,
) -> (SurfaceGraphicsScene, DeliveryCache, SourceFiles) {
    let popup_content_size = popup.map(|popup| (popup.frame.width, popup.frame.height));
    crate::kitty_graphics::surface::collect_scene(
        app,
        crate::ui::TabSurfaceView {
            target,
            pane_infos,
            split_borders,
        },
        popup_content_size,
        cell_size,
        delivered,
        client_id,
    )
}
