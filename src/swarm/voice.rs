//! The voice that reads agents' spoken summaries (the Claude Code Stop hook runs
//! `hivey voice say "<text>"`). Providers: the local `tts` tool (Kokoro voices), the system
//! `say` command, or none.
//!
//! Settings live in `~/.hivey/voice.json`: `provider` ("tts", "say" or "off"), `voices` (the
//! voice per provider; none means the provider's own default), `quiet_hours` ([start, end],
//! local hours when nothing is spoken) and `volume` (1.0 when absent: as the provider plays it;
//! any other value saves the line to a file and plays it with `afplay` at that volume).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Loudest volume accepted: above 1 `afplay` amplifies, and far above it the voice distorts.
pub(crate) const MAX_VOLUME: f64 = 4.0;
const MIN_VOLUME: f64 = 0.05;
/// The volumes hivey settings offers (any other with `hivey voice volume`).
pub(crate) const VOLUME_LEVELS: &[f64] = &[0.5, 1.0, 1.5, 2.0, 2.5, 3.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider {
    Tts,
    Say,
    Off,
}

impl Provider {
    pub(crate) const ALL: &[Self] = &[Self::Tts, Self::Say, Self::Off];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Tts => "tts",
            Self::Say => "say",
            Self::Off => "off",
        }
    }

    pub(crate) fn about(self) -> &'static str {
        match self {
            Self::Tts => "local Kokoro voices (the tts tool)",
            Self::Say => "the system voices (say)",
            Self::Off => "silent",
        }
    }

    pub(crate) fn parse(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|provider| provider.id() == id)
    }

    /// Whether its command is on this computer (`off` always is).
    pub(crate) fn available(self) -> bool {
        match self {
            Self::Tts => tts_bin().is_some(),
            Self::Say => find_on_path("say").is_some(),
            Self::Off => true,
        }
    }
}

/// Voices offered in hivey settings for each provider: name and a short description. Other
/// installed voices can be set with `hivey voice use`.
const TTS_SUGGESTED: &[(&str, &str)] = &[
    ("am_michael", "American man"),
    ("af_heart", "American woman"),
    ("af_bella", "American woman, brighter"),
    ("bf_emma", "British woman"),
];
const SAY_SUGGESTED: &[(&str, &str)] = &[
    ("Daniel", "British man"),
    ("Fred", "American man, classic"),
    ("Ralph", "American man, deep"),
    ("Samantha", "American woman"),
    ("Karen", "Australian woman"),
];

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn settings_path() -> PathBuf {
    home().join(".hivey").join("voice.json")
}

fn settings() -> Value {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn save(update: impl FnOnce(&mut Value)) -> std::io::Result<()> {
    let mut saved = settings();
    update(&mut saved);
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&saved).unwrap_or_default();
    std::fs::write(path, text + "\n")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The `tts` tool: on PATH, else where `uv tool` installs it (hooks may run with a minimal PATH).
fn tts_bin() -> Option<PathBuf> {
    find_on_path("tts").or_else(|| {
        let local = home().join(".local").join("bin").join("tts");
        local.is_file().then_some(local)
    })
}

/// The chosen provider (default: tts, the one the Stop hook used before this setting existed).
pub(crate) fn provider() -> Provider {
    settings()["provider"]
        .as_str()
        .and_then(Provider::parse)
        .unwrap_or(Provider::Tts)
}

pub(crate) fn set_provider(provider: Provider) -> std::io::Result<()> {
    save(|saved| saved["provider"] = json!(provider.id()))
}

/// The voice chosen for a provider, if any (else the provider's own default).
pub(crate) fn voice(provider: Provider) -> Option<String> {
    settings()["voices"][provider.id()]
        .as_str()
        .filter(|voice| !voice.is_empty())
        .map(str::to_string)
}

pub(crate) fn set_voice(provider: Provider, voice: &str) -> std::io::Result<()> {
    save(|saved| {
        if !saved["voices"].is_object() {
            saved["voices"] = json!({});
        }
        saved["voices"][provider.id()] = json!(voice);
    })
}

/// The voices hivey settings offers for a provider.
pub(crate) fn suggested(provider: Provider) -> &'static [(&'static str, &'static str)] {
    match provider {
        Provider::Tts => TTS_SUGGESTED,
        Provider::Say => SAY_SUGGESTED,
        Provider::Off => &[],
    }
}

