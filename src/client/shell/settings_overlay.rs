use super::*;

fn choice_style(selected: bool, palette: &Palette) -> Style {
    if selected {
        Style::default()
            .fg(contrast(palette))
            .bg(palette.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text).bg(palette.panel_bg)
    }
}

fn draw_choice(
    buffer: &mut Buffer,
    rect: Rect,
    label: &str,
    selected: bool,
    current: bool,
    palette: &Palette,
) {
    let style = choice_style(selected, palette);
    buffer.set_style(rect, style);
    let marker = if selected { "▸" } else { " " };
    let current = if current { " ✓" } else { "" };
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        &format!(" {marker} {label}{current}"),
        style,
    );
}

pub(super) fn render_settings_overlay(
    buffer: &mut Buffer,
    settings: &ClientSettingsOverlay,
    integration_updates_available: bool,
    palette: &Palette,
) -> Option<OverlayRender> {
    let integration_height = 14u16
        .saturating_add(settings.integrations.len().max(1) as u16)
        .saturating_add(settings.integration_messages.len().min(6) as u16);
    // hivey: larger than herdr's 76×22 so every tab and the longer hivey lists (voice,
    // plugins, skills) fit; `popup` still shrinks it to small terminals.
    let height = if settings.section == ClientSettingsSection::Integrations {
        integration_height.max(30)
    } else {
        30
    };
    let popup = popup(buffer.area, 110, height)?;
    let inner = panel(buffer, popup, palette.accent, palette.panel_bg)?;
    if inner.width < 20 || inner.height < 8 {
        return None;
    }

    put_text(
        buffer,
        inner.x,
        inner.y,
        inner.width,
        " settings",
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );

    let integration_badge = integration_updates_available
        || settings
            .integrations
            .iter()
            .any(|integration| integration.state == crate::api::schema::IntegrationState::Outdated);
    let mut tab_x = inner.x;
    let mut tab_hits = Vec::new();
    for section in ClientSettingsSection::ALL {
        let badge = *section == ClientSettingsSection::Integrations && integration_badge;
        let label = if badge {
            format!(" ● {} ", section.label())
        } else {
            format!(" {} ", section.label())
        };
        let width = display_width(&label).min(inner.right().saturating_sub(tab_x));
        let rect = Rect::new(tab_x, inner.y + 1, width, 1);
        let active = *section == settings.section;
        let style = if active {
            Style::default()
                .fg(contrast(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.overlay1).bg(palette.panel_bg)
        };
        buffer.set_style(rect, style);
        put_text(buffer, rect.x, rect.y, rect.width, &label, style);
        if badge && !active {
            put_text(
                buffer,
                rect.x.saturating_add(1),
                rect.y,
                rect.width.saturating_sub(1).min(2),
                "● ",
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.panel_bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
        tab_hits.push((rect, *section));
        tab_x = tab_x.saturating_add(width.saturating_add(1));
        if tab_x >= inner.right() {
            break;
        }
    }
    put_text(
        buffer,
        inner.x,
        inner.y + 2,
        inner.width,
        &"─".repeat(inner.width as usize),
        Style::default().fg(palette.surface0).bg(palette.panel_bg),
    );

    let content = Rect::new(
        inner.x,
        inner.y + 4,
        inner.width,
        inner.height.saturating_sub(7),
    );
    let mut choice_hits = Vec::new();
    match settings.section {
        ClientSettingsSection::Theme => {
            let visible = usize::from(content.height);
            let scroll = settings.selected.saturating_sub(visible.saturating_sub(1));
            for (visible_index, (index, name)) in crate::config::THEME_NAMES
                .iter()
                .enumerate()
                .skip(scroll)
                .take(visible)
                .enumerate()
            {
                let rect = Rect::new(
                    content.x,
                    content.y + visible_index as u16,
                    content.width,
                    1,
                );
                draw_choice(
                    buffer,
                    rect,
                    name,
                    index == settings.selected,
                    super::super::settings::normalized_theme_name(name)
                        == super::super::settings::normalized_theme_name(
                            &settings.original_theme_name,
                        ),
                    palette,
                );
                choice_hits.push((rect, index));
            }
        }
        ClientSettingsSection::Indicators => {
            render_choice_section(
                buffer,
                content,
                "agent status indicators",
                "choose color dots or distinct symbols for each state",
                &["color dots  ● ● ● ○ ·", "distinct symbols  × ◐ ✓ ○ ·"],
                settings.selected,
                None,
                palette,
                &mut choice_hits,
            );
        }
        ClientSettingsSection::Sound => {
            render_choice_section(
                buffer,
                content,
                "sound alerts",
                "play sounds when agents change state in background",
                &["on", "off"],
                settings.selected,
                None,
                palette,
                &mut choice_hits,
            );
        }
        ClientSettingsSection::Toast => {
            render_choice_section(
                buffer,
                content,
                "notification popups",
                "choose where background popup notifications should appear",
                &["off", "inside herdr", "via terminal", "via system"],
                settings.selected,
                None,
                palette,
                &mut choice_hits,
            );
        }
        ClientSettingsSection::Integrations => {
            render_integrations(buffer, content, settings, palette);
        }
        ClientSettingsSection::Voice => {
            render_voice(buffer, content, settings, palette, &mut choice_hits);
        }
        ClientSettingsSection::Pets => {
            render_pets(buffer, content, settings, palette, &mut choice_hits);
        }
        ClientSettingsSection::Plugins => {
            render_plugins(buffer, content, settings, palette, &mut choice_hits);
        }
        ClientSettingsSection::Skills => {
            render_skills(buffer, content, settings, palette, &mut choice_hits);
        }
    }

    let installable = settings
        .integrations
        .iter()
        .any(super::super::settings::integration_needs_install);
    let show_primary = settings.section != ClientSettingsSection::Integrations || installable;
    let labels = if show_primary { vec![10, 12] } else { vec![12] };
    let buttons = row(inner, &labels, 2, inner.height.saturating_sub(1));
    let (primary, close) = if show_primary {
        let primary = buttons[0];
        button(
            buffer,
            primary,
            if settings.section == ClientSettingsSection::Integrations {
                " ↵ install "
            } else {
                " ↵ apply "
            },
            Style::default()
                .fg(contrast(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );
        (primary, buttons[1])
    } else {
        (Rect::default(), buttons[0])
    };
    button(
        buffer,
        close,
        " esc close ",
        Style::default()
            .fg(palette.text)
            .bg(palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
    put_text(
        buffer,
        inner.x,
        inner.bottom().saturating_sub(2),
        inner.width,
        " ↑↓ select  tab section",
        Style::default().fg(palette.overlay1).bg(palette.panel_bg),
    );

    Some(OverlayRender {
        area: popup,
        primary,
        cancel: close,
        settings_popup: popup,
        settings_tabs: tab_hits,
        settings_choices: choice_hits,
        ..OverlayRender::default()
    })
}

fn render_choice_section(
    buffer: &mut Buffer,
    area: Rect,
    title: &str,
    description: &str,
    choices: &[&str],
    selected: usize,
    current: Option<usize>,
    palette: &Palette,
    hits: &mut Vec<(Rect, usize)>,
) {
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        title,
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        description,
        Style::default().fg(palette.overlay1).bg(palette.panel_bg),
    );
    let row_gap = u16::from(choices.len() > 2);
    for (index, choice) in choices.iter().enumerate() {
        let y = area.y + 3 + index as u16 * (1 + row_gap);
        if y >= area.bottom() {
            break;
        }
        let rect = Rect::new(area.x, y, area.width, 1);
        draw_choice(
            buffer,
            rect,
            choice,
            index == selected,
            current == Some(index),
            palette,
        );
        hits.push((rect, index));
    }
}

/// hivey: who reads agents' spoken summaries: one row per provider and voice, then off
/// (✓ the one used), then the volume levels side by side. Rows have no gap so the list fits.
fn render_voice(
    buffer: &mut Buffer,
    area: Rect,
    settings: &ClientSettingsOverlay,
    palette: &Palette,
    hits: &mut Vec<(Rect, usize)>,
) {
    use crate::swarm::voice;
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        "voice",
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        "who reads agents' summaries aloud (the Claude Code hook and the pet)",
        Style::default().fg(palette.overlay1).bg(palette.panel_bg),
    );
    let current = voice::choice_index(settings.voice_provider, settings.voice_current.as_deref());
    let rows: Vec<(String, bool)> = voice::choices()
        .iter()
        .enumerate()
        .map(|(index, (provider, name, about))| {
            let missing = if provider.available() {
                ""
            } else {
                " (not installed)"
            };
            (
                format!(
                    "{:<4} {:<11} {about}{missing}",
                    provider.id(),
                    name.unwrap_or("")
                ),
                current == Some(index),
            )
        })
        .collect();
    let mut y = area.y + 3;
    for (index, (label, current)) in rows.iter().enumerate() {
        if y >= area.bottom() {
            return;
        }
        let rect = Rect::new(area.x, y, area.width, 1);
        draw_choice(
            buffer,
            rect,
            label,
            index == settings.selected,
            *current,
            palette,
        );
        hits.push((rect, index));
        y += 1;
    }
    // Volume levels: one line of buttons, numbered after the voice rows.
    y += 1;
    if y + 1 >= area.bottom() {
        return;
    }
    put_text(
        buffer,
        area.x,
        y,
        area.width,
        "volume  louder to hear it over music (1 is normal; high levels may distort)",
        Style::default().fg(palette.overlay1).bg(palette.panel_bg),
    );
    y += 1;
    let current_volume = voice::volume_index(settings.voice_volume);
    let mut x = area.x;
    for (level, volume) in voice::VOLUME_LEVELS.iter().enumerate() {
        let label = if voice::is_normal_volume(*volume) {
            "1 normal".to_string()
        } else {
            volume.to_string()
        };
        // " ▸ " + label + " ✓" + a space between buttons
        let width = label.chars().count() as u16 + 6;
        if x + width > area.right() {
            break;
        }
        let rect = Rect::new(x, y, width - 1, 1);
        let index = rows.len() + level;
        draw_choice(
            buffer,
            rect,
            &label,
            index == settings.selected,
            current_volume == Some(level),
            palette,
        );
        hits.push((rect, index));
        x += width;
    }
    if current_volume.is_none() {
        put_text(
            buffer,
            x,
            y,
            area.right().saturating_sub(x),
            &format!(" now {}", settings.voice_volume),
            Style::default().fg(palette.accent).bg(palette.panel_bg),
        );
    }
    y += 1;
    let mut lines = Vec::new();
    if let Some(message) = &settings.voice_message {
        lines.push((message.clone(), palette.accent));
    }
    lines.push((
        "more voices: hivey voice list · quiet hours: hivey voice quiet 22-8 · any volume: \
         hivey voice volume 1.7"
            .to_string(),
        palette.overlay1,
    ));
    y += 1;
    for (text, color) in lines {
        if y >= area.bottom() {
            break;
        }
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            &format!(" {text}"),
            Style::default().fg(color).bg(palette.panel_bg),
        );
        y += 1;
    }
}

/// hivey: the desktop pet, one of the hivey pets or none (✓ marks the current one).
fn render_pets(
    buffer: &mut Buffer,
    area: Rect,
    settings: &ClientSettingsOverlay,
    palette: &Palette,
    hits: &mut Vec<(Rect, usize)>,
) {
    use super::super::settings::{PETS_SUPPORTED, PET_CHOICES};
    if !PETS_SUPPORTED {
        render_choice_section(
            buffer,
            area,
            "desktop pet",
            "hivey pets are macOS apps; there is none for this computer",
            &[],
            0,
            None,
            palette,
            hits,
        );
        return;
    }
    let labels: Vec<String> = PET_CHOICES
        .iter()
        .map(|(label, _, about)| {
            if about.is_empty() {
                (*label).to_string()
            } else {
                format!("{label:<13} {about}")
            }
        })
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let current = PET_CHOICES
        .iter()
        .position(|(_, id, _)| *id == settings.pet_current.as_deref());
    render_choice_section(
        buffer,
        area,
        "desktop pet",
        "acts out what your agents do · right-click the pet for more",
        &labels,
        settings.selected,
        current,
        palette,
        hits,
    );
    if let Some(message) = &settings.pet_message {
        let y = area.y + 3 + PET_CHOICES.len() as u16 * 2;
        if y < area.bottom() {
            put_text(
                buffer,
                area.x,
                y,
                area.width,
                &format!(" {message}"),
                Style::default().fg(palette.accent).bg(palette.panel_bg),
            );
        }
    }
}

/// hivey: the swarm and agent creators; ✓ marks the one `hivey swarm new` uses for each kind.
fn render_plugins(
    buffer: &mut Buffer,
    area: Rect,
    settings: &ClientSettingsOverlay,
    palette: &Palette,
    hits: &mut Vec<(Rect, usize)>,
) {
    let (swarm, agent) = &settings.creator_current;
    let mark = |current: &Option<String>, id: &str| {
        if current.as_deref() == Some(id) {
            "✓"
        } else {
            " "
        }
    };
    let labels: Vec<String> = settings
        .creators
        .iter()
        .map(|creator| {
            let current = if creator.agent { agent } else { swarm };
            let kind = if creator.agent { "agent" } else { "swarm" };
            format!(
                "{} {kind}   {:<34} {}",
                mark(current, &creator.id),
                creator.name,
                creator.id
            )
        })
        .chain(settings.skill_providers.iter().map(|provider| {
            format!(
                "{} skills  {:<34} {}",
                mark(&settings.skill_provider_current, &provider.id),
                provider.name,
                provider.id
            )
        }))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let description = if labels.is_empty() {
        "no creators installed: hivey plugin link <hivey repo>/plugins/swarm-creator"
    } else {
        "↵ picks what new swarms / agents use (✓) · hivey's built-in ones when none is picked"
    };
    render_choice_section(
        buffer,
        area,
        "swarm and agent creators, skills",
        description,
        &labels,
        settings.selected,
        None,
        palette,
        hits,
    );
    let row_gap = u16::from(labels.len() > 2);
    let mut y = area.y + 3 + labels.len() as u16 * (1 + row_gap);
    let mut lines = Vec::new();
    if let Some(message) = &settings.creator_message {
        lines.push((message.clone(), palette.accent));
    }
    lines.push((
        "install more: hivey plugin install OWNER/REPO[/DIR] · hivey plugin link <folder>"
            .to_string(),
        palette.overlay1,
    ));
    for (text, color) in lines {
        if y >= area.bottom() {
            break;
        }
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            &format!(" {text}"),
            Style::default().fg(color).bg(palette.panel_bg),
        );
        y += 1;
    }
}

/// hivey: online skill search; the library is skylls.
fn render_skills(
    buffer: &mut Buffer,
    area: Rect,
    settings: &ClientSettingsOverlay,
    palette: &Palette,
    hits: &mut Vec<(Rect, usize)>,
) {
    let labels = [format!(
        "{} online   search skills.sh for skills skylls lacks (asks first)",
        if settings.skills_online { "✓" } else { " " }
    )];
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    render_choice_section(
        buffer,
        area,
        "skills for new agents",
        "where creators pick each agent's skills · installed in the agent's folder, never global",
        &labels,
        settings.selected,
        None,
        palette,
        hits,
    );
    let row_gap = u16::from(labels.len() > 2);
    let mut y = area.y + 3 + labels.len() as u16 * (1 + row_gap);
    let mut lines = Vec::new();
    if let Some(message) = &settings.skills_message {
        lines.push((message.clone(), palette.accent));
    }
    lines.push(if settings.skylls_installed {
        (
            "library: skylls, your published skills and friends' (skylls find <words>)".to_string(),
            palette.overlay1,
        )
    } else {
        (
            "library: skylls is not installed (hivey skills shows how to install it)".to_string(),
            palette.accent,
        )
    });
    for (text, color) in lines {
        if y >= area.bottom() {
            break;
        }
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            &format!(" {text}"),
            Style::default().fg(color).bg(palette.panel_bg),
        );
        y += 1;
    }
}

fn render_integrations(
    buffer: &mut Buffer,
    area: Rect,
    settings: &ClientSettingsOverlay,
    palette: &Palette,
) {
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        "agent integrations",
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        "enable session restore and, where supported, direct status updates",
        Style::default().fg(palette.overlay1).bg(palette.panel_bg),
    );
    if settings.loading_integrations {
        put_text(
            buffer,
            area.x,
            area.y + 3,
            area.width,
            " loading integrations…",
            Style::default().fg(palette.overlay1).bg(palette.panel_bg),
        );
        return;
    }
    if settings.integrations.is_empty() {
        put_text(
            buffer,
            area.x,
            area.y + 3,
            area.width,
            " no integration targets available",
            Style::default().fg(palette.overlay1).bg(palette.panel_bg),
        );
        return;
    }
    for (index, integration) in settings.integrations.iter().enumerate() {
        let y = area.y + 3 + index as u16;
        if y >= area.bottom() {
            break;
        }
        let (marker, color, status) = match integration.state {
            crate::api::schema::IntegrationState::Current => ("✓", palette.green, "installed"),
            crate::api::schema::IntegrationState::Outdated => {
                ("↻", palette.yellow, "update available")
            }
            crate::api::schema::IntegrationState::NotInstalled if integration.available => {
                ("+", palette.accent, "available")
            }
            crate::api::schema::IntegrationState::NotInstalled => {
                ("–", palette.overlay0, "not found")
            }
        };
        put_text(
            buffer,
            area.x,
            y,
            3,
            &format!(" {marker}"),
            Style::default().fg(color).bg(palette.panel_bg),
        );
        put_text(
            buffer,
            area.x + 3,
            y,
            11.min(area.width.saturating_sub(3)),
            &format!("{:<9}", integration.label),
            Style::default().fg(palette.subtext0).bg(palette.panel_bg),
        );
        put_text(
            buffer,
            area.x + 14,
            y,
            area.width.saturating_sub(14),
            status,
            Style::default().fg(palette.overlay1).bg(palette.panel_bg),
        );
    }
    let message_y = area
        .y
        .saturating_add(4)
        .saturating_add(settings.integrations.len() as u16);
    for (offset, message) in settings.integration_messages.iter().take(6).enumerate() {
        let y = message_y.saturating_add(offset as u16);
        if y >= area.bottom() {
            break;
        }
        put_text(
            buffer,
            area.x,
            y,
            area.width,
            &format!(" {message}"),
            Style::default().fg(palette.overlay1).bg(palette.panel_bg),
        );
    }
    if settings.installing_integrations && message_y < area.bottom() {
        put_text(
            buffer,
            area.x,
            message_y,
            area.width,
            " installing…",
            Style::default().fg(palette.overlay1).bg(palette.panel_bg),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice_settings(volume: f64, selected: usize) -> ClientSettingsOverlay {
        let config = ClientShellConfig::from_config(&crate::config::Config::default());
        ClientSettingsOverlay {
            section: ClientSettingsSection::Voice,
            selected,
            original_theme_name: String::new(),
            original_palette: config.palette,
            integrations: Vec::new(),
            integration_messages: Vec::new(),
            loading_integrations: false,
            installing_integrations: false,
            voice_provider: crate::swarm::voice::Provider::Tts,
            voice_current: Some("am_michael".into()),
            voice_volume: volume,
            voice_message: None,
            pet_current: None,
            pet_message: None,
            creators: Vec::new(),
            creator_current: (None, None),
            creator_message: None,
            skill_providers: Vec::new(),
            skill_provider_current: None,
            skylls_installed: false,
            skills_online: true,
            skills_message: None,
        }
    }

    fn screen(buffer: &Buffer) -> String {
        let area = buffer.area;
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn voice_tab_offers_clickable_volume_levels_after_the_voices() {
        use crate::swarm::voice;
        let voices = voice::choices().len();
        let selected = voices + 2; // the 1.5 button
        let settings = voice_settings(1.5, selected);
        let palette = settings.original_palette.clone();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
        let rendered = render_settings_overlay(&mut buffer, &settings, false, &palette)
            .expect("the settings popup fits");
        let text = screen(&buffer);
        if std::env::var_os("HIVEY_SHOW_SETTINGS").is_some() {
            println!("{text}");
        }
        assert!(text.contains("volume  louder"), "{text}");
        assert!(text.contains("1 normal"), "{text}");
        assert!(text.contains("▸ 1.5 ✓"), "selected and current: {text}");
        let indices: Vec<usize> = rendered
            .settings_choices
            .iter()
            .map(|(_, index)| *index)
            .collect();
        let expected: Vec<usize> = (0..voices + voice::VOLUME_LEVELS.len()).collect();
        assert_eq!(
            indices, expected,
            "every voice and volume level is clickable"
        );
        // Volume buttons share one line.
        let rows: Vec<u16> = rendered.settings_choices[voices..]
            .iter()
            .map(|(rect, _)| rect.y)
            .collect();
        assert!(rows.windows(2).all(|pair| pair[0] == pair[1]), "{rows:?}");
    }

    #[test]
    fn a_volume_set_by_hand_is_shown_next_to_the_levels() {
        let settings = voice_settings(1.7, 0);
        let palette = settings.original_palette.clone();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
        render_settings_overlay(&mut buffer, &settings, false, &palette).expect("fits");
        assert!(screen(&buffer).contains("now 1.7"));
    }
}
