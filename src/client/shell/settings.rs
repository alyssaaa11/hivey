use super::*;
use crossterm::event::{KeyCode, KeyModifiers};

pub(super) fn normalized_theme_name(name: &str) -> String {
    name.to_lowercase().replace([' ', '_'], "-")
}

fn theme_index(name: &str) -> usize {
    let normalized = normalized_theme_name(name);
    crate::config::THEME_NAMES
        .iter()
        .position(|candidate| normalized_theme_name(candidate) == normalized)
        .unwrap_or(0)
}

fn indicator_index(style: crate::config::StatusIndicatorStyle) -> usize {
    usize::from(style == crate::config::StatusIndicatorStyle::Symbols)
}

fn toast_index(delivery: crate::config::ToastDelivery) -> usize {
    match delivery {
        crate::config::ToastDelivery::Off => 0,
        crate::config::ToastDelivery::Herdr => 1,
        crate::config::ToastDelivery::Terminal => 2,
        crate::config::ToastDelivery::System => 3,
    }
}

pub(super) fn integration_needs_install(info: &crate::api::schema::IntegrationInfo) -> bool {
    info.state == crate::api::schema::IntegrationState::Outdated
        || info.available && info.state == crate::api::schema::IntegrationState::NotInstalled
}

/// hivey pets (macOS apps built from the hivey repo's `pets/`): label, id (`None`: no pet),
/// and a short description.
pub(super) const PET_CHOICES: &[(&str, Option<&str>, &str)] = &[
    (
        "Hivey",
        Some("hivey-h"),
        "a 3D H; its sections slide apart to hand out work",
    ),
    (
        "Hivey System",
        Some("hivey-dot"),
        "a dot; its agents ride a signal wave around it",
    ),
    (
        "Hivey Prompt",
        Some("hivey-prompt"),
        "a >_ sphere leading three agent spheres",
    ),
    ("no pet", None, "nothing on the desktop"),
];

/// Pets are macOS desktop apps.
pub(super) const PETS_SUPPORTED: bool = cfg!(target_os = "macos");

