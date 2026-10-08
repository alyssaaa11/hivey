//! `hivey voice`: who reads agents' spoken summaries (the tts tool, the system `say`, or
//! nobody), with which voice, and when to stay quiet. The Claude Code Stop hook speaks through
//! `hivey voice say`.

use crate::swarm::voice::{self, Provider};

const HELP: &str = "\
usage: hivey voice                        the provider, its voice and quiet hours
       hivey voice provider tts|say|off   who speaks: the local tts tool (Kokoro voices), the
                                          system say command, or nobody
       hivey voice use [PROVIDER] VOICE   the voice (for PROVIDER, else the current provider);
                                          naming a provider also switches to it
       hivey voice list [PROVIDER]        installed voices (* = used)
       hivey voice quiet START-END|off    local hours when nothing is spoken, e.g. 22-8
       hivey voice volume [N|reset]       how loud: 1 is normal, 1.5 or 150% is louder (up to
                                          4; high values may distort), 0.5 quieter
       hivey voice say [text…]            speak it now, or stdin without text (silent when off
                                          or in quiet hours; lines queue, never overlap)
       hivey voice test                   speak a sample, even in quiet hours
  provider, use and volume take --test to speak a sample after saving (hivey settings does).
  Also in hivey settings → voice. Settings live in ~/.hivey/voice.json.";

const SAMPLE: &str = "Done testing the voice, on hivey.";

fn status() -> i32 {
    let provider = voice::provider();
    let missing = if provider.available() {
        ""
    } else {
        " (not installed on this computer)"
    };
    println!(
        "provider: {} — {}{missing}",
        provider.id(),
        provider.about()
    );
    if provider != Provider::Off {
        println!(
            "voice:    {}",
            voice::voice(provider).unwrap_or_else(|| format!("{}'s default", provider.id()))
        );
    }
    match voice::quiet_hours() {
        Some((start, end)) => println!("quiet:    {start}:00 to {end}:00"),
        None => println!("quiet:    never"),
    }
    println!("volume:   {}", volume_label(voice::volume()));
    0
}

fn volume_label(volume: f64) -> String {
    if voice::is_normal_volume(volume) {
        "1 (normal)".to_string()
    } else {
        format!("{volume} ({:.0}%)", volume * 100.0)
    }
}

/// Saves the line to a temporary file and plays it at `volume`; `None` when that isn't possible
/// here (no afplay), so the caller plays it the normal way.
fn speak_at_volume(provider: Provider, text: &str, volume: f64) -> Option<i32> {
    let file = std::env::temp_dir().join(format!(
        "hivey-voice-{}.{}",
        std::process::id(),
        voice::audio_extension(provider)
    ));
    let mut play = voice::play_command(&file, volume)?;
    let mut render =
        voice::render_command(provider, voice::voice(provider).as_deref(), text, &file)?;
    // tts prints "Audio saved to …"; the Stop hook and the pets only want the sound.
    render.stdout(std::process::Stdio::null());
    let code = match render.status() {
        Ok(status) if status.success() => match play.status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(err) => {
                eprintln!("hivey voice: cannot run afplay: {err}");
                1
            }
        },
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("hivey voice: cannot run {}: {err}", provider.id());
            1
        }
    };
    let _ = std::fs::remove_file(&file);
    Some(code)
}

/// Speaks `text` and waits for it to finish; `force` ignores quiet hours.
fn speak(text: &str, force: bool) -> i32 {
    if text.trim().is_empty() || (!force && voice::quiet_now()) {
        return 0;
    }
    let provider = voice::provider();
    let Some(mut command) = voice::command(provider, voice::voice(provider).as_deref(), text)
    else {
        if provider != Provider::Off {
            eprintln!("hivey voice: {} is not installed", provider.id());
            return 1;
        }
        return 0;
    };
    // Released when it goes out of scope, after the line has been spoken.
    let _turn = voice::speaking_turn();
    let volume = voice::volume();
    if !voice::is_normal_volume(volume) {
        if let Some(code) = speak_at_volume(provider, text, volume) {
            return code;
        }
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("hivey voice: cannot run {}: {err}", provider.id());
            1
        }
    }
}

