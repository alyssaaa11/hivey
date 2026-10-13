//! Quiet mode: one switch for a quiet night. While `~/.hivey/quiet` exists nothing is spoken
//! (`hivey voice say`, so the pet and agents' summaries), hivey plays no notification sounds,
//! and the Slack relay holds its posts (it sends one catch-up message when quiet mode ends).
//! Agents keep working. Toggled by the hivey menu (quiet mode) and `hivey quiet on|off`.
//!
//! A daily schedule (settings → quiet, `hivey quiet schedule 22-7`) lives in
//! `~/.hivey/quiet.json`: `schedule` ([start, end] local hours, end excluded, may wrap
//! midnight) and `applied` (what the schedule last did). The hivey server turns quiet mode on
//! when the hours start and off when they end, only when that changes, so switching it by
//! hand lasts until the next start or end.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The schedules settings → quiet offers (any other with `hivey quiet schedule`).
pub(crate) const SCHEDULE_CHOICES: &[Option<(u8, u8)>] = &[
    None,
    Some((21, 7)),
    Some((22, 7)),
    Some((22, 8)),
    Some((23, 7)),
    Some((23, 8)),
    Some((0, 8)),
];

/// How often the server checks the schedule.
const SCHEDULE_CHECK: Duration = Duration::from_secs(30);

/// The last value read or set in this process, for the hivey menu's label (render reads it
/// without touching the file).
static CACHED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread use `path` instead of `~/.hivey/quiet`.
#[cfg(test)]
pub(crate) fn use_path_for_test(path: PathBuf) {
    TEST_PATH.with(|test_path| *test_path.borrow_mut() = Some(path));
}

fn path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_PATH.with(|test_path| test_path.borrow().clone()) {
        return path;
    }
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".hivey")
        .join("quiet")
}

/// Whether quiet mode is on now (reads the file).
pub(crate) fn is_on() -> bool {
    let on = path().exists();
    CACHED.store(on, Ordering::Relaxed);
    on
}

/// The value `is_on` or `set` last saw in this process.
pub(crate) fn cached() -> bool {
    // Tests run in parallel threads with their own paths, so they share no cache
    if cfg!(test) {
        return is_on();
    }
    CACHED.load(Ordering::Relaxed)
}

/// Turns quiet mode on (the file holds when, in Unix seconds) or off.
pub(crate) fn set(on: bool) -> std::io::Result<()> {
    let path = path();
    if on {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        std::fs::write(&path, format!("{since}\n"))?;
    } else {
        match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    }
    CACHED.store(on, Ordering::Relaxed);
    Ok(())
}

fn settings_path() -> PathBuf {
    path().with_file_name("quiet.json")
}

fn load() -> Value {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn save(saved: &Value) -> std::io::Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, format!("{saved:#}\n"))?;
    std::fs::rename(&tmp, &path)
}

fn parse_hours(value: &Value) -> Option<(u8, u8)> {
    let hour = |index: usize| {
        value
            .get(index)?
            .as_u64()
            .filter(|hour| *hour < 24)
            .map(|hour| hour as u8)
    };
    Some((hour(0)?, hour(1)?))
}

/// The daily quiet hours, if any.
pub(crate) fn schedule() -> Option<(u8, u8)> {
    parse_hours(&load()["schedule"])
}

/// `22:00 → 07:00`, or `off`.
pub(crate) fn describe(hours: Option<(u8, u8)>) -> String {
    match hours {
        Some((start, end)) => format!("{start:02}:00 → {end:02}:00"),
        None => "off".to_string(),
    }
}

/// Saves the daily quiet hours (none: no schedule) and applies them now: quiet mode goes on
/// when it is inside the new hours, off when it isn't.
pub(crate) fn set_schedule(hours: Option<(u8, u8)>) -> std::io::Result<()> {
    let mut saved = load();
    saved["schedule"] = match hours {
        Some((start, end)) => json!([start, end]),
        None => Value::Null,
    };
    if let Some(saved) = saved.as_object_mut() {
        saved.remove("applied");
    }
    save(&saved)?;
    apply_schedule(local_hour()).map(|_| ())
}

