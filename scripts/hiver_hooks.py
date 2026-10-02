#!/usr/bin/env python3
"""One-shot P0 rename hooks for the hiver fork (idempotent). Kept for reapplying after upstream rebases."""
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent


def norm(text):
    """Whitespace-insensitive form, so hooks reformatted by cargo fmt still count as applied."""
    return re.sub(r"\s+", " ", text)


def edit(rel, old, new, count=1, required=True, marker=None):
    """Replace `old` with `new` once. Already applied when `new` (whitespace-insensitive) or
    `marker` is present; use a marker for hooks rustfmt may reflow beyond whitespace."""
    p = ROOT / rel
    s = p.read_text()
    if (marker and marker in s) or new in s or norm(new) in norm(s):
        return
    if old not in s:
        if required:
            raise SystemExit(f"{rel}: pattern not found: {old[:60]!r}")
        return
    p.write_text(s.replace(old, new, count))


def sub(rel, pattern, repl, count=0):
    p = ROOT / rel
    p.write_text(re.sub(pattern, repl, p.read_text(), count=count, flags=re.M))


# Package / binary name
sub("Cargo.toml", r'^name = "herdr"$', 'name = "hiver"', 1)
sub("Cargo.toml", r'^description = .*$', 'description = "terminal workspace for swarms of AI coding agents (fork of herdr)"', 1)
sub("Cargo.toml", r'^repository = .*$', 'repository = "https://github.com/jcsancho/hiver"', 1)
sub("Cargo.toml", r'^homepage = .*\n', '', 1)
for t in ROOT.joinpath("tests").rglob("*.rs"):
    s = t.read_text()
    new = s.replace("CARGO_BIN_EXE_herdr", "CARGO_BIN_EXE_hiver")
    # Tests hardcode the app dir name (see app_dir_name below).
    new = new.replace('"herdr-dev"', '"hiver-dev"').replace('"herdr-dev/', '"hiver-dev/')
    new = new.replace('"herdr/config.toml"', '"hiver/config.toml"')
    new = new.replace('join("herdr")', 'join("hiver")')
    new = re.sub(r'^(\s*)"herdr"$', r'\1"hiver"', new, flags=re.M)
    if new != s:
        t.write_text(new)
sub("justfile", r"--bin herdr ", "--bin hiver ")
sub("justfile", r'target\}/release/herdr"', 'target}/release/hiver"')
sub("justfile", r"cargo update -p herdr ", "cargo update -p hiver ")

# Own config / socket / session dirs
edit("src/config/io.rs",
     '''    if cfg!(debug_assertions) {
        "herdr-dev"
    } else {
        "herdr"
    }''',
     '''    // hiver: own config/socket/session dirs so hiver and herdr run side by side.
    if cfg!(debug_assertions) {
        "hiver-dev"
    } else {
        "hiver"
    }''')

# main: module, env isolation, branding
edit("src/main.rs", 'pub(crate) const HERDR_ENV_VALUE: &str = "1";\n',
     'pub(crate) const HERDR_ENV_VALUE: &str = "1";\nmod hiver;\n')
edit("src/main.rs", "fn main() -> io::Result<()> {\n    let raw_args",
     "fn main() -> io::Result<()> {\n    hiver::isolate_from_parent_herdr();\n    let raw_args")
edit("src/main.rs", 'println!("herdr {}", crate::build_info::version());',
     'println!("hiver {}", crate::build_info::version());')
edit("src/main.rs", 'println!("herdr — terminal workspace manager for AI coding agents");',
     'println!("hiver — terminal workspace for swarms of AI coding agents (fork of herdr)");')

# Panes get HIVER_ENV=1 next to HERDR_ENV=1
marker = "HIVER_ENV_VAR"
for rel in ["src/pane.rs", "src/pty/backend/unix.rs"]:
    p = ROOT / rel
    s = p.read_text()
    if marker not in s:
        s, n = re.subn(
            r"\n(\s*)cmd\.env\(crate::HERDR_ENV_VAR, crate::HERDR_ENV_VALUE\);",
            lambda m: m.group(0) + f"\n{m.group(1)}cmd.env(crate::hiver::HIVER_ENV_VAR, crate::HERDR_ENV_VALUE); // hiver",
            s, count=1)
        if n != 1:
            raise SystemExit(f"{rel}: pane env hook not found")
        p.write_text(s)

