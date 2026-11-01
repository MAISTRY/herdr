use super::*;

/// Two spaces: ws_1 holds tab_1 ("1"), ws_2 holds tab_2 ("review", custom label).
fn two_spaces() -> ClientShellSnapshot {
    let mut projected = snapshot();
    let mut workspace = projected.workspaces[0].clone();
    workspace.workspace_id = "ws_2".into();
    workspace.active_tab_id = "tab_2".into();
    workspace.number = 2;
    workspace.label = "other".into();
    workspace.focused = false;
    projected.workspaces.push(workspace);
    let mut tab = projected.tabs[0].clone();
    tab.tab_id = "tab_2".into();
    tab.workspace_id = "ws_2".into();
    tab.label = "review".into();
    tab.custom_label = true;
    tab.focused = false;
    projected.tabs.push(tab);
    let mut pane = projected.panes[0].clone();
    pane.pane_id = "pane_2".into();
    pane.workspace_id = "ws_2".into();
    pane.tab_id = "tab_2".into();
    pane.focused = false;
    projected.panes.push(pane);
    projected
}

fn ready_state() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_spaces()));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("sidebar frame");
    state
}

fn mouse(state: &mut ClientShellState, kind: MouseEventKind, at: Rect) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind,
        column: at.x + 4,
        row: at.y,
        modifiers: KeyModifiers::empty(),
    })])
}

fn sidebar_tab(state: &ClientShellState, tab_id: &str) -> Rect {
    state
        .hits
        .sidebar_tabs
        .iter()
        .find(|(_, id)| id == tab_id)
        .map(|(rect, _)| *rect)
        .expect("session row")
}

#[test]
fn sidebar_lists_each_space_sessions_under_it() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_spaces()));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("sidebar frame");

    let ids = state
        .hits
        .sidebar_tabs
        .iter()
        .map(|(_, id)| id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["tab_1", "tab_2"]);

    let ws_2 = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .expect("ws_2 row")
        .rect;
    let review = sidebar_tab(&state, "tab_2");
    assert!(review.y >= ws_2.bottom(), "session row sits under its space");

    let row = (0..frame.width)
        .map(|x| frame.cells[(review.y * frame.width + x) as usize].symbol.as_str())
        .collect::<String>();
    assert!(row.contains("review"), "row text was {row:?}");
}

#[test]
fn clicking_a_sidebar_session_focuses_its_tab() {
    let mut state = ready_state();
    let review = sidebar_tab(&state, "tab_2");
    mouse(&mut state, MouseEventKind::Down(MouseButton::Left), review);
    let click = mouse(&mut state, MouseEventKind::Up(MouseButton::Left), review);
    assert!(matches!(
        &click.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(&request.method, crate::api::schema::Method::TabFocus(target) if target.tab_id == "tab_2")
    ));
}

#[test]
fn dragging_a_session_onto_another_space_moves_it_there() {
    let mut state = ready_state();
    let source = sidebar_tab(&state, "tab_1");
    let target = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .expect("ws_2 row")
        .rect;

    mouse(&mut state, MouseEventKind::Down(MouseButton::Left), source);
    let drag = mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), target);
    assert!(drag.repaint);
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Tab { ref target_workspace_id, .. })
            if target_workspace_id.as_deref() == Some("ws_2")
    ));

    let drop = mouse(&mut state, MouseEventKind::Up(MouseButton::Left), target);
    let [ClientShellAction::Endpoint { request, .. }] = &drop.actions[..] else {
        panic!("expected one pane move, got {:?}", drop.actions);
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::PaneMove(params)
            if params.pane_id == "pane_1"
                && params.focus
                && params.destination == crate::api::schema::PaneMoveDestination::NewTab {
                    workspace_id: Some("ws_2".into()),
                    label: None,
                }
    ));
}

#[test]
fn dropping_a_session_on_its_own_space_does_nothing() {
    let mut state = ready_state();
    let source = sidebar_tab(&state, "tab_2");
    let own_space = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .expect("ws_2 row")
        .rect;
    mouse(&mut state, MouseEventKind::Down(MouseButton::Left), source);
    mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), own_space);
    let drop = mouse(&mut state, MouseEventKind::Up(MouseButton::Left), own_space);
    assert!(drop.actions.is_empty(), "got {:?}", drop.actions);
}

fn line_text(line: &[ratatui::text::Span<'static>]) -> String {
    line.iter().map(|span| span.content.as_ref()).collect()
}

#[test]
fn wrap_spans_breaks_at_spaces_and_indents_continuations() {
    let spans = vec![ratatui::text::Span::raw("Linear tickets QA review")];
    let lines = super::super::agent_sidebar::wrap_spans(spans, 12, 1, 3);
    let text = lines.iter().map(|line| line_text(line)).collect::<Vec<_>>();
    assert_eq!(text, [" Linear ", "   tickets ", "   QA review"]);
    for line in &text {
        assert!(unicode_width::UnicodeWidthStr::width(line.as_str()) <= 12, "{line:?}");
    }
}

#[test]
fn wrap_spans_splits_a_word_longer_than_the_line() {
    let spans = vec![ratatui::text::Span::raw("abcdefghij")];
    let lines = super::super::agent_sidebar::wrap_spans(spans, 5, 0, 1);
    let text = lines.iter().map(|line| line_text(line)).collect::<Vec<_>>();
    assert_eq!(text, ["abcde", " fghi", " j"]);
}

#[test]
fn long_session_names_wrap_onto_extra_lines_of_the_same_row() {
    let mut projected = two_spaces();
    projected.tabs[1].label = "a very long session name that cannot fit in the sidebar".into();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 40).expect("sidebar frame");

    let rect = sidebar_tab(&state, "tab_2");
    assert!(rect.height > 1, "long name should take several lines: {rect:?}");
    let text = (rect.y..rect.bottom())
        .map(|y| {
            (rect.x..rect.right())
                .map(|x| frame.cells[(y * frame.width + x) as usize].symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("sidebar"), "end of the name is shown: {text:?}");
}