fn local_hour() -> u8 {
    (crate::swarm::schedule::local_now().minute / 60) as u8
}

/// Turns quiet mode on when the schedule's hours start and off when they end, only when that
/// changes (so a switch by hand lasts until the next start or end). Returns what it did.
pub(crate) fn apply_schedule(hour: u8) -> std::io::Result<Option<bool>> {
    let mut saved = load();
    let Some(hours) = parse_hours(&saved["schedule"]) else {
        return Ok(None);
    };
    let on = crate::swarm::voice::in_quiet_hours(Some(hours), hour);
    let applied = json!({"schedule": [hours.0, hours.1], "on": on});
    if saved["applied"] == applied {
        return Ok(None);
    }
    set(on)?;
    saved["applied"] = applied;
    save(&saved)?;
    Ok(Some(on))
}

/// Called by the hivey server's swarm tick: checks the schedule about twice a minute.
pub(crate) fn tick() {
    static LAST_CHECK: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let Ok(mut last) = LAST_CHECK.lock() else {
            return;
        };
        if last.is_some_and(|at| at.elapsed() < SCHEDULE_CHECK) {
            return;
        }
        *last = Some(Instant::now());
    }
    match apply_schedule(local_hour()) {
        Ok(Some(on)) => tracing::info!(on, "hivey quiet mode schedule"),
        Ok(None) => {}
        Err(err) => tracing::warn!(%err, "hivey quiet mode schedule"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_on_and_off_and_silences_the_voice() {
        let dir = std::env::temp_dir().join(format!("hivey-quiet-{}", std::process::id()));
        use_path_for_test(dir.join(".hivey").join("quiet"));
        assert!(!is_on());
        set(false).expect("off when already off");
        set(true).expect("on");
        assert!(is_on());
        assert!(cached());
        assert!(crate::swarm::voice::quiet_now());
        set(false).expect("off");
        assert!(!is_on());
        assert!(!cached());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn schedule_switches_at_its_edges_and_a_manual_switch_lasts_until_the_next() {
        let dir = std::env::temp_dir().join(format!("hivey-quiet-sched-{}", std::process::id()));
        use_path_for_test(dir.join("quiet"));
        assert_eq!(apply_schedule(23).expect("no schedule"), None);
        set_schedule(Some((22, 7))).expect("schedule");
        assert_eq!(schedule(), Some((22, 7)));
        // Start from outside the hours, whatever time the test runs at
        apply_schedule(9).expect("day");
        assert!(!is_on());
        assert_eq!(apply_schedule(22).expect("start"), Some(true));
        assert!(is_on());
        assert_eq!(apply_schedule(23).expect("still night"), None);
        // Off by hand at night: stays off until the night ends
        set(false).expect("off by hand");
        assert_eq!(apply_schedule(2).expect("night"), None);
        assert!(!is_on());
        assert_eq!(apply_schedule(7).expect("end"), Some(false));
        // On by hand in the day: stays on until the next night starts, then ends with it
        set(true).expect("on by hand");
        assert_eq!(apply_schedule(12).expect("day"), None);
        assert!(is_on());
        assert_eq!(apply_schedule(22).expect("start"), Some(true));
        assert_eq!(apply_schedule(7).expect("end"), Some(false));
        assert!(!is_on());
        set_schedule(None).expect("no schedule");
        assert_eq!(schedule(), None);
        assert_eq!(apply_schedule(23).expect("no schedule"), None);
        assert!(!is_on());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn describes_schedules() {
        assert_eq!(describe(Some((22, 7))), "22:00 → 07:00");
        assert_eq!(describe(Some((0, 8))), "00:00 → 08:00");
        assert_eq!(describe(None), "off");
    }
}