# No self-update / background checks against herdr.dev releases
edit("src/update.rs",
     "pub fn self_update(options: SelfUpdateOptions) -> Result<Version, String> {\n",
     "pub fn self_update(options: SelfUpdateOptions) -> Result<Version, String> {\n"
     "    // hiver: never download herdr releases over the hiver binary.\n"
     "    if !cfg!(test) {\n"
     "        let _ = options;\n"
     "        return Err(crate::hiver::SELF_UPDATE_DISABLED.into());\n"
     "    }\n")
edit("src/update.rs",
     "pub fn auto_update(events: tokio::sync::mpsc::Sender<crate::events::AppEvent>) {\n",
     "pub fn auto_update(events: tokio::sync::mpsc::Sender<crate::events::AppEvent>) {\n"
     "    // hiver: no background checks against herdr.dev release manifests.\n"
     "    if !cfg!(test) {\n"
     "        drop(events);\n"
     "        return;\n"
     "    }\n")

# `hiver update`: update from source (scripts/sync-herdr.sh) instead of herdr's release download
edit("src/main.rs",
     '    if args.get(1).map(|s| s.as_str()) == Some("update") {\n        let options',
     '    if args.get(1).map(|s| s.as_str()) == Some("update") {\n'
     "        // hiver: update from source (sync-herdr.sh), not herdr's release download.\n"
     "        std::process::exit(hiver::run_update(&args[2..]));\n"
     "        #[allow(unreachable_code)] // hiver: herdr's updater below stays for easy upstream merges\n"
     "        let options",
     marker="hiver::run_update")
edit("src/cli/spec.rs",
     '        .about("Download and install the latest version")\n'
     '        .arg(flag("handoff").help("Try live handoff after installing"))',
     "        .about(\n"
     '            "Update hiver to the latest version: build, install and live-hand-off running \\\n'
     '             sessions",\n'
     "        )\n"
     '        .arg(flag("check").help("Maintainer: merge herdr, test, ask, then publish and install"))\n'
     '        .arg(flag("yes").help("With --check: don\'t ask before publishing"))',
     marker="Update hiver to the latest version")
print("p0 rename hooks applied")

# ---------------------------------------------------------------------------
# Swarm engine hooks (src/swarm)
# ---------------------------------------------------------------------------
if "\nmod swarm;" not in (ROOT / "src/main.rs").read_text():
    edit("src/main.rs", "mod hiver;\n", "mod hiver;\nmod swarm;\n")
edit("src/api/schema.rs",
     '    #[serde(rename = "agent.view.set")]\n    AgentViewSet(AgentViewSetParams),\n',
     '    // hiver: swarm engine (src/swarm); one variant keeps upstream rebases small.\n'
     '    #[serde(rename = "swarm")]\n    Swarm(crate::swarm::SwarmParams),\n'
     '    #[serde(rename = "agent.view.set")]\n    AgentViewSet(AgentViewSetParams),\n')
edit("src/api/server.rs",
     '        Method::AgentViewSet(_) => "agent.view.set",\n',
     '        Method::Swarm(_) => "swarm", // hiver\n        Method::AgentViewSet(_) => "agent.view.set",\n')
edit("src/api/server.rs",
     "        method_body => {\n            let (response_write_tx, response_write_rx)",
     "        // hiver: swarm ops are answered by the swarm engine, not the app state machine.\n"
     "        Method::Swarm(params) => {\n"
     "            let response = crate::swarm::handle_request(&request_id, &params);\n"
     "            let result = write_text_line_allow_disconnect(&mut stream, &response);\n"
     "            if result.is_ok() {\n"
     "                crate::logging::api_request_completed(\n"
     "                    &request_id,\n"
     "                    method,\n"
     "                    api_response_outcome(&response),\n"
     "                    changes_ui,\n"
     "                );\n"
     "            }\n"
     "            result\n"
     "        }\n"
     "        method_body => {\n            let (response_write_tx, response_write_rx)")
