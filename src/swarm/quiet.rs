//! Quiet mode: one switch for a quiet night. While `~/.hivey/quiet` exists nothing is spoken
//! (`hivey voice say`, so the pet and agents' summaries), hivey plays no notification sounds,
//! and the Slack relay holds its posts (it sends one catch-up message when quiet mode ends).
//! Agents keep working. Toggled by the hivey menu (quiet mode) and `hivey quiet on|off`.
//!
//! A daily schedule (settings → quiet, `hivey quiet schedule 22:30-7`) lives in
//! `~/.hivey/quiet.json`: `schedule` (`{"on": true, "start": "22:30", "end": "07:00"}`, local
//! times, end excluded, may wrap midnight; the times are kept while it is off) and `applied`
//! (what the schedule last did). The hivey server turns quiet mode on when the time starts
//! and off when it ends, only when that changes, so switching it by hand lasts until the next
//! start or end.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Minutes in a day; schedule times are minutes since midnight.
const DAY: u16 = 24 * 60;
/// The times a new schedule starts from.
pub(crate) const DEFAULT_TIMES: (u16, u16) = (22 * 60, 7 * 60);
/// How much settings → quiet's − and + move a time.
pub(crate) const STEP: u16 = 30;

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

/// `22:30` → minutes since midnight.
pub(crate) fn format_time(minute: u16) -> String {
    format!("{:02}:{:02}", minute / 60 % 24, minute % 60)
}

