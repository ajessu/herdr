# Task: Clear Stale Drag State on Tab Switch, Workspace Switch, and Outer Focus Loss

## Description
Add defensive drag-state clearing to tab-switch, workspace-switch, and outer-focus-lost event handlers so that an interrupted drag gesture (e.g., mouse-up lost over a web transport, or focus stolen by another window) does not leave residual drag state that corrupts pane ratios or scroll offsets on subsequent mouse moves.

## Background
Herdr's `AppState` holds `drag: Option<DragState>`, `workspace_press: Option<WorkspacePressState>`, and `tab_press: Option<TabPressState>`. These are set on mouse-press/move and cleared on mouse-up. However, if the user switches tabs, switches workspaces, or loses outer terminal focus while a drag is in-flight, the mouse-up event may never arrive (especially common on web/mobile). The right-click passthrough handler (mouse.rs:1591-1593) already clears all three together as a precedent.

## Technical Requirements
1. Clear `self.drag`, `self.workspace_press`, and `self.tab_press` in `AppState::switch_tab` (actions.rs).
2. Clear `self.drag`, `self.workspace_press`, and `self.tab_press` in `AppState::switch_workspace` (actions.rs).
3. Clear `self.drag`, `self.workspace_press`, and `self.tab_press` in `AppState::switch_workspace_tab` (actions.rs).
4. Clear `self.state.drag`, `self.state.workspace_press`, and `self.state.tab_press` in the `OuterFocusLost` handler (runtime.rs).
5. Unit tests cover each of the three clear triggers (tab switch, workspace switch, outer focus loss).
6. A press → move → switch → move sequence leaves no pane ratio or scroll offset changed.
7. `just check` passes.

## Acceptance Criteria
- Active drag state is cleared when switching tabs, switching workspaces, and on the outer-focus-lost event.
- The existing tab-activation paths route through a single tab-switch chokepoint where the clear fires.
- A press → move → (tab switch / workspace switch / outer-focus-lost) → move sequence leaves drag state cleared and changes no pane ratio or scroll offset.
- Unit tests cover each of the three clear triggers.
- `just check` passes.