edit("src/api/server.rs",
     "    let running = Arc::new(AtomicBool::new(true));\n    let listener_running = Arc::clone(&running);\n",
     "    let running = Arc::new(AtomicBool::new(true));\n"
     "    #[cfg(not(test))]\n"
     "    crate::swarm::start(api_tx.clone()); // hiver\n"
     "    let listener_running = Arc::clone(&running);\n")
edit("src/api/mod.rs",
     "pub type ApiRequestSender = mpsc::UnboundedSender<ApiRequestMessage>;\n",
     "pub type ApiRequestSender = mpsc::UnboundedSender<ApiRequestMessage>;\n\n"
     "/// hiver: in-process API calls for the swarm engine.\n"
     "pub(crate) fn dispatch_internal(\n"
     "    request: Request,\n"
     "    api_tx: &ApiRequestSender,\n"
     "    timeout: Option<std::time::Duration>,\n"
     ") -> String {\n"
     "    server::dispatch_to_app_with_timeout(request, api_tx, timeout)\n"
     "}\n")
print("swarm engine hooks applied")

# CLI: hiver swarm … / hiver msg …
edit("src/cli.rs", '        "agent" => agent::run_agent_command(&args[2..])?,\n',
     '        "agent" => agent::run_agent_command(&args[2..])?,\n'
     '        "swarm" => swarm::run_swarm_command(&args[2..])?, // hiver\n'
     '        "msg" => swarm::run_msg_command(&args[2..])?, // hiver\n',
     marker='"swarm" => swarm::run_swarm_command')
edit("src/cli.rs", '        "agent" => agent::run_agent_command(&args[2..])?,\n',
     '        "agent" => agent::run_agent_command(&args[2..])?,\n'
     '        "skill" => swarm::run_skill_command(&args[2..])?, // hiver\n',
     marker='"skill" => swarm::run_skill_command')
edit("src/cli.rs", '        "agent" => agent::run_agent_command(&args[2..])?,\n',
     '        "agent" => agent::run_agent_command(&args[2..])?,\n'
     '        "home" => swarm::run_home_command(&args[2..])?, // hiver\n',
     marker='"home" => swarm::run_home_command')
p = ROOT / "src/cli.rs"
s = p.read_text()
if "\nmod swarm;" not in s:
    s, n = re.subn(r"^(mod status;\n)", r"\1mod swarm; // hiver\n", s, count=1, flags=re.M)
    if n != 1:
        raise SystemExit("src/cli.rs: mod list anchor not found")
    p.write_text(s)
print("cli hooks applied")

# ---------------------------------------------------------------------------
# Swarm tree in the Agents panel (src/client/shell/swarm_sidebar.rs)
# ---------------------------------------------------------------------------
# rustfmt sorts `mod` lines, so check for the line anywhere rather than at the anchor.
if "mod swarm_sidebar;" not in (ROOT / "src/client/shell.rs").read_text():
    edit("src/client/shell.rs", "mod agent_sidebar;\n", "mod agent_sidebar;\nmod swarm_sidebar; // hiver\n")
edit("src/client/shell/state.rs",
     "    pub(super) agents: Vec<(Rect, String)>,\n    pub(super) endpoint_agents:",
     "    pub(super) agents: Vec<(Rect, String)>,\n"
     "    pub(super) swarm_headers: Vec<super::swarm_sidebar::SwarmHeaderHit>, // hiver\n"
     "    pub(super) endpoint_agents:")
edit("src/client/shell/state.rs",
     "    pub(super) agent_scroll: usize,\n    pub(super) pending_agent_reveal",
     "    pub(super) agent_scroll: usize,\n"
     "    pub(super) swarm_tree: super::swarm_sidebar::SwarmTreeState, // hiver\n"
     "    pub(super) pending_agent_reveal")
edit("src/client/shell/state.rs",
     "            agent_scroll: 0,\n            pending_agent_reveal: None,",
     "            agent_scroll: 0,\n            swarm_tree: Default::default(), // hiver\n            pending_agent_reveal: None,")
edit("src/client/shell/render.rs",
     "    pub(super) agent_scroll: &'a mut usize,\n",
     "    pub(super) agent_scroll: &'a mut usize,\n"
     "    pub(super) swarm_tree: &'a super::swarm_sidebar::SwarmTreeState, // hiver\n")
