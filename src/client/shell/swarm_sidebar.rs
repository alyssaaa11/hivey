//! hiver: swarm tree for the Agents panel.
//!
//! Built only from the pane tokens the swarm engine reports (`swarm`, `role`, `queued`),
//! so it needs no protocol changes. Active whenever at least one agent carries a `swarm`
//! token; agents without one are listed under "solo" exactly as before.
//!
//! A collapsed swarm shows its master plus any agent that needs attention (blocked, or
//! finished and unseen). The swarm holding focus is expanded unless the user collapsed it.

use std::collections::{BTreeMap, HashMap};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use super::render::put_text;
use super::*;
use crate::api::schema::AgentStatus;

/// Client-only UI state: explicit expand/collapse choices per swarm slug.
#[derive(Debug, Default, Clone)]
pub(crate) struct SwarmTreeState {
    overrides: HashMap<String, bool>,
}

impl SwarmTreeState {
    pub(super) fn set_expanded(&mut self, slug: &str, expanded: bool) {
        self.overrides.insert(slug.to_string(), expanded);
    }

    fn expanded(&self, slug: &str, holds_focus: bool) -> bool {
        self.overrides.get(slug).copied().unwrap_or(holds_focus)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Role {
    Master,
    Worker,
    Critic,
    Script,
}

impl Role {
    fn from_token(value: Option<&str>) -> Self {
        match value {
            Some("master") => Self::Master,
            Some("critic") => Self::Critic,
            Some("script") => Self::Script,
            _ => Self::Worker,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Master => "◆",
            Self::Worker => "●",
            Self::Critic => "✎",
            Self::Script => "▷",
        }
    }

    fn color(self, palette: &Palette) -> Color {
        match self {
            Self::Master => palette.yellow,
            Self::Worker => palette.blue,
            Self::Critic => palette.mauve,
            Self::Script => palette.overlay0,
        }
    }
}

/// Header click targets: `(rect, slug, expanded, master pane)`.
pub(crate) type SwarmHeaderHit = (Rect, String, bool, Option<String>);

pub(super) enum TreeRow {
    Swarm {
        slug: String,
        expanded: bool,
        members: usize,
        working: usize,
        attention: usize,
        queued: usize,
        rollup: AgentStatus,
        master_pane: Option<String>,
        holds_focus: bool,
        paused: bool,
    },
    Member {
        pane_id: String,
        status: AgentStatus,
        focused: bool,
        role: Role,
        name: String,
        queued: usize,
    },
    SoloHeader,
    Solo(super::agent_sidebar::AgentRow),
}

impl TreeRow {
    fn lines(&self) -> usize {
        match self {
            Self::Solo(row) => row.rows.len(),
            _ => 1,
        }
    }
}

fn needs_attention(status: AgentStatus) -> bool {
    matches!(status, AgentStatus::Blocked | AgentStatus::Done)
}

fn severity(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Blocked => 4,
        AgentStatus::Done => 3,
        AgentStatus::Working => 2,
        AgentStatus::Idle => 1,
        AgentStatus::Unknown => 0,
    }
}

/// `◆ coordinator` → `coordinator`; else the herdr name without its `<slug>-` prefix.
fn member_name(agent: &crate::protocol::ClientShellAgent, slug: &str) -> String {
    if let Some(display) = agent.display_agent.as_deref() {
        let trimmed = display.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let name = agent
        .name
        .as_deref()
        .or(agent.agent.as_deref())
        .unwrap_or("agent");
    name.strip_prefix(&format!("{slug}-"))
        .unwrap_or(name)
        .to_string()
}

/// The tree, or `None` when no agent belongs to a swarm (the normal list is used).
pub(super) fn tree_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    tree: &SwarmTreeState,
) -> Option<Vec<TreeRow>> {
    let order = super::agent_sidebar::ordered_agent_pane_ids(snapshot, config.agent_panel_sort);
    let mut swarms: BTreeMap<String, Vec<(&crate::protocol::ClientShellAgent, Role, usize)>> =
        BTreeMap::new();
    let mut solo = Vec::new();
    let mut paused = std::collections::HashSet::new();
    for pane_id in &order {
        let Some(agent) = snapshot
            .agents
            .iter()
            .find(|agent| &agent.pane_id == pane_id)
        else {
            continue;
        };
        let token = |name: &str| {
            agent
                .tokens
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        match token("swarm") {
            Some(slug) => {
                if token("paused").is_some() {
                    paused.insert(slug.to_string());
                }
                let queued = token("queued").and_then(|q| q.parse().ok()).unwrap_or(0);
                swarms.entry(slug.to_string()).or_default().push((
                    agent,
                    Role::from_token(token("role")),
                    queued,
                ));
            }
            None => solo.push(pane_id.clone()),
        }
    }
    if swarms.is_empty() {
        return None;
    }

    let mut rows = Vec::new();
    for (slug, mut members) in swarms {
        members.sort_by_key(|(agent, role, _)| {
            (*role, std::cmp::Reverse(severity(agent.agent_status)))
        });
        let holds_focus = members.iter().any(|(agent, _, _)| agent.focused);
        let expanded = tree.expanded(&slug, holds_focus);
        rows.push(TreeRow::Swarm {
            slug: slug.clone(),
            expanded,
            members: members.len(),
            working: members
                .iter()
                .filter(|(agent, _, _)| agent.agent_status == AgentStatus::Working)
                .count(),
            attention: members
                .iter()
                .filter(|(agent, _, _)| needs_attention(agent.agent_status))
                .count(),
            queued: members.iter().map(|(_, _, queued)| queued).sum(),
            rollup: members
                .iter()
                .map(|(agent, _, _)| agent.agent_status)
                .max_by_key(|status| severity(*status))
                .unwrap_or(AgentStatus::Unknown),
            master_pane: members
                .iter()
                .find(|(_, role, _)| *role == Role::Master)
                .map(|(agent, _, _)| agent.pane_id.clone()),
            holds_focus,
            paused: paused.contains(&slug),
        });
        for (agent, role, queued) in members {
            if expanded || role == Role::Master || needs_attention(agent.agent_status) {
                rows.push(TreeRow::Member {
                    pane_id: agent.pane_id.clone(),
                    status: agent.agent_status,
                    focused: agent.focused,
                    role,
                    name: member_name(agent, &slug),
                    queued,
                });
            }
        }
    }
    if !solo.is_empty() {
        rows.push(TreeRow::SoloHeader);
        rows.extend(
            solo.iter()
                .filter_map(|pane_id| {
                    super::agent_sidebar::agent_row(snapshot, pane_id, config, None)
                })
                .map(TreeRow::Solo),
        );
    }
    Some(rows)
}

pub(super) fn render_tree(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[TreeRow],
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    super::agent_sidebar::render_agent_list(
        buffer,
        area,
        rows,
        None,
        config,
        agent_scroll,
        hits,
        TreeRow::lines,
        |buffer, rect, row, hits| match row {
            TreeRow::Swarm {
                slug,
                expanded,
                master_pane,
                ..
            } => {
                hits.swarm_headers
                    .push((rect, slug.clone(), *expanded, master_pane.clone()));
                render_swarm_header(buffer, rect, row, config);
            }
            TreeRow::Member { pane_id, .. } => {
                hits.agents.push((rect, pane_id.clone()));
                render_member(buffer, rect, row, config);
            }
            TreeRow::SoloHeader => {
                put_text(
                    buffer,
                    rect.x,
                    rect.y,
                    rect.width,
                    " solo",
                    Style::default()
                        .fg(config.palette.overlay0)
                        .add_modifier(Modifier::BOLD),
                );
            }
            TreeRow::Solo(agent) => {
                hits.agents.push((rect, agent.pane_id.clone()));
                super::agent_sidebar::render_agent_row(buffer, rect, agent, config);
            }
        },
    );
}

/// Writes `text` left to right and returns the next column.
fn put(buffer: &mut Buffer, x: u16, rect: Rect, text: &str, style: Style) -> u16 {
    if x >= rect.right() {
        return x;
    }
    put_text(buffer, x, rect.y, rect.right() - x, text, style);
    x.saturating_add(UnicodeWidthStr::width(text) as u16)
}

fn put_right(buffer: &mut Buffer, rect: Rect, min_x: u16, text: &str, style: Style) {
    let width = UnicodeWidthStr::width(text) as u16;
    let x = rect.right().saturating_sub(width.saturating_add(1));
    if x > min_x {
        put_text(buffer, x, rect.y, width, text, style);
    }
}

fn render_swarm_header(buffer: &mut Buffer, rect: Rect, row: &TreeRow, config: &ClientShellConfig) {
    let TreeRow::Swarm {
        slug,
        expanded,
        members,
        working,
        attention,
        queued,
        rollup,
        holds_focus,
        paused,
        ..
    } = row
    else {
        return;
    };
    let palette = &config.palette;
    if *holds_focus && !*expanded {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let mut x = rect.x;
    x = put(
        buffer,
        x,
        rect,
        if *expanded { " ▾ " } else { " ▸ " },
        Style::default().fg(palette.overlay0),
    );
    x = put(
        buffer,
        x,
        rect,
        status_icon(*rollup, config.status_indicators),
        Style::default().fg(status_color(*rollup, palette)),
    );
    x = put(
        buffer,
        x,
        rect,
        &format!(" {slug}"),
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD),
    );
    if *paused {
        x = put(buffer, x, rect, " ⏸", Style::default().fg(palette.yellow));
    }
    let mut summary = if *attention > 0 {
        format!("⚠{attention} ●{working}/{members}")
    } else {
        format!("●{working}/{members}")
    };
    if *queued > 0 {
        summary.push_str(&format!(" ✉{queued}"));
    }
    let color = if *attention > 0 {
        palette.yellow
    } else {
        palette.overlay0
    };
    put_right(buffer, rect, x, &summary, Style::default().fg(color));
}

fn render_member(buffer: &mut Buffer, rect: Rect, row: &TreeRow, config: &ClientShellConfig) {
    let TreeRow::Member {
        status,
        focused,
        role,
        name,
        queued,
        ..
    } = row
    else {
        return;
    };
    let palette = &config.palette;
    if *focused {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let mut x = rect.x;
    x = put(buffer, x, rect, "   ", Style::default());
    x = put(
        buffer,
        x,
        rect,
        status_icon(*status, config.status_indicators),
        Style::default().fg(status_color(*status, palette)),
    );
    x = put(buffer, x, rect, " ", Style::default());
    x = put(
        buffer,
        x,
        rect,
        role.glyph(),
        Style::default().fg(role.color(palette)),
    );
    let mut name_style = Style::default().fg(if *role == Role::Master {
        role.color(palette)
    } else {
        palette.subtext0
    });
    if *role == Role::Master || *focused {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    if *role == Role::Script {
        name_style = name_style.add_modifier(Modifier::DIM);
    }
    x = put(buffer, x, rect, &format!(" {name}"), name_style);
    if *queued > 0 {
        put_right(
            buffer,
            rect,
            x,
            &format!("✉{queued}"),
            Style::default().fg(palette.peach),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ClientShellAgent;

    fn agent(
        pane: &str,
        swarm: Option<&str>,
        role: &str,
        status: AgentStatus,
        focused: bool,
    ) -> ClientShellAgent {
        let mut tokens = vec![("role".to_string(), role.to_string())];
        if let Some(slug) = swarm {
            tokens.push(("swarm".to_string(), slug.to_string()));
        }
        ClientShellAgent {
            pane_id: format!("{}:{pane}", swarm.unwrap_or("solo")),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            name: Some(format!("{}-{pane}", swarm.unwrap_or("x"))),
            display_agent: Some(format!("◆ {pane}")),
            agent: Some("claude".into()),
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: status,
            state_change_seq: 0,
            state_labels: Vec::new(),
            tokens,
            focused,
        }
    }

    fn names(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                TreeRow::Swarm { slug, expanded, .. } => {
                    format!("{}{slug}", if *expanded { "▾" } else { "▸" })
                }
                TreeRow::Member { name, .. } => format!("  {name}"),
                TreeRow::SoloHeader => "solo".into(),
                TreeRow::Solo(row) => format!("  {}", row.pane_id),
            })
            .collect()
    }

    fn rows_for(agents: Vec<ClientShellAgent>, tree: &SwarmTreeState) -> Option<Vec<TreeRow>> {
        let mut snapshot = super::super::tests::snapshot();
        snapshot.agents = agents;
        snapshot.agent_order.clear();
        let config = ClientShellConfig::from_config(&crate::config::Config::default());
        tree_rows(&snapshot, &config, tree)
    }

    #[test]
    fn no_swarm_tokens_keeps_the_normal_list() {
        let rows = rows_for(
            vec![agent("a", None, "worker", AgentStatus::Idle, false)],
            &SwarmTreeState::default(),
        );
        assert!(rows.is_none());
    }

    #[test]
    fn collapsed_swarms_show_master_and_attention_only_focused_swarm_expands() {
        let rows = rows_for(
            vec![
                agent(
                    "scout",
                    Some("ideas"),
                    "worker",
                    AgentStatus::Working,
                    false,
                ),
                agent(
                    "critic",
                    Some("ideas"),
                    "critic",
                    AgentStatus::Blocked,
                    false,
                ),
                agent(
                    "coordinator",
                    Some("ideas"),
                    "master",
                    AgentStatus::Idle,
                    false,
                ),
                agent(
                    "backend",
                    Some("tonight"),
                    "worker",
                    AgentStatus::Working,
                    true,
                ),
                agent(
                    "coordinator",
                    Some("tonight"),
                    "master",
                    AgentStatus::Idle,
                    false,
                ),
                agent("solo1", None, "worker", AgentStatus::Idle, false),
            ],
            &SwarmTreeState::default(),
        )
        .unwrap();
        assert_eq!(
            names(&rows),
            [
                "▸ideas",
                "  coordinator",
                "  critic", // scout hidden: working, collapsed
                "▾tonight",
                "  coordinator",
                "  backend", // focused swarm expanded, master first
                "solo",
                "  solo:solo1",
            ]
        );
    }

    #[test]
    fn user_choice_overrides_focus() {
        let mut tree = SwarmTreeState::default();
        tree.set_expanded("ideas", true);
        tree.set_expanded("tonight", false);
        let rows = rows_for(
            vec![
                agent(
                    "scout",
                    Some("ideas"),
                    "worker",
                    AgentStatus::Working,
                    false,
                ),
                agent(
                    "coordinator",
                    Some("ideas"),
                    "master",
                    AgentStatus::Idle,
                    false,
                ),
                agent(
                    "backend",
                    Some("tonight"),
                    "worker",
                    AgentStatus::Working,
                    true,
                ),
            ],
            &tree,
        )
        .unwrap();
        assert_eq!(
            names(&rows),
            ["▾ideas", "  coordinator", "  scout", "▸tonight"]
        );
    }
}
