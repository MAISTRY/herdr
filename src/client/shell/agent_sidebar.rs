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
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
    /// Colour of the agent's space; its background is tinted with it.
    pub(super) space_color: Option<ratatui::style::Color>,
    /// Extra columns of indent, used under a space header in the grouped list.
    pub(super) extra_indent: usize,
}

/// Header of one space in the grouped agent list.
pub(super) struct AgentGroupHeader {
    /// Fold key in `collapsed_groups`; only set when the space can fold (2+ agents).
    pub(super) fold_key: Option<String>,
    pub(super) label: String,
    pub(super) color: ratatui::style::Color,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) agent_count: usize,
    pub(super) folded: bool,
    /// A blank line sits above every header except the first.
    pub(super) spacer: bool,
}

pub(super) enum AgentListItem {
    Header(AgentGroupHeader),
    Agent(AgentRow),
}

/// Shape of the grouped list, shared by rendering and keyboard reveal.
pub(super) enum AgentLayoutItem<'a> {
    Header {
        workspace: &'a ClientShellWorkspace,
        fold_key: Option<String>,
        folded: bool,
        agent_count: usize,
        spacer: bool,
    },
    Agent(&'a str),
}

/// The agent panel groups agents under space headers when sorted by space
/// and no agent view filter is active.
pub(super) fn agent_panel_grouped(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
) -> bool {
    sort == crate::config::AgentPanelSortConfig::Spaces && snapshot.agent_view_label.is_none()
}

pub(super) fn agent_group_key(workspace: &ClientShellWorkspace) -> String {
    format!(
        "agents:{}\u{1f}{}",
        workspace.label,
        workspace
            .worktree
            .as_ref()
            .map_or("", |worktree| worktree.key.as_str())
    )
}

/// Grouped list layout: a header per space with agents, then its agents (only
/// the focused one while the space is folded).
pub(super) fn grouped_agent_layout<'a>(
    snapshot: &'a ClientShellSnapshot,
    ordered: &[String],
    collapsed_groups: &HashSet<String>,
) -> Vec<AgentLayoutItem<'a>> {
    let mut items = Vec::new();
    for workspace in &snapshot.workspaces {
        let members = ordered
            .iter()
            .filter_map(|pane_id| {
                snapshot.agents.iter().find(|agent| {
                    agent.pane_id == *pane_id && agent.workspace_id == workspace.workspace_id
                })
            })
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        let fold_key = (members.len() >= 2).then(|| agent_group_key(workspace));
        let folded = fold_key
            .as_ref()
            .is_some_and(|key| collapsed_groups.contains(key));
        items.push(AgentLayoutItem::Header {
            workspace,
            fold_key,
            folded,
            agent_count: members.len(),
            spacer: !items.is_empty(),
        });
        items.extend(
            members
                .into_iter()
                .filter(|agent| !folded || agent.focused)
                .map(|agent| AgentLayoutItem::Agent(agent.pane_id.as_str())),
        );
    }
    items
}

/// Index of the agent's item in the agent list (its space header when folded away).
pub(super) fn agent_list_item_index(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
    collapsed_groups: &HashSet<String>,
    pane_id: &str,
) -> Option<usize> {
    let ordered = ordered_agent_pane_ids(snapshot, sort);
    if !agent_panel_grouped(snapshot, sort) {
        return ordered.iter().position(|candidate| candidate == pane_id);
    }
    let workspace_id = &snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?
        .workspace_id;
    let layout = grouped_agent_layout(snapshot, &ordered, collapsed_groups);
    layout
        .iter()
        .position(|item| matches!(item, AgentLayoutItem::Agent(id) if *id == pane_id))
        .or_else(|| {
            layout.iter().position(|item| {
                matches!(item, AgentLayoutItem::Header { workspace, .. }
                    if workspace.workspace_id == *workspace_id)
            })
        })
}