p = ROOT / "src/client/shell/composition.rs"
s = p.read_text()
if "swarm_tree: &self.swarm_tree" not in s:
    s, n = re.subn(r"\n(\s*)agent_scroll: &mut self\.agent_scroll,",
                   lambda m: m.group(0) + f"\n{m.group(1)}swarm_tree: &self.swarm_tree, // hiver", s)
    if n != 2:
        raise SystemExit(f"composition.rs: expected 2 render-state constructors, found {n}")
    p.write_text(s)
edit("src/client/shell/sidebar.rs",
     "        config,\n        state.agent_scroll,\n        hits,\n    );",
     "        config,\n        state.agent_scroll,\n        state.swarm_tree, // hiver\n        hits,\n    );")
edit("src/client/shell/agent_sidebar.rs",
     """    agent_scroll: &mut usize,
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
""",
     """    agent_scroll: &mut usize,
    swarm_tree: &super::swarm_sidebar::SwarmTreeState,
    hits: &mut ShellHitMap,
) {
    // hiver: swarm tree whenever an agent belongs to a swarm.
    let swarm_rows = super::swarm_sidebar::tree_rows(snapshot, config, swarm_tree);
    let label = snapshot
        .agent_view_label
        .as_deref()
        .or(swarm_rows.as_ref().map(|_| "swarms"));
    if !render_agent_panel_header(buffer, area, label, config, hits) {
        return;
    }
    if let Some(rows) = swarm_rows {
        super::swarm_sidebar::render_tree(buffer, area, &rows, config, agent_scroll, hits);
        return;
    }
""")
edit("src/client/shell/mouse.rs",
     """                let agent_pane_id = self
                    .hits
                    .agents
                    .iter()
                    .find(|(rect, _)| super::contains(*rect, point))""",
     """                // hiver: swarm headers; the chevron toggles, the rest focuses the master.
                let swarm_header = self
                    .hits
                    .swarm_headers
                    .iter()
                    .find(|(rect, ..)| super::contains(*rect, point))
                    .cloned();
                if let Some((rect, slug, expanded, master)) = swarm_header {
                    match master.filter(|_| point.0 >= rect.x.saturating_add(3)) {
                        Some(pane_id) => {
                            self.swarm_tree.set_expanded(&slug, true);
                            self.push_endpoint_method(
                                crate::api::schema::Method::PaneFocus(
                                    crate::api::schema::PaneTarget { pane_id },
                                ),
                                outcome,
                            );
                        }
                        None => self.swarm_tree.set_expanded(&slug, !expanded),
                    }
                    outcome.repaint = true;
                    return;
                }
                let agent_pane_id = self
                    .hits
                    .agents
                    .iter()
                    .find(|(rect, _)| super::contains(*rect, point))""")
print("swarm sidebar hooks applied")

# ---------------------------------------------------------------------------
# Role-colored pane border titles
# ---------------------------------------------------------------------------
edit("src/metadata_tokens.rs",
     "    pub(crate) fn values(&self) -> HashMap<String, String> {",
     "    /// hiver: one value without allocating (used per pane while rendering borders).\n"
     "    pub(crate) fn get(&self, key: &str) -> Option<&str> {\n"
     "        self.entries.get(key).map(|token| token.value.as_str())\n"
     "    }\n\n"
     "    pub(crate) fn values(&self) -> HashMap<String, String> {")
edit("src/ui/panes.rs",
     """        let color = if info.is_focused {
            app.palette.accent
        } else {
            app.palette.overlay0
        };
        let mut style = Style::default().fg(color);
        if info.is_focused {
            style = style.add_modifier(Modifier::BOLD);
        }
        buf.set_stringn(
            start_x,""",
     """        // hiver: swarm panes take their role color (master, worker, critic, script).
        let role_color = ws
            .pane_state(info.id)
            .and_then(|pane| app.terminals.get(&pane.attached_terminal_id))
            .and_then(|terminal| terminal.metadata_tokens.get("role"))
            .and_then(|role| crate::swarm::model::role_color(role, &app.palette));
        let color = role_color.unwrap_or(if info.is_focused {
            app.palette.accent
        } else {
            app.palette.overlay0
        });
        let mut style = Style::default().fg(color);
        if info.is_focused {
            style = style.add_modifier(Modifier::BOLD);
        }
        buf.set_stringn(
            start_x,""")