/// `22`, `7`, `22:30`, `7:05`, `2230` or `730` → minutes since midnight.
pub(crate) fn parse_time(text: &str) -> Option<u16> {
    let text = text.trim();
    let (hours, minutes) = match text.split_once(':') {
        Some((hours, minutes)) if minutes.len() == 2 => (hours, minutes),
        Some(_) => return None,
        None if text.len() <= 2 => (text, "0"),
        None if text.len() <= 4 => text.split_at(text.len() - 2),
        None => return None,
    };
    if hours.is_empty() || !(hours.chars().chain(minutes.chars())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hours: u16 = hours.parse().ok().filter(|hours| *hours < 24)?;
    let minutes: u16 = minutes.parse().ok().filter(|minutes| *minutes < 60)?;
    Some(hours * 60 + minutes)
}

/// `22:30-7` → (start, end).
pub(crate) fn parse_times(text: &str) -> Option<(u16, u16)> {
    let (start, end) = text.split_once('-')?;
    Some((parse_time(start)?, parse_time(end)?))
}

/// `22:30 → 07:00`, or `off`.
pub(crate) fn describe(times: Option<(u16, u16)>) -> String {
    match times {
        Some((start, end)) => format!("{} → {}", format_time(start), format_time(end)),
        None => "off".to_string(),
    }
}

/// Moves a time by `delta` minutes around the clock.
pub(crate) fn shift(minute: u16, delta: i32) -> u16 {
    (i32::from(minute) + delta).rem_euclid(i32::from(DAY)) as u16
}

/// Whether `minute` falls in [start, end) (wrapping midnight; never when start == end).
fn in_times((start, end): (u16, u16), minute: u16) -> bool {
    match start.cmp(&end) {
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Less => (start..end).contains(&minute),
        std::cmp::Ordering::Greater => minute >= start || minute < end,
    }
}

/// (on, start, end) as saved; an older `[start hour, end hour]` reads as on at those hours.
fn read_schedule(saved: &Value) -> (bool, (u16, u16)) {
    let schedule = &saved["schedule"];
    if let Some(hours) = schedule.as_array() {
        let hour = |index: usize| {
            hours
                .get(index)?
                .as_u64()
                .filter(|hour| *hour < 24)
                .map(|hour| hour as u16 * 60)
        };
        if let (Some(start), Some(end)) = (hour(0), hour(1)) {
            return (true, (start, end));
        }
    }
    let time = |key: &str| schedule[key].as_str().and_then(parse_time);
    let times = match (time("start"), time("end")) {
        (Some(start), Some(end)) => (start, end),
        _ => DEFAULT_TIMES,
    };
    (schedule["on"].as_bool().unwrap_or(false), times)
}

/// The daily quiet times when the schedule is on.
pub(crate) fn schedule() -> Option<(u16, u16)> {
    let (on, times) = read_schedule(&load());
    on.then_some(times)
}

/// The schedule's times, kept while it is off (a new one starts from 22:00 → 07:00).
pub(crate) fn schedule_times() -> (u16, u16) {
    read_schedule(&load()).1
}

/// Saves the schedule (on or off, and its times) and applies it now: when on, quiet mode goes
/// on inside the times and off outside them.
pub(crate) fn save_schedule(on: bool, (start, end): (u16, u16)) -> std::io::Result<()> {
    let mut saved = load();
    saved["schedule"] = json!({"on": on, "start": format_time(start), "end": format_time(end)});
    if let Some(saved) = saved.as_object_mut() {
        saved.remove("applied");
    }
    save(&saved)?;
    apply_schedule(local_minute()).map(|_| ())
}

/// Turns the schedule on at these times, or off (keeping its times).
pub(crate) fn set_schedule(times: Option<(u16, u16)>) -> std::io::Result<()> {
    match times {
        Some(times) => save_schedule(true, times),
        None => save_schedule(false, schedule_times()),
    }
}

fn local_minute() -> u16 {
    crate::swarm::schedule::local_now().minute as u16
}

/// Turns quiet mode on when the schedule's time starts and off when it ends, only when that
/// changes (so a switch by hand lasts until the next start or end). Returns what it did.
pub(crate) fn apply_schedule(minute: u16) -> std::io::Result<Option<bool>> {
    let mut saved = load();
    let (on_schedule, times) = read_schedule(&saved);
    if !on_schedule {
        return Ok(None);
    }
    let on = in_times(times, minute);
    let applied = json!({"start": format_time(times.0), "end": format_time(times.1), "on": on});
    if saved["applied"] == applied {
        return Ok(None);
    }
    set(on)?;
    saved["applied"] = applied;
    save(&saved)?;
    Ok(Some(on))
}

/// Called by the hivey server's swarm tick: checks the schedule about twice a minute.
// The swarm engine doesn't run in tests (`api::server` starts it only outside them)
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn tick() {
    const SCHEDULE_CHECK: Duration = Duration::from_secs(30);
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
    match apply_schedule(local_minute()) {
        Ok(Some(on)) => tracing::info!(on, "hivey quiet mode schedule"),
        Ok(None) => {}
        Err(err) => tracing::warn!(%err, "hivey quiet mode schedule"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> u16 {
        parse_time(text).expect("time")
    }

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
        assert_eq!(apply_schedule(at("23")).expect("no schedule"), None);
        assert_eq!(schedule_times(), DEFAULT_TIMES);
        set_schedule(Some((at("22:30"), at("6:45")))).expect("schedule");
        assert_eq!(schedule(), Some((at("22:30"), at("6:45"))));
        // Start from outside the times, whatever time the test runs at
        apply_schedule(at("12")).expect("day");
        assert!(!is_on());
        assert_eq!(apply_schedule(at("22:29")).expect("not yet"), None);
        assert_eq!(apply_schedule(at("22:30")).expect("start"), Some(true));
        assert!(is_on());
        assert_eq!(apply_schedule(at("23")).expect("still night"), None);
        // Off by hand at night: stays off until the night ends
        set(false).expect("off by hand");
        assert_eq!(apply_schedule(at("2")).expect("night"), None);
        assert!(!is_on());
        assert_eq!(apply_schedule(at("6:45")).expect("end"), Some(false));
        // On by hand in the day: stays on until the next night starts, then ends with it
        set(true).expect("on by hand");
        assert_eq!(apply_schedule(at("12")).expect("day"), None);
        assert!(is_on());
        assert_eq!(apply_schedule(at("22:30")).expect("start"), Some(true));
        assert_eq!(apply_schedule(at("6:45")).expect("end"), Some(false));
        assert!(!is_on());
        // Off keeps the times for next time
        set_schedule(None).expect("no schedule");
        assert_eq!(schedule(), None);
        assert_eq!(schedule_times(), (at("22:30"), at("6:45")));
        assert_eq!(apply_schedule(at("23")).expect("no schedule"), None);
        assert!(!is_on());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_the_older_hours_format() {
        let saved = json!({"schedule": [22, 7]});
        assert_eq!(read_schedule(&saved), (true, (at("22"), at("7"))));
        assert_eq!(read_schedule(&json!({})), (false, DEFAULT_TIMES));
    }

    #[test]
    fn parses_and_formats_times() {
        assert_eq!(parse_time("22"), Some(22 * 60));
        assert_eq!(parse_time("7"), Some(7 * 60));
        assert_eq!(parse_time("22:30"), Some(22 * 60 + 30));
        assert_eq!(parse_time("2230"), Some(22 * 60 + 30));
        assert_eq!(parse_time("730"), Some(7 * 60 + 30));
        assert_eq!(parse_time("0645"), Some(6 * 60 + 45));
        for bad in ["", "24", "7:5", "2260", "12345", "ab", "-1", "7:"] {
            assert_eq!(parse_time(bad), None, "{bad:?}");
        }
        assert_eq!(parse_times("22:30-7"), Some((22 * 60 + 30, 7 * 60)));
        assert_eq!(format_time(at("7:05")), "07:05");
        assert_eq!(describe(Some((at("22:30"), at("7")))), "22:30 → 07:00");
        assert_eq!(describe(None), "off");
        assert_eq!(shift(at("23:45"), 30), at("0:15"));
        assert_eq!(shift(at("0:15"), -30), at("23:45"));
    }

    #[test]
    fn times_wrap_midnight() {
        let night = (at("22:30"), at("6:45"));
        assert!(in_times(night, at("23")));
        assert!(in_times(night, at("0")));
        assert!(!in_times(night, at("6:45")));
        assert!(!in_times(night, at("12")));
        assert!(in_times((at("13"), at("14")), at("13:59")));
        assert!(!in_times((at("5"), at("5")), at("5")));
    }
}
