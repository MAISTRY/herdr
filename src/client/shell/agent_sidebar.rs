use std::collections::{HashMap, HashSet};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) workspace_id: String,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
    /// Colour of the agent's space: drawn as a bar beside the entry and tinting its background.
    pub(super) space_colour: Option<ratatui::style::Color>,
}

pub(super) struct AgentGroupHeader {
    pub(super) key: String,
    pub(super) workspace_index: usize,
    pub(super) foldable: bool,
    pub(super) collapsed: bool,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) count: usize,
    pub(super) first: bool,
}

pub(super) enum AgentPanelItem {
    Header(AgentGroupHeader),
    Agent(AgentRow),
}

pub(super) fn agent_group_key(workspace: &ClientShellWorkspace) -> String {
    let worktree_key = workspace
        .worktree
        .as_ref()
        .map(|w| w.key.as_str())
        .unwrap_or("");
    format!("agents:{}\u{1f}{}", workspace.label, worktree_key)
}

pub(super) fn agent_panel_items(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    collapsed_groups: &HashSet<String>,
) -> Vec<AgentPanelItem> {
    let rows = agent_rows(snapshot, config, None);
    if config.agent_panel_sort != crate::config::AgentPanelSortConfig::Spaces
        || snapshot.agent_view_label.is_some()
    {
        return rows.into_iter().map(AgentPanelItem::Agent).collect();
    }
    let mut group_indices = HashMap::<String, usize>::new();
    let mut groups = Vec::<Vec<AgentRow>>::new();
    for row in rows {
        let index = *group_indices
            .entry(row.workspace_id.clone())
            .or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
        groups[index].push(row);
    }
    let mut items = Vec::new();
    let mut first = true;
    for rows in groups {
        let Some(workspace_index) = snapshot
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id == rows[0].workspace_id)
        else {
            items.extend(rows.into_iter().map(AgentPanelItem::Agent));
            continue;
        };
        let key = agent_group_key(&snapshot.workspaces[workspace_index]);
        // A space with one agent has nothing to fold away, so it always shows its agent.
        let foldable = rows.len() > 1;
        let collapsed = foldable && collapsed_groups.contains(&key);
        let status = rows
            .iter()
            .map(|row| row.status)
            .max_by_key(|status| status_priority(*status))
            .unwrap_or(crate::api::schema::AgentStatus::Unknown);
        items.push(AgentPanelItem::Header(AgentGroupHeader {
            key,
            workspace_index,
            foldable,
            collapsed,
            status,
            count: rows.len(),
            first,
        }));
        first = false;
        items.extend(
            rows.into_iter()
                .filter(|row| !collapsed || row.focused)
                .map(AgentPanelItem::Agent),
        );
    }
    items
}

pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
) -> Vec<String> {
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str())
            })
            .cloned()
            .collect();
    }
    let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
    if sort == crate::config::AgentPanelSortConfig::Priority {
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(status_priority(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
    }
    agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect()
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    collapsed_groups: &HashSet<String>,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let grouped = config.agent_panel_sort == crate::config::AgentPanelSortConfig::Spaces
        && snapshot.agent_view_label.is_none();
    let rows = agent_panel_items(snapshot, config, collapsed_groups);
    render_agent_list_with_gap(
        buffer,
        area,
        &rows,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        if grouped { 0 } else { config.agents.row_gap },
        agent_scroll,
        hits,
        |item, width| match item {
            AgentPanelItem::Header(header) => {
                if header.first {
                    1
                } else {
                    2
                }
            }
            AgentPanelItem::Agent(row) => {
                agent_row_lines(row, config, width, if grouped { 2 } else { 0 }).len()
            }
        },
        |buffer, rect, item, hits| match item {
            AgentPanelItem::Header(header) => {
                let label_rect =
                    Rect::new(rect.x, rect.y + u16::from(!header.first), rect.width, 1);
                if label_rect.y < rect.bottom() {
                    if let Some(colour) = super::sidebar::space_colour(
                        snapshot,
                        &snapshot.workspaces[header.workspace_index],
                        &config.spaces,
                    ) {
                        buffer.set_style(
                            label_rect,
                            Style::default().bg(super::sidebar::space_tint(
                                colour,
                                false,
                                &config.palette,
                            )),
                        );
                    }
                    render_agent_group_header(buffer, label_rect, header, snapshot, config);
                    if header.foldable {
                        hits.agent_group_headers
                            .push((label_rect, header.key.clone()));
                    }
                }
            }
            AgentPanelItem::Agent(row) => {
                hits.agents.push((rect, row.pane_id.clone()));
                render_agent_row(buffer, rect, row, config, if grouped { 2 } else { 0 });
            }
        },
    );
}

fn render_agent_group_header(
    buffer: &mut Buffer,
    rect: Rect,
    header: &AgentGroupHeader,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
) {
    let workspace = &snapshot.workspaces[header.workspace_index];
    let palette = &config.palette;
    let mut tokens =
        super::sidebar::workspace_rows(workspace, header.status, false, &config.spaces)
            .into_iter()
            .next()
            .unwrap_or_default();
    if !header.collapsed {
        tokens.retain(|token| {
            !matches!(
                token.kind,
                crate::ui::ResolvedTokenKind::StateIcon
                    | crate::ui::ResolvedTokenKind::StateText(_)
            )
        });
    }
    let name_style = Style::default()
        .fg(palette.text)
        .add_modifier(Modifier::BOLD);
    let status_style = Style::default().fg(status_color(header.status, palette));
    let width = rect
        .width
        .saturating_sub(if header.foldable { 4 } else { 2 });
    let mut spans = if tokens.is_empty() {
        vec![ratatui::text::Span::styled(
            workspace.label.clone(),
            name_style,
        )]
    } else {
        crate::ui::resolved_token_spans(
            &tokens,
            (
                status_icon(header.status, config.status_indicators),
                status_style,
            ),
            status_style,
            name_style,
            Style::default().fg(palette.overlay0),
            Style::default().fg(palette.overlay1),
            palette,
            width as usize,
        )
    };
    if header.collapsed {
        spans.push(ratatui::text::Span::styled(
            format!(" · {}", header.count),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        ));
    }
    Paragraph::new(Line::from(spans)).render(Rect::new(rect.x + 1, rect.y, width, 1), buffer);
    if header.foldable {
        super::sidebar::render_fold_toggle(buffer, rect, header.collapsed, palette);
    }
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

/// Columns the agent list keeps beside the text: the space colour bar on the left
/// and the scrollbar column on the right.
pub(super) const AGENT_RESERVED_COLUMNS: u16 = 2;

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T, u16) -> usize,
    render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    render_agent_list_with_gap(
        buffer,
        area,
        rows,
        empty_message,
        config,
        config.agents.row_gap,
        agent_scroll,
        hits,
        row_lines,
        render_row,
    );
}