/// A row of hivey settings → voice: a provider, one of its voices (none for off) and a short
/// description.
pub(crate) type Choice = (Provider, Option<&'static str>, &'static str);

/// The rows of hivey settings → voice: every suggested voice of every provider, then off, so
/// each row is a complete choice.
pub(crate) fn choices() -> Vec<Choice> {
    Provider::ALL
        .iter()
        .flat_map(|&provider| {
            let voices = suggested(provider)
                .iter()
                .map(move |&(name, about)| (provider, Some(name), about));
            let off = (provider == Provider::Off).then_some((provider, None, "silent"));
            voices.chain(off)
        })
        .collect()
}

/// The row of `choices()` for a provider and its voice (off: the off row; a voice not in the
/// list, or the provider's default: none).
pub(crate) fn choice_index(provider: Provider, voice: Option<&str>) -> Option<usize> {
    choices().iter().position(|&(row_provider, row_voice, _)| {
        row_provider == provider && (provider == Provider::Off || row_voice == voice)
    })
}

/// The voices installed for a provider: Kokoro voice files for tts, `say -v '?'` for say.
pub(crate) fn installed(provider: Provider) -> Vec<String> {
    let mut voices: Vec<String> = match provider {
        Provider::Tts => {
            let models = std::fs::read_to_string(home().join(".config/tts/config.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|config| config["models_dir"].as_str().map(PathBuf::from))
                .unwrap_or_else(|| home().join("models"));
            std::fs::read_dir(models.join("Kokoro-82M").join("voices"))
                .map(|entries| {
                    entries
                        .flatten()
                        .filter_map(|entry| {
                            let path = entry.path();
                            (path.extension()? == "pt")
                                .then(|| path.file_stem()?.to_str().map(str::to_string))?
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        Provider::Say => Command::new("say")
            .args(["-v", "?"])
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(say_voice_name)
                    .collect()
            })
            .unwrap_or_default(),
        Provider::Off => Vec::new(),
    };
    voices.sort();
    voices.dedup();
    voices
}

/// The voice name in a `say -v '?'` line: `Eddy (English (UK))  en_GB    # Hello! ...`.
fn say_voice_name(line: &str) -> Option<String> {
    let before_comment = line.split('#').next()?.trim_end();
    let (name, _locale) = before_comment.rsplit_once(char::is_whitespace)?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The playback volume: 1.0 (the provider's own level) unless set.
pub(crate) fn volume() -> f64 {
    volume_from(&settings()["volume"])
}

fn volume_from(value: &Value) -> f64 {
    value
        .as_f64()
        .filter(|volume| volume.is_finite() && *volume >= MIN_VOLUME)
        .map(|volume| volume.min(MAX_VOLUME))
        .unwrap_or(1.0)
}

/// Saves the volume; 1.0 removes the setting (back to the provider's own playback).
pub(crate) fn set_volume(volume: f64) -> std::io::Result<()> {
    save(|saved| {
        saved["volume"] = if is_normal_volume(volume) {
            Value::Null
        } else {
            json!(volume)
        }
    })
}

pub(crate) fn is_normal_volume(volume: f64) -> bool {
    (volume - 1.0).abs() < 1e-9
}

/// The entry of `VOLUME_LEVELS` for a volume (none for a level set by hand, e.g. 1.7).
pub(crate) fn volume_index(volume: f64) -> Option<usize> {
    VOLUME_LEVELS
        .iter()
        .position(|level| (level - volume).abs() < 1e-9)
}

/// Parses `1.5` or `150%` into 1.5 (between 0.05 and 4).
pub(crate) fn parse_volume(text: &str) -> Option<f64> {
    let text = text.trim();
    let volume = match text.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f64>().ok()? / 100.0,
        None => text.parse::<f64>().ok()?,
    };
    (volume.is_finite() && (MIN_VOLUME..=MAX_VOLUME).contains(&volume)).then_some(volume)
}

/// Quiet hours ([start, end], local hours 0-23; end excluded; may wrap midnight).
pub(crate) fn quiet_hours() -> Option<(u8, u8)> {
    let hours = settings()["quiet_hours"].as_array()?.clone();
    let hour = |index: usize| {
        hours
            .get(index)?
            .as_u64()
            .filter(|hour| *hour < 24)
            .map(|hour| hour as u8)
    };
    Some((hour(0)?, hour(1)?))
}

pub(crate) fn set_quiet_hours(hours: Option<(u8, u8)>) -> std::io::Result<()> {
    save(|saved| {
        saved["quiet_hours"] = match hours {
            Some((start, end)) => json!([start, end]),
            None => Value::Null,
        }
    })
}

/// Parses `22-8` into (22, 8).
pub(crate) fn parse_quiet_hours(text: &str) -> Option<(u8, u8)> {
    let (start, end) = text.split_once('-')?;
    let start: u8 = start.trim().parse().ok().filter(|hour| *hour < 24)?;
    let end: u8 = end.trim().parse().ok().filter(|hour| *hour < 24)?;
    Some((start, end))
}

fn in_quiet_hours(hours: Option<(u8, u8)>, hour: u8) -> bool {
    match hours {
        Some((start, end)) if start == end => false,
        Some((start, end)) if start < end => (start..end).contains(&hour),
        Some((start, end)) => hour >= start || hour < end,
        None => false,
    }
}

/// Whether it is quiet hours now.
pub(crate) fn quiet_now() -> bool {
    let hour = (crate::swarm::schedule::local_now().minute / 60) as u8;
    in_quiet_hours(quiet_hours(), hour)
}

/// Holds `~/.hivey/voice.lock` while speaking, so lines from the Stop hook and the pet queue
/// up instead of talking over each other. `None` when the lock file can't be opened (then
/// speech just isn't serialized).
pub(crate) fn speaking_turn() -> Option<std::fs::File> {
    let path = home().join(".hivey").join("voice.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    file.lock().ok()?;
    Some(file)
}

/// The command that speaks `text` with this provider and voice (`None` for off or when the
/// provider's command is missing).
pub(crate) fn command(provider: Provider, voice: Option<&str>, text: &str) -> Option<Command> {
    provider_command(provider, voice, text, None)
}

/// The command that saves `text` spoken with this provider and voice to `out`, for
/// `play_command` (`None` for off or when the provider's command is missing).
pub(crate) fn render_command(
    provider: Provider,
    voice: Option<&str>,
    text: &str,
    out: &Path,
) -> Option<Command> {
    provider_command(provider, voice, text, Some(out))
}

/// The audio file type `render_command` writes for a provider.
pub(crate) fn audio_extension(provider: Provider) -> &'static str {
    match provider {
        Provider::Tts => "mp3",
        Provider::Say | Provider::Off => "aiff",
    }
}

fn provider_command(
    provider: Provider,
    voice: Option<&str>,
    text: &str,
    out: Option<&Path>,
) -> Option<Command> {
    let mut command = match provider {
        Provider::Tts => {
            let mut command = Command::new(tts_bin()?);
            match out {
                Some(out) => command.arg("--output").arg(out),
                None => command.arg("--talk"),
            };
            if let Some(voice) = voice {
                command.args(["--voice", voice]);
            }
            command
        }
        Provider::Say => {
            let mut command = Command::new(find_on_path("say")?);
            if let Some(voice) = voice {
                command.args(["-v", voice]);
            }
            if let Some(out) = out {
                command.arg("-o").arg(out);
            }
            command
        }
        Provider::Off => return None,
    };
    // `--` keeps a summary that starts with `-` from being read as an option.
    command.arg("--").arg(text);
    Some(command)
}

/// `afplay -v VOLUME FILE`, or `None` when afplay isn't there (then the provider plays the line
/// itself at its own level).
pub(crate) fn play_command(file: &Path, volume: f64) -> Option<Command> {
    let afplay = find_on_path("afplay").or_else(|| {
        let system = PathBuf::from("/usr/bin/afplay");
        system.is_file().then_some(system)
    })?;
    let mut command = Command::new(afplay);
    command.arg("-v").arg(format!("{volume}")).arg(file);
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_wrap_midnight() {
        let night = Some((22, 8));
        assert!(in_quiet_hours(night, 23));
        assert!(in_quiet_hours(night, 0));
        assert!(in_quiet_hours(night, 7));
        assert!(!in_quiet_hours(night, 8));
        assert!(!in_quiet_hours(night, 21));
    }

    #[test]
    fn quiet_hours_within_a_day() {
        let lunch = Some((12, 14));
        assert!(in_quiet_hours(lunch, 12));
        assert!(in_quiet_hours(lunch, 13));
        assert!(!in_quiet_hours(lunch, 14));
        assert!(!in_quiet_hours(None, 3));
        assert!(!in_quiet_hours(Some((5, 5)), 5));
    }

    #[test]
    fn parses_quiet_hours() {
        assert_eq!(parse_quiet_hours("22-8"), Some((22, 8)));
        assert_eq!(parse_quiet_hours(" 9 - 17 "), Some((9, 17)));
        assert_eq!(parse_quiet_hours("24-8"), None);
        assert_eq!(parse_quiet_hours("off"), None);
    }

    #[test]
    fn parses_volume() {
        assert_eq!(parse_volume("1.5"), Some(1.5));
        assert_eq!(parse_volume(" 150% "), Some(1.5));
        assert_eq!(parse_volume("0.5"), Some(0.5));
        assert_eq!(parse_volume("4"), Some(4.0));
        assert_eq!(parse_volume("5"), None);
        assert_eq!(parse_volume("0"), None);
        assert_eq!(parse_volume("-1"), None);
        assert_eq!(parse_volume("loud"), None);
        assert_eq!(parse_volume("NaN"), None);
    }

    #[test]
    fn stored_volume_defaults_to_normal_and_is_capped() {
        assert_eq!(volume_from(&Value::Null), 1.0);
        assert_eq!(volume_from(&json!(1.5)), 1.5);
        assert_eq!(volume_from(&json!(9)), MAX_VOLUME);
        assert_eq!(volume_from(&json!(0)), 1.0);
        assert_eq!(volume_from(&json!("loud")), 1.0);
        assert!(is_normal_volume(1.0));
        assert!(!is_normal_volume(1.5));
    }

    #[test]
    fn volume_levels_are_valid_and_found() {
        for (index, level) in VOLUME_LEVELS.iter().enumerate() {
            assert_eq!(parse_volume(&level.to_string()), Some(*level));
            assert_eq!(volume_index(*level), Some(index));
        }
        assert!(volume_index(1.0).is_some());
        assert_eq!(volume_index(1.7), None);
    }

    #[test]
    fn reads_say_voice_names() {
        assert_eq!(
            say_voice_name("Daniel              en_GB    # Hello! My name is Daniel."),
            Some("Daniel".to_string())
        );
        assert_eq!(
            say_voice_name("Eddy (English (UK)) en_GB    # Hello! My name is Eddy."),
            Some("Eddy (English (UK))".to_string())
        );
        assert_eq!(say_voice_name(""), None);
    }

    #[test]
    fn choices_list_every_provider_and_find_the_current_one() {
        let rows = choices();
        assert_eq!(rows.len(), TTS_SUGGESTED.len() + SAY_SUGGESTED.len() + 1);
        assert_eq!(rows.last().map(|row| row.0), Some(Provider::Off));
        let daniel = choice_index(Provider::Say, Some("Daniel"));
        assert_eq!(daniel.map(|index| rows[index].1), Some(Some("Daniel")));
        assert_eq!(choice_index(Provider::Off, None), Some(rows.len() - 1));
        assert_eq!(
            choice_index(Provider::Off, Some("Daniel")),
            Some(rows.len() - 1)
        );
        assert_eq!(choice_index(Provider::Say, Some("Zarvox")), None);
        assert_eq!(choice_index(Provider::Tts, None), None);
    }

    #[test]
    fn providers_round_trip() {
        for provider in Provider::ALL {
            assert_eq!(Provider::parse(provider.id()), Some(*provider));
        }
        assert_eq!(Provider::parse("espeak"), None);
    }
}