pub(super) fn agent_list_items(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    collapsed_groups: &HashSet<String>,
) -> Vec<AgentListItem> {
    if !agent_panel_grouped(snapshot, config.agent_panel_sort) {
        return agent_rows(snapshot, config, None)
            .into_iter()
            .map(AgentListItem::Agent)
            .collect();
    }
    let ordered = ordered_agent_pane_ids(snapshot, config.agent_panel_sort);
    grouped_agent_layout(snapshot, &ordered, collapsed_groups)
        .into_iter()
        .filter_map(|item| match item {
            AgentLayoutItem::Header {
                workspace,
                fold_key,
                folded,
                agent_count,
                spacer,
            } => {
                let status = snapshot
                    .agents
                    .iter()
                    .filter(|agent| agent.workspace_id == workspace.workspace_id)
                    .map(|agent| agent.agent_status)
                    .max_by_key(|status| status_priority(*status))
                    .unwrap_or(workspace.agent_status);
                Some(AgentListItem::Header(AgentGroupHeader {
                    fold_key,
                    label: workspace.label.clone(),
                    color: super::sidebar::space_color(snapshot, &workspace.workspace_id, config),
                    status,
                    agent_count,
                    folded,
                    spacer,
                }))
            }
            AgentLayoutItem::Agent(pane_id) => {
                agent_row(snapshot, pane_id, config, None).map(|mut row| {
                    row.extra_indent = 2;
                    AgentListItem::Agent(row)
                })
            }
        })
        .collect()
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
    } else {
        // Space order, so keyboard navigation follows the grouped list.
        agents.sort_by_key(|agent| {
            snapshot
                .workspaces
                .iter()
                .position(|workspace| workspace.workspace_id == agent.workspace_id)
                .unwrap_or(usize::MAX)
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

    let items = agent_list_items(snapshot, config, collapsed_groups);
    let row_gap = if agent_panel_grouped(snapshot, config.agent_panel_sort) {
        0
    } else {
        config.agents.row_gap
    };
    render_agent_list(
        buffer,
        area,
        &items,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        row_gap,
        agent_scroll,
        hits,
        |item, width| match item {
            AgentListItem::Header(header) => 1 + usize::from(header.spacer),
            AgentListItem::Agent(row) => agent_row_lines(row, config, width).len(),
        },
        |buffer, rect, item, hits| match item {
            AgentListItem::Header(header) => {
                render_agent_group_header(buffer, rect, header, config, hits);
            }
            AgentListItem::Agent(row) => {
                hits.agents.push((rect, row.pane_id.clone()));
                render_agent_row(buffer, rect, row, config);
            }
        },
    );
}

/// One space header in the grouped list: a blank spacer line (except the first),
/// then the space name on a tinted line with a fold chevron when it has 2+ agents.
fn render_agent_group_header(
    buffer: &mut Buffer,
    rect: Rect,
    header: &AgentGroupHeader,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    use ratatui::text::Span;
    if rect.height == 0 {
        return;
    }
    let palette = &config.palette;
    let line = Rect::new(rect.x, rect.bottom() - 1, rect.width, 1);
    let background = super::sidebar::tint(header.color, palette.sidebar_bg, 0.22);
    buffer.set_style(line, Style::default().bg(background));
    let mut spans = vec![Span::raw(" ")];
    if header.folded {
        spans.push(Span::styled(
            format!("{} ", status_icon(header.status, config.status_indicators)),
            Style::default().fg(status_color(header.status, palette)),
        ));
    }
    spans.push(Span::styled(
        header.label.clone(),
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD),
    ));
    if header.folded {
        spans.push(Span::styled(
            format!(" · {}", header.agent_count),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        ));
    }
    let chevron_width = if header.fold_key.is_some() { 2 } else { 0 };
    Paragraph::new(Line::from(spans))
        .style(Style::default().bg(background))
        .render(
            Rect::new(line.x, line.y, line.width.saturating_sub(chevron_width), 1),
            buffer,
        );
    if let Some(key) = &header.fold_key {
        put_text(
            buffer,
            line.right().saturating_sub(chevron_width),
            line.y,
            chevron_width.min(line.width),
            if header.folded { "▸ " } else { "▾ " },
            Style::default().fg(palette.overlay0).bg(background),
        );
        hits.agent_group_headers.push((line, key.clone()));
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
        .map(|(index, _)| {
            if index + 1 < rows.len() {
                row_gap
            } else {
                0
            }
        })
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
            .saturating_add(if index + 1 < rows.len() {
                row_gap
            } else {
                0
            });
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
    // Custom tab label, else the agent's terminal title, else the tab number
    // when the space has more than one tab.
    let tab_label = tab.and_then(|tab| {
        if tab.custom_label {
            return Some(tab.label.clone());
        }
        agent
            .terminal_title_stripped
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .or_else(|| (tab_count > 1).then(|| tab.number.to_string()))
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
            tab: tab_label.as_deref(),
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
        status: agent.agent_status,
        focused: agent.focused,
        rows,
        space_color: Some(super::sidebar::space_color(
            snapshot,
            &agent.workspace_id,
            config,
        )),
        extra_indent: 0,
    })
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = match row.space_color {
        Some(color) => Style::default().bg(super::sidebar::tint(
            color,
            palette.sidebar_bg,
            if row.focused { 0.38 } else { 0.22 },
        )),
        None if row.focused => Style::default().bg(palette.active_row_bg),
        None => Style::default(),
    };
    for (index, line) in agent_row_lines(row, config, rect.width.saturating_sub(1))
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
            match row.space_color {
                Some(color) => {
                    cell.set_char('▌').set_fg(color);
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
        let indent = row.extra_indent + if index == 0 { 1 } else { 3 };
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