fn render_agent_list_with_gap<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    row_gap: u16,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T, u16) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let row_heights = rows
        .iter()
        // Two columns are reserved (space bar and scrollbar), always, so the measured
        // wrap width is exactly the drawn one.
        .map(|row| {
            row_lines(row, body.width.saturating_sub(AGENT_RESERVED_COLUMNS))
                .max(1)
                .min(u16::MAX as usize) as u16
        })
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(
            |(index, _)| {
                if index + 1 < rows.len() {
                    row_gap
                } else {
                    0
                }
            },
        )
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(1);
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        render_row(buffer, rect, row, hits);
        y = y
            .saturating_add(height)
            .saturating_add(if index + 1 < rows.len() { row_gap } else { 0 });
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Vec<AgentRow> {
    ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .filter_map(|pane_id| agent_row(snapshot, &pane_id, config, machine))
        .collect()
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    // A tab herdr numbered itself says nothing about the agent in it, so the agent's own
    // terminal title (a Claude session's topic) stands in, and the number only as a last resort.
    let tab_label = tab
        .filter(|tab| tab.custom_label)
        .map(|tab| tab.label.as_str())
        .or(agent
            .terminal_title_stripped
            .as_deref()
            .filter(|title| !title.is_empty()))
        .or_else(|| {
            tab.filter(|_| tab_count > 1)
                .map(|tab| tab.label.as_str())
        });
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        workspace_id: agent.workspace_id.clone(),
        status: agent.agent_status,
        focused: agent.focused,
        rows,
        space_colour: super::sidebar::space_colour(snapshot, workspace, &config.spaces),
    })
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
    extra_indent: usize,
) {
    let palette = &config.palette;
    // Agent entries wear a stronger wash of their space colour than the spaces rows.
    let row_style = match row.space_colour {
        Some(colour) => Style::default().bg(super::sidebar::space_tint_amount(
            colour,
            if row.focused { 0.38 } else { 0.22 },
            palette,
        )),
        None if row.focused => Style::default().bg(palette.active_row_bg),
        None => Style::default(),
    };
    for (index, line) in
        agent_row_lines(row, config, rect.width.saturating_sub(1), extra_indent)
            .into_iter()
            .take(rect.height as usize)
            .enumerate()
    {
        let y = rect.y + index as u16;
        Paragraph::new(Line::from(line)).style(row_style).render(
            Rect::new(rect.x + 1, y, rect.width.saturating_sub(1), 1),
            buffer,
        );
        if let Some(cell) = buffer.cell_mut((rect.x, y)) {
            match row.space_colour {
                Some(colour) => {
                    cell.set_char('▌').set_fg(colour);
                }
                None => {
                    cell.set_char(' ');
                }
            }
            if let Some(bg) = row_style.bg {
                cell.set_bg(bg);
            }
        }
    }
}

/// Display lines of one agent entry at `width`: each configured row, wrapped
/// (at spaces when possible) instead of cut off. Continuation lines are indented.
pub(super) fn agent_row_lines(
    row: &AgentRow,
    config: &ClientShellConfig,
    width: u16,
    extra_indent: usize,
) -> Vec<Vec<ratatui::text::Span<'static>>> {
    let palette = &config.palette;
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    let mut lines = Vec::new();
    for (index, tokens) in rows.iter().enumerate() {
        let indent = (if index == 0 { 1 } else { 3 }) + extra_indent;
        let spans = crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            usize::from(u16::MAX),
        );
        lines.extend(wrap_spans(spans, width as usize, indent, indent + 2));
    }
    lines
}

/// Wrap styled text into lines no wider than `width`, breaking after a space
/// when one is on the line. The first line is indented by `first_indent`
/// columns and later lines by `rest_indent`.
pub(super) fn wrap_spans(
    spans: Vec<ratatui::text::Span<'static>>,
    width: usize,
    first_indent: usize,
    rest_indent: usize,
) -> Vec<Vec<ratatui::text::Span<'static>>> {
    use unicode_width::UnicodeWidthChar;
    let cells = spans
        .iter()
        .flat_map(|span| span.content.chars().map(move |ch| (ch, span.style)))
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < cells.len() || lines.is_empty() {
        let indent = if lines.is_empty() { first_indent } else { rest_indent };
        let room = width.saturating_sub(indent).max(1);
        // Skip spaces that would start a continuation line.
        if !lines.is_empty() {
            while start < cells.len() && cells[start].0 == ' ' {
                start += 1;
            }
            if start >= cells.len() {
                break;
            }
        }
        let mut used = 0;
        let mut end = start;
        let mut last_space = None;
        while end < cells.len() {
            let w = cells[end].0.width().unwrap_or(0);
            if used + w > room {
                break;
            }
            if cells[end].0 == ' ' {
                last_space = Some(end);
            }
            used += w;
            end += 1;
        }
        if end < cells.len() {
            if let Some(space) = last_space.filter(|space| *space > start) {
                end = space + 1;
            }
            if end == start {
                end += 1;
            }
        }
        let mut line = vec![ratatui::text::Span::raw(" ".repeat(indent))];
        for (ch, style) in &cells[start..end] {
            match line.last_mut() {
                Some(last) if last.style == *style && !last.content.is_empty() => {
                    last.content.to_mut().push(*ch);
                }
                _ => line.push(ratatui::text::Span::styled(ch.to_string(), *style)),
            }
        }
        lines.push(line);
        start = end;
    }
    lines
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}
