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

#[test]
fn sidebar_prefixed_status_entries_move_from_tab_bar_to_sidebar_top() {
    let mut projected = two_spaces();
    projected.tab_bar_right = vec![
        crate::protocol::ClientShellTabStatusSegment {
            text: "sidebar:CPU 12%   GPU 3%".into(),
            accent: false,
        },
        crate::protocol::ClientShellTabStatusSegment {
            text: "14:05".into(),
            accent: false,
        },
    ];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("frame");
    let rows = (0..frame.height)
        .map(|y| {
            (0..frame.width)
                .map(|x| frame.cells[(y * frame.width + x) as usize].symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let all = rows.join("\n");
    assert!(all.contains("CPU 12%"), "stats shown somewhere:\n{all}");
    assert!(!all.contains("sidebar:"), "prefix is never drawn:\n{all}");
    assert!(all.contains("14:05"), "other entries stay in the tab bar:\n{all}");
    let cpu_row = rows.iter().position(|row| row.contains("CPU 12%")).unwrap();
    let spaces_row = rows.iter().position(|row| row.contains(" spaces")).unwrap();
    assert!(cpu_row < spaces_row, "stats sit above the spaces list");
}

fn test_agent(pane_id: &str, tab_id: &str, workspace_id: &str, name: &str) -> crate::protocol::ClientShellAgent {
    crate::protocol::ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: workspace_id.into(),
        tab_id: tab_id.into(),
        name: Some(name.into()),
        display_agent: None,
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: crate::api::schema::AgentStatus::Working,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    }
}

fn session_lines(projected: &ClientShellSnapshot, tab: usize, width: u16) -> Vec<String> {
    let sessions = projected.tabs.iter().skip(tab).take(1).collect::<Vec<_>>();
    let config = ClientShellConfig::from_config(&Config::default());
    super::super::sidebar::space_session_lines(
        projected,
        &sessions,
        false,
        config.status_indicators,
        &config.palette,
        width,
    )
    .remove(0)
    .iter()
    .map(|line| line_text(line))
    .collect()
}

#[test]
fn session_name_keeps_its_prefix_line_and_wraps_at_spaces() {
    let mut projected = two_spaces();
    projected.tabs[1].label = "continuous-improvement plan notes".into();
    let text = session_lines(&projected, 1, 32);
    assert!(text[0].contains('└') && text[0].contains("continuous-improvement"), "{text:?}");
    assert!(text.len() >= 2, "{text:?}");
    let name_start = text[0].chars().take_while(|c| *c != 'c').count();
    let indent = text[1].chars().take_while(|c| *c == ' ').count();
    assert_eq!(indent, name_start, "continuation sits under the name: {text:?}");
}

#[test]
fn wrap_spans_adds_no_empty_line_at_exact_width() {
    let spans = vec![ratatui::text::Span::raw("continuous-improvement")];
    let lines = super::super::agent_sidebar::wrap_spans(spans, 23, 1, 3);
    let text = lines.iter().map(|line| line_text(line)).collect::<Vec<_>>();
    assert_eq!(text, [" continuous-improvement"]);
}

#[test]
fn agent_blocks_have_a_space_bar_and_no_blank_lines_at_exact_width() {
    let mut projected = two_spaces();
    projected.agents.push(test_agent("pane_1", "tab_1", "ws_1", "continuous-improvement"));
    projected.agents.push(test_agent("pane_2", "tab_2", "ws_2", "second"));
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 40).expect("frame");
    let rows = (0..frame.height)
        .map(|y| {
            (0..frame.width)
                .map(|x| frame.cells[(y * frame.width + x) as usize].symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let body = state.hits.agent_body;
    let bar_rows = (body.y..body.bottom())
        .filter(|y| rows[*y as usize].chars().nth(body.x as usize) == Some('▌'))
        .count();
    assert!(bar_rows >= 2, "every agent line has the bar:\n{}", rows.join("\n"));
    let spans = state.hits.endpoint_agents.iter().map(|(rect, _, _)| *rect).collect::<Vec<_>>();
    for rect in spans {
        for y in rect.y..rect.bottom() {
            assert_eq!(
                rows[y as usize].chars().nth(body.x as usize),
                Some('▌'),
                "no blank line inside an agent block (row {y}):\n{}",
                rows.join("\n")
            );
        }
    }
}

#[test]
fn dragging_a_multi_pane_session_keeps_panes_together() {
    let mut projected = two_spaces();
    let mut pane = projected.panes[0].clone();
    pane.pane_id = "pane_3".into();
    pane.focused = false;
    projected.panes.push(pane);
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("frame");
    let source = sidebar_tab(&state, "tab_1");
    let target = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .expect("ws_2 row")
        .rect;
    mouse(&mut state, MouseEventKind::Down(MouseButton::Left), source);
    mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), target);
    let drop = mouse(&mut state, MouseEventKind::Up(MouseButton::Left), target);
    assert!(
        drop.actions.is_empty(),
        "multi-pane moves run as one ordered sequence into a single tab, not one NewTab per pane: {:?}",
        drop.actions
    );
}

#[test]
fn sessions_with_several_agents_list_each_agent_pane() {
    let mut projected = two_spaces();
    projected.agents.push(test_agent("pane_1", "tab_1", "ws_1", "orchestrator"));
    projected.agents.push(test_agent("pane_3", "tab_1", "ws_1", "helper-bot"));
    let text = session_lines(&projected, 0, 40);
    assert_eq!(text.len(), 3, "{text:?}");
    assert!(text[1].contains("orchestrator") && text[2].contains("helper-bot"), "{text:?}");

    // A single-agent session stays on one line.
    let mut single = two_spaces();
    single.agents.push(test_agent("pane_1", "tab_1", "ws_1", "orchestrator"));
    assert_eq!(session_lines(&single, 0, 40).len(), 1);
}

#[test]
fn session_label_prefers_custom_label_then_terminal_title_then_number() {
    use super::super::sidebar::session_label;
    let mut projected = two_spaces();
    // Custom label wins.
    assert_eq!(session_label(&projected, &projected.tabs[1], 1), "review");

    // Lone unnamed tab with no title shows nothing; with siblings it shows its number.
    let tab = projected.tabs[0].clone();
    assert_eq!(session_label(&projected, &tab, 1), "");
    assert_eq!(session_label(&projected, &tab, 2), tab.number.to_string());

    // A blank title is ignored; the focused agent's title wins over others.
    let mut blank = test_agent("pane_1", "tab_1", "ws_1", "a");
    blank.terminal_title_stripped = Some("   ".into());
    projected.agents.push(blank);
    assert_eq!(session_label(&projected, &tab, 1), "");
    let mut other = test_agent("pane_3", "tab_1", "ws_1", "b");
    other.terminal_title_stripped = Some("other title".into());
    projected.agents.push(other);
    let mut focused = test_agent("pane_4", "tab_1", "ws_1", "c");
    focused.terminal_title_stripped = Some("fix login".into());
    focused.focused = true;
    projected.agents.push(focused);
    assert_eq!(session_label(&projected, &tab, 2), "fix login");
}