print("border color hooks applied")

# ---------------------------------------------------------------------------
# Sidebar: "swarms" list, no agents panel, swarm summary row, click focuses master
# ---------------------------------------------------------------------------
edit("src/client/shell/sidebar.rs",
     """    let (workspace_area, detail_area) =
        crate::ui::expanded_sidebar_sections(area, state.sidebar_section_split);
    hits.sidebar_section_divider =
        crate::ui::sidebar_section_divider_rect(area, state.sidebar_section_split);
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        " spaces",""",
     """    // hiver: with ui.swarm_sidebar the list is titled "swarms" and uses the full height; the
    // agents panel is hidden (agents are the panes on the right).
    let (workspace_area, detail_area) = if config.swarm_sidebar {
        (
            Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height),
            Rect::default(),
        )
    } else {
        crate::ui::expanded_sidebar_sections(area, state.sidebar_section_split)
    };
    hits.sidebar_section_divider = if config.swarm_sidebar {
        Rect::default()
    } else {
        crate::ui::sidebar_section_divider_rect(area, state.sidebar_section_split)
    };
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        if config.swarm_sidebar { " swarms" } else { " spaces" }, // hiver""")
edit("src/config/sidebar.rs",
     """            rows: vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![SpaceSidebarToken::Branch, SpaceSidebarToken::GitStatus],
            ],
            row_gap: DEFAULT_SIDEBAR_ROW_GAP,""",
     """            rows: vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                // hiver: `$swarm` is the swarm summary (●working/total ⚠ ✉ ⏸) the engine reports.
                vec![
                    SpaceSidebarToken::Custom("swarm".into()),
                    SpaceSidebarToken::Branch,
                    SpaceSidebarToken::GitStatus,
                ],
            ],
            row_gap: DEFAULT_SIDEBAR_ROW_GAP,""")
edit("src/config/sidebar.rs",
     """        assert_eq!(
            config.spaces.rows,
            vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![SpaceSidebarToken::Branch, SpaceSidebarToken::GitStatus],
            ]
        );""",
     """        assert_eq!(
            config.spaces.rows,
            vec![
                vec![SpaceSidebarToken::StateIcon, SpaceSidebarToken::Workspace],
                vec![
                    SpaceSidebarToken::Custom("swarm".into()),
                    SpaceSidebarToken::Branch,
                    SpaceSidebarToken::GitStatus,
                ],
            ]
        );""")
edit("src/client/shell/endpoint_navigation.rs",
     """        outcome: &mut ClientShellInput,
    ) {
        self.focus_or_activate(
            press.endpoint_id,
            ClientEndpointFocusTarget::Workspace(press.workspace_id),
            outcome,
        );
    }""",
     """        outcome: &mut ClientShellInput,
    ) {
        // hiver: clicking a swarm's space focuses its master, ready to talk to.
        if let Some(pane_id) = self.swarm_master_pane(&press.endpoint_id, &press.workspace_id) {
            self.push_endpoint_method(
                crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget { pane_id }),
                outcome,
            );
            return;
        }
        self.focus_or_activate(
            press.endpoint_id,
            ClientEndpointFocusTarget::Workspace(press.workspace_id),
            outcome,
        );
    }

    /// hiver: the master pane the swarm engine reports on a swarm's space.
    fn swarm_master_pane(&self, endpoint_id: &ClientEndpointId, workspace_id: &str) -> Option<String> {
        if !endpoint_id.is_local() {
            return None;
        }
        self.endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)?
            .snapshot
            .as_deref()?
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)?
            .tokens
            .iter()
            .find(|(key, _)| key == "master_pane")
            .map(|(_, value)| value.clone())
    }""", marker="fn swarm_master_pane")
print("swarm list sidebar hooks applied")

# ui.swarm_sidebar setting (default off: herdr behavior; `hiver swarm setup` turns it on)
edit("src/config/model.rs",
     """    /// Agent sidebar ordering. Saved values are "spaces" or "priority". Default: "spaces".
    pub agent_panel_sort: AgentPanelSortConfig,""",
     """    /// Agent sidebar ordering. Saved values are "spaces" or "priority". Default: "spaces".
    pub agent_panel_sort: AgentPanelSortConfig,
    /// hiver: swarm-style sidebar. The space list is titled "swarms" and fills the sidebar;
    /// the agents panel is hidden (agents are the panes). Default: false.
    pub swarm_sidebar: bool,""")