/// The pet chosen on this computer (`~/.hivey/pet.json`, written by `hivey pet`).
pub(super) fn chosen_pet() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(std::path::Path::new(&home).join(".hivey/pet.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?["pet"]
        .as_str()
        .map(str::to_string)
}

/// The installed creators, swarm creators first, and the one used for each kind.
pub(super) fn load_creators() -> (
    Vec<crate::swarm::creators::Creator>,
    (Option<String>, Option<String>),
) {
    use crate::swarm::creators;
    let mut found = creators::installed();
    found.sort_by(|a, b| (a.agent, &a.name).cmp(&(b.agent, &b.name)));
    let current = |agent| creators::pick(&found, agent, creators::chosen(agent).as_deref()).ok();
    let current = (current(false), current(true));
    (found, current)
}

/// The installed skills plugins and the one creators follow.
pub(super) fn load_skill_providers() -> (Vec<crate::swarm::skills_library::Provider>, Option<String>) {
    use crate::swarm::skills_library as library;
    let found = library::providers();
    let current = library::pick_provider(&found, library::chosen_provider().as_deref())
        .map(|provider| provider.id.clone());
    (found, current)
}

impl ClientShellState {
    /// Settings → skills: a library folder (rows before the last) or online search (the last
    /// row, toggled), saved with `hivey skills` in the background.
    fn choose_skills_setting(&mut self, selected: usize, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() else {
            return;
        };
        let args = if let Some(dir) = settings.skill_dirs.get(selected).cloned() {
            if settings.skill_dir_current.as_ref() == Some(&dir) {
                return;
            }
            settings.skills_message = Some(format!("skills library: {}", dir.display()));
            settings.skill_dir_current = Some(dir.clone());
            vec!["skills".into(), "dir".into(), dir.display().to_string()]
        } else if selected == settings.skill_dirs.len() {
            settings.skills_online = !settings.skills_online;
            let value = if settings.skills_online { "on" } else { "off" };
            settings.skills_message = Some(format!("online skill search {value}"));
            vec!["skills".into(), "online".into(), value.into()]
        } else {
            return;
        };
        outcome.actions.push(ClientShellAction::RunHivey(args));
        outcome.repaint = true;
    }

    /// Makes the selected creator the one `hivey swarm new` uses for its kind, with
    /// `hivey swarm providers --default` in the background.
    fn choose_creator(&mut self, selected: usize, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() else {
            return;
        };
        let Some(creator) = settings.creators.get(selected).cloned() else {
            // Rows after the creators are the skills plugins.
            let Some(provider) = settings
                .skill_providers
                .get(selected.saturating_sub(settings.creators.len()))
                .cloned()
            else {
                return;
            };
            if selected < settings.creators.len()
                || settings.skill_provider_current.as_deref() == Some(provider.id.as_str())
            {
                return;
            }
            settings.skill_provider_current = Some(provider.id.clone());
            settings.creator_message = Some(format!("new agents get their skills with {}", provider.name));
            outcome.actions.push(ClientShellAction::RunHivey(vec![
                "skills".into(),
                "providers".into(),
                "--default".into(),
                provider.id,
            ]));
            outcome.repaint = true;
            return;
        };
        let current = if creator.agent {
            &mut settings.creator_current.1
        } else {
            &mut settings.creator_current.0
        };
        if current.as_deref() == Some(creator.id.as_str()) {
            return;
        }
        *current = Some(creator.id.clone());
        settings.creator_message = Some(format!(
            "new {}s now use {}",
            if creator.agent { "agent" } else { "swarm" },
            creator.name
        ));
        outcome.actions.push(ClientShellAction::RunHivey(vec![
            "swarm".into(),
            "providers".into(),
            "--default".into(),
            creator.id,
        ]));
        outcome.repaint = true;
    }

    /// Switches the pet (or turns it off) with `hivey pet` in the background.
    fn choose_pet(&mut self, selected: usize, outcome: &mut ClientShellInput) {
        let Some(&(label, id, _)) = PET_CHOICES.get(selected) else {
            return;
        };
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() else {
            return;
        };
        if !PETS_SUPPORTED || settings.pet_current.as_deref() == id {
            return;
        }
        let args = match id {
            Some(id) => vec!["pet".to_string(), "use".to_string(), id.to_string()],
            None => vec!["pet".to_string(), "off".to_string()],
        };
        settings.pet_current = id.map(str::to_string);
        settings.pet_message = Some(match id {
            Some(_) => format!("switching to {label}… (the first time it builds, about a minute)"),
            None => "pet turned off".to_string(),
        });
        outcome.actions.push(ClientShellAction::RunHivey(args));
        outcome.repaint = true;
    }

    pub(super) fn open_settings_overlay(&mut self) {
        self.overlay = Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
            section: ClientSettingsSection::Theme,
            selected: theme_index(&self.config.theme_name),
            original_theme_name: self.config.theme_name.clone(),
            original_palette: self.config.palette.clone(),
            integrations: Vec::new(),
            integration_messages: Vec::new(),
            loading_integrations: false,
            installing_integrations: false,
            pet_current: chosen_pet(),
            pet_message: None,
            creators: Vec::new(),
            creator_current: (None, None),
            creator_message: None,
            skill_providers: Vec::new(),
            skill_provider_current: None,
            skill_dirs: Vec::new(),
            skill_dir_current: None,
            skills_online: true,
            skills_message: None,
        }));
    }

    fn selected_index_for_settings_section(&self, section: ClientSettingsSection) -> usize {
        match section {
            ClientSettingsSection::Theme => theme_index(&self.config.theme_name),
            ClientSettingsSection::Indicators => indicator_index(self.config.status_indicators),
            ClientSettingsSection::Sound => usize::from(!self.config.sound_enabled),
            ClientSettingsSection::Toast => toast_index(self.config.toast_delivery),
            ClientSettingsSection::Integrations => 0,
            ClientSettingsSection::Pets => {
                let current = chosen_pet();
                PET_CHOICES
                    .iter()
                    .position(|(_, id, _)| *id == current.as_deref())
                    .unwrap_or(PET_CHOICES.len() - 1)
            }
            ClientSettingsSection::Plugins => 0,
            ClientSettingsSection::Skills => 0,
        }
    }

    pub(super) fn select_settings_section(
        &mut self,
        section: ClientSettingsSection,
        outcome: &mut ClientShellInput,
    ) {
        let selected = self.selected_index_for_settings_section(section);
        let request_integrations = matches!(section, ClientSettingsSection::Integrations)
            && matches!(
                self.overlay,
                Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
                    loading_integrations: false,
                    installing_integrations: false,
                    ..
                }))
            );
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.section = section;
            settings.selected = selected;
            if section == ClientSettingsSection::Plugins {
                // Read fresh each time: plugins may have been installed meanwhile.
                (settings.creators, settings.creator_current) = load_creators();
                (settings.skill_providers, settings.skill_provider_current) =
                    load_skill_providers();
                settings.creator_message = None;
            }
            if section == ClientSettingsSection::Skills {
                use crate::swarm::skills_library as library;
                settings.skill_dirs = library::candidates();
                settings.skill_dir_current = Some(library::dir());
                settings.skills_online = library::online();
                settings.skills_message = None;
            }
        }
        if request_integrations {
            self.queue_integration_list(outcome, true);
        }
        outcome.repaint = true;
    }

    fn move_settings_section(&mut self, delta: isize, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_ref() else {
            return;
        };
        let current = ClientSettingsSection::ALL
            .iter()
            .position(|section| *section == settings.section)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(ClientSettingsSection::ALL.len() as isize)
            as usize;
        self.select_settings_section(ClientSettingsSection::ALL[next], outcome);
    }

    fn settings_choice_count(&self) -> usize {
        match self.overlay.as_ref() {
            Some(ClientShellOverlay::Settings(settings)) => match settings.section {
                ClientSettingsSection::Theme => crate::config::THEME_NAMES.len(),
                ClientSettingsSection::Indicators | ClientSettingsSection::Sound => 2,
                ClientSettingsSection::Toast => 4,
                ClientSettingsSection::Integrations => settings.integrations.len(),
                ClientSettingsSection::Pets if PETS_SUPPORTED => PET_CHOICES.len(),
                ClientSettingsSection::Pets => 0,
                ClientSettingsSection::Plugins => {
                    settings.creators.len() + settings.skill_providers.len()
                }
                ClientSettingsSection::Skills => settings.skill_dirs.len() + 1,
            },
            _ => 0,
        }
    }

    pub(super) fn move_settings_selection(&mut self, delta: isize) {
        let count = self.settings_choice_count();
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() else {
            return;
        };
        if count == 0 {
            settings.selected = 0;
            return;
        }
        settings.selected = (settings.selected as isize + delta)
            .clamp(0, count.saturating_sub(1) as isize) as usize;
        if settings.section == ClientSettingsSection::Theme {
            self.preview_selected_theme();
        }
    }

    pub(super) fn select_settings_choice(&mut self, index: usize) {
        let count = self.settings_choice_count();
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            if count > 0 {
                settings.selected = index.min(count - 1);
            }
        }
        if matches!(
            self.overlay,
            Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
                section: ClientSettingsSection::Theme,
                ..
            }))
        ) {
            self.preview_selected_theme();
        }
    }

    fn preview_selected_theme(&mut self) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_ref() else {
            return;
        };
        let Some(name) = crate::config::THEME_NAMES.get(settings.selected) else {
            return;
        };
        self.config.theme_name = (*name).to_owned();
        self.config.palette =
            crate::app::client_palette_for_theme(&self.config.theme_runtime, name);
    }

    pub(super) fn cancel_settings_overlay(&mut self) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.take() else {
            return;
        };
        self.config.theme_name = settings.original_theme_name;
        self.config.palette = settings.original_palette;
    }

    fn save_settings_edit(
        &mut self,
        edit: crate::config::ConfigEdit<'_>,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if let Err(error) = crate::config::write_edit(edit) {
            self.set_endpoint_error(error);
            outcome.repaint = true;
            return false;
        }
        self.reload_client_config();
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::ServerReloadConfig(
                crate::api::schema::EmptyParams::default(),
            ),
            PendingEndpointKind::ReloadConfig,
            outcome,
        );
        outcome.repaint = true;
        true
    }

    pub(super) fn apply_settings_choice(&mut self, outcome: &mut ClientShellInput) {
        let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_ref() else {
            return;
        };
        let section = settings.section;
        let selected = settings.selected;
        match section {
            ClientSettingsSection::Theme => {
                let Some(name) = crate::config::THEME_NAMES.get(selected).copied() else {
                    return;
                };
                if self.save_settings_edit(crate::config::ConfigEdit::Theme(name), outcome) {
                    self.overlay = None;
                }
            }
            ClientSettingsSection::Indicators => {
                let style = if selected == 0 {
                    crate::config::StatusIndicatorStyle::Dots
                } else {
                    crate::config::StatusIndicatorStyle::Symbols
                };
                self.save_settings_edit(
                    crate::config::ConfigEdit::StatusIndicators(style),
                    outcome,
                );
            }
            ClientSettingsSection::Sound => {
                self.save_settings_edit(crate::config::ConfigEdit::Sound(selected == 0), outcome);
            }
            ClientSettingsSection::Toast => {
                let delivery = match selected {
                    0 => crate::config::ToastDelivery::Off,
                    1 => crate::config::ToastDelivery::Herdr,
                    2 => crate::config::ToastDelivery::Terminal,
                    _ => crate::config::ToastDelivery::System,
                };
                self.save_settings_edit(
                    crate::config::ConfigEdit::ToastDelivery(delivery),
                    outcome,
                );
            }
            ClientSettingsSection::Integrations => self.install_recommended_integrations(outcome),
            ClientSettingsSection::Pets => self.choose_pet(selected, outcome),
            ClientSettingsSection::Plugins => self.choose_creator(selected, outcome),
            ClientSettingsSection::Skills => self.choose_skills_setting(selected, outcome),
        }
    }

    fn queue_integration_list(&mut self, outcome: &mut ClientShellInput, clear_messages: bool) {
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.loading_integrations = true;
            if clear_messages {
                settings.integration_messages.clear();
            }
        }
        if !self.push_endpoint_method_with_kind(
            crate::api::schema::Method::IntegrationList(crate::api::schema::EmptyParams::default()),
            PendingEndpointKind::IntegrationList,
            outcome,
        ) {
            if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                settings.loading_integrations = false;
            }
        }
    }

    fn install_recommended_integrations(&mut self, outcome: &mut ClientShellInput) {
        if self.pending_integration_installs > 0 {
            return;
        }
        let targets = match self.overlay.as_ref() {
            Some(ClientShellOverlay::Settings(settings)) => settings
                .integrations
                .iter()
                .filter(|integration| integration_needs_install(integration))
                .map(|integration| integration.target)
                .collect::<Vec<_>>(),
            _ => return,
        };
        if targets.is_empty() {
            return;
        }
        if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
            settings.installing_integrations = true;
            settings.integration_messages.clear();
        }
        self.pending_integration_installs = 0;
        for target in targets {
            if self.push_endpoint_method_with_kind(
                crate::api::schema::Method::IntegrationInstall(
                    crate::api::schema::IntegrationInstallParams { target },
                ),
                PendingEndpointKind::IntegrationInstall,
                outcome,
            ) {
                self.pending_integration_installs += 1;
            }
        }
        if self.pending_integration_installs == 0 {
            if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                settings.installing_integrations = false;
            }
        }
        outcome.repaint = true;
    }

    pub(super) fn handle_settings_endpoint_result(
        &mut self,
        kind: PendingEndpointKind,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) -> (bool, Vec<ClientShellAction>) {
        match kind {
            PendingEndpointKind::IntegrationList => {
                if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                    settings.loading_integrations = false;
                    match result {
                        Ok(crate::api::schema::ResponseResult::IntegrationList {
                            integrations,
                        }) => {
                            settings.integrations = integrations;
                            settings.selected = settings
                                .selected
                                .min(settings.integrations.len().saturating_sub(1));
                        }
                        Ok(_) => {
                            self.set_endpoint_error(
                                "endpoint returned an unexpected integration list result",
                            );
                        }
                        Err(_) => {}
                    }
                }
                (true, Vec::new())
            }
            PendingEndpointKind::IntegrationInstall => {
                let cancelled = result
                    .as_ref()
                    .is_err_and(|error| error.code.as_deref() == Some("endpoint_cancelled"));
                self.pending_integration_installs =
                    self.pending_integration_installs.saturating_sub(1);
                if let Some(ClientShellOverlay::Settings(settings)) = self.overlay.as_mut() {
                    match result {
                        Ok(crate::api::schema::ResponseResult::IntegrationInstall {
                            details,
                            ..
                        }) => settings.integration_messages.extend(details.messages),
                        Ok(_) => settings
                            .integration_messages
                            .push("endpoint returned an unexpected integration result".into()),
                        Err(error) => settings.integration_messages.push(error.message),
                    }
                    settings.installing_integrations = self.pending_integration_installs > 0;
                }
                let actions = if !cancelled
                    && self.pending_integration_installs == 0
                    && matches!(self.overlay, Some(ClientShellOverlay::Settings(_)))
                {
                    let mut deferred = ClientShellInput::default();
                    self.queue_integration_list(&mut deferred, false);
                    deferred.actions
                } else {
                    Vec::new()
                };
                (true, actions)
            }
            _ => (false, Vec::new()),
        }
    }

    pub(super) fn route_settings_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !matches!(self.overlay, Some(ClientShellOverlay::Settings(_))) {
            return false;
        }
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        if code == KeyCode::Esc {
            if !matches!(
                self.overlay,
                Some(ClientShellOverlay::Settings(ClientSettingsOverlay {
                    installing_integrations: true,
                    ..
                }))
            ) {
                self.cancel_settings_overlay();
                outcome.repaint = true;
            }
            return true;
        }
        if matches!(code, KeyCode::Tab | KeyCode::Right | KeyCode::Char('l'))
            && modifiers.is_empty()
        {
            self.move_settings_section(1, outcome);
            return true;
        }
        if matches!(code, KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h'))
            && modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            self.move_settings_section(-1, outcome);
            return true;
        }
        if matches!(code, KeyCode::Up | KeyCode::Char('k')) && modifiers.is_empty() {
            self.move_settings_selection(-1);
            outcome.repaint = true;
            return true;
        }
        if matches!(code, KeyCode::Down | KeyCode::Char('j')) && modifiers.is_empty() {
            self.move_settings_selection(1);
            outcome.repaint = true;
            return true;
        }
        if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) && modifiers.is_empty() {
            self.apply_settings_choice(outcome);
            return true;
        }
        true
    }
}