fn use_voice(args: &[String]) -> std::io::Result<i32> {
    let (provider, name) = match args {
        [name] => (voice::provider(), name.as_str()),
        [provider, name] => match Provider::parse(provider) {
            Some(provider) => (provider, name.as_str()),
            None => {
                eprintln!("hivey voice: unknown provider {provider:?} (tts, say or off)");
                return Ok(2);
            }
        },
        _ => {
            eprintln!("usage: hivey voice use [PROVIDER] VOICE");
            return Ok(2);
        }
    };
    if provider == Provider::Off {
        eprintln!("hivey voice: off has no voices");
        return Ok(2);
    }
    let installed = voice::installed(provider);
    if !installed.is_empty() && !installed.iter().any(|known| known == name) {
        eprintln!(
            "hivey voice: {name:?} is not an installed {} voice (hivey voice list {})",
            provider.id(),
            provider.id()
        );
        return Ok(1);
    }
    voice::set_voice(provider, name)?;
    if args.len() == 2 {
        voice::set_provider(provider)?;
    }
    println!("{} voice: {name}", provider.id());
    Ok(0)
}

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    // `--test` only for the commands that save; `say` speaks its text as given.
    let saves = matches!(
        args.first().map(String::as_str),
        Some("provider" | "use" | "volume")
    );
    let test = saves && args.iter().any(|arg| arg == "--test");
    let args: Vec<String> = args
        .iter()
        .filter(|arg| !saves || *arg != "--test")
        .cloned()
        .collect();
    let code = match args.first().map(String::as_str) {
        None | Some("status") => status(),
        Some("help" | "--help" | "-h") => {
            println!("{HELP}");
            0
        }
        Some("provider") => match args.get(1).map(|id| (id, Provider::parse(id))) {
            Some((_, Some(provider))) => {
                voice::set_provider(provider)?;
                let missing = if provider.available() {
                    ""
                } else {
                    " (not installed on this computer: nothing will be spoken)"
                };
                println!("provider: {}{missing}", provider.id());
                0
            }
            Some((id, None)) => {
                eprintln!("hivey voice: unknown provider {id:?} (tts, say or off)");
                2
            }
            None => {
                println!("{}", voice::provider().id());
                0
            }
        },
        Some("use") => use_voice(&args[1..])?,
        Some("list") => {
            let provider = match args.get(1) {
                Some(id) => match Provider::parse(id) {
                    Some(provider) => provider,
                    None => {
                        eprintln!("hivey voice: unknown provider {id:?} (tts, say or off)");
                        return Ok(2);
                    }
                },
                None => voice::provider(),
            };
            let current = voice::voice(provider);
            let voices = voice::installed(provider);
            for name in &voices {
                let mark = if current.as_deref() == Some(name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {name}");
            }
            if voices.is_empty() {
                println!("no {} voices found", provider.id());
            }
            0
        }
        Some("quiet") => match args.get(1).map(String::as_str) {
            Some("off") => {
                voice::set_quiet_hours(None)?;
                println!("quiet hours: off");
                0
            }
            Some(text) => match voice::parse_quiet_hours(text) {
                Some(hours) => {
                    voice::set_quiet_hours(Some(hours))?;
                    println!("quiet hours: {}:00 to {}:00", hours.0, hours.1);
                    0
                }
                None => {
                    eprintln!("usage: hivey voice quiet START-END|off  (hours 0-23, e.g. 22-8)");
                    2
                }
            },
            None => {
                eprintln!("usage: hivey voice quiet START-END|off  (hours 0-23, e.g. 22-8)");
                2
            }
        },
        Some("volume") => match args.get(1).map(String::as_str) {
            None => {
                println!("volume: {}", volume_label(voice::volume()));
                0
            }
            Some("reset" | "normal") => {
                voice::set_volume(1.0)?;
                println!("volume: {}", volume_label(1.0));
                0
            }
            Some(text) => match voice::parse_volume(text) {
                Some(volume) => {
                    voice::set_volume(volume)?;
                    println!("volume: {}", volume_label(volume));
                    0
                }
                None => {
                    eprintln!(
                        "usage: hivey voice volume N|reset  (1 is normal, e.g. 1.5 or 150%; \
                         0.05 to {})",
                        voice::MAX_VOLUME
                    );
                    2
                }
            },
        },
        Some("say") if args.len() > 1 => speak(&args[1..].join(" "), false),
        Some("say") => {
            // No text: read it from stdin (the pets do, so text starting with `-` stays text).
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
            speak(&text, false)
        }
        Some("test") => speak(SAMPLE, true),
        Some(other) => {
            eprintln!("hivey voice: unknown command {other:?}\n{HELP}");
            2
        }
    };
    if code == 0 && test {
        return Ok(speak(SAMPLE, true));
    }
    Ok(code)
}