edit("src/config/model.rs",
     """            agent_panel_sort: AgentPanelSortConfig::Spaces,
""",
     """            agent_panel_sort: AgentPanelSortConfig::Spaces,
            swarm_sidebar: false, // hiver
""")
edit("src/client/shell/state.rs",
     """    pub(super) agent_panel_sort: crate::config::AgentPanelSortConfig,
""",
     """    pub(super) agent_panel_sort: crate::config::AgentPanelSortConfig,
    pub(super) swarm_sidebar: bool, // hiver
""")
edit("src/client/shell/config.rs",
     """            agent_panel_sort: config.ui.agent_panel_sort,
""",
     """            agent_panel_sort: config.ui.agent_panel_sort,
            swarm_sidebar: config.ui.swarm_sidebar, // hiver
""")
edit("src/client/shell/config.rs",
     """                self.agent_panel_sort = ui.agent_panel_sort;
""",
     """                self.agent_panel_sort = ui.agent_panel_sort;
                self.swarm_sidebar = ui.swarm_sidebar; // hiver
""")
print("swarm sidebar setting hooks applied")

# Swarm info card while hovering a swarm row (swarm_sidebar::hover_card)
edit("src/client/shell/mouse.rs",
     """        self.update_link_hover(mouse, outcome);
        let point = (mouse.column, mouse.row);""",
     """        self.update_link_hover(mouse, outcome);
        let point = (mouse.column, mouse.row);
        // hiver: hovering a swarm row shows its info card.
        if mouse.kind == MouseEventKind::Moved {
            let hovered = self
                .hits
                .workspaces
                .iter()
                .find(|hit| super::contains(hit.rect, point))
                .map(|hit| hit.workspace_id.clone());
            if self.swarm_tree.set_hover(hovered) {
                outcome.repaint = true;
            }
        }""", marker="self.swarm_tree.set_hover(hovered)")
edit("src/client/shell/composition.rs",
     """            frame.replace_from_ratatui_buffer_preserving_effects(&composed, cursor);
        }
        self.hits.popup = None;""",
     """            frame.replace_from_ratatui_buffer_preserving_effects(&composed, cursor);
        }
        // hiver: swarm info card while hovering a swarm row, drawn over the panes.
        if let Some(card) = super::swarm_sidebar::hover_card(&self.swarm_tree, &self.hits, snapshot) {
            if let Some(mut composed) = frame.to_ratatui_buffer() {
                let cursor = frame.cursor.clone();
                occlusion.cover(super::swarm_sidebar::render_hover_card(&mut composed, &card, &self.config));
                frame.replace_from_ratatui_buffer_preserving_effects(&composed, cursor);
            }
        }
        self.hits.popup = None;""", marker="swarm_sidebar::hover_card(")
print("swarm info hover card hooks applied")

# Double-click a pane's title bar: zoom it to full size, and back
edit("src/client/shell/mouse.rs",
     """            if self.swarm_tree.set_hover(hovered) {
                outcome.repaint = true;
            }
        }""",
     """            if self.swarm_tree.set_hover(hovered) {
                outcome.repaint = true;
            }
        }
        // hiver: double-click a pane's title bar (its top border) to zoom it, and back.
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            let title_bar = self.hits.panes.iter().find(|hit| {
                !hit.popup
                    && point.1 == hit.rect.y
                    && point.0 >= hit.rect.x
                    && point.0 < hit.rect.right()
                    && !super::contains(hit.inner_rect, point)
            });
            if let Some(pane_id) = title_bar.map(|hit| hit.pane_id.clone()) {
                if self.swarm_tree.title_double_click(&pane_id) {
                    self.push_endpoint_method(
                        crate::api::schema::Method::PaneZoom(crate::api::schema::PaneZoomParams {
                            pane_id: Some(pane_id),
                            mode: crate::api::schema::PaneZoomMode::Toggle,
                        }),
                        outcome,
                    );
                    outcome.repaint = true;
                    return;
                }
            }
        }""", marker="self.swarm_tree.title_double_click(")
print("title-bar double-click zoom hooks applied")
