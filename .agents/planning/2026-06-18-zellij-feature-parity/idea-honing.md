# Requirements Clarification

## Q1: What's the primary use case — agent orchestration with multiple panes, or general terminal multiplexer?
**A (assumption):** Both. Herdr's differentiator is agent views/workspaces, but day-to-day terminal multiplexer ergonomics should not suffer. The user wants Zellij-level UX polish for pane/tab management while keeping herdr's unique features.

## Q2: Which Zellij shortcuts were most relied upon?
**A (assumption):** Alt+direction for pane navigation, Alt+n for new pane, Alt+[] for tab switching. The user explicitly mentioned alt-related shortcuts and their visibility.

## Q3: Is the goal full Zellij parity or selective cherry-picking of high-value features?
**A (assumption):** Selective cherry-picking. Focus on features that are high-value for daily use and feasible to implement given herdr's architecture. Not trying to clone Zellij.

## Q4: Priority between visual polish (status bar, shortcut hints) vs functional (stacked panes, resize modes)?
**A (assumption):** Both matter, but functional features (stacked panes) paired with discoverability (shortcut hints in the bar) is the sweet spot.
