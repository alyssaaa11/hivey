//! Scheduled wake-ups for a swarm's master (or any agent): "every 15m" or "daily at 09:00".
//!
//! Stored in `<root>/.swarm/schedules.json` (owned by hivey). When one is due the engine
//! sends a bus message with the task and a status snapshot; the bus delivers it when the
//! target is idle, so a check never interrupts work and costs nothing until it is due.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Schedule {
    pub id: String,
    /// Repeat interval in seconds (`--every`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_secs: Option<u64>,
    /// Daily local time `HH:MM` (`--at`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Agent key that receives it (default: the master).
    #[serde(default = "default_target")]
    pub to: String,
    pub task: String,
    /// Unix seconds of the last run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<u64>,
    /// Local date (`YYYY-MM-DD`) of the last daily run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_day: Option<String>,
    /// Unix seconds when it was added (an interval counts from here before the first run).
    #[serde(default)]
    pub created: u64,
}

fn default_target() -> String {
    "coordinator".into()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Schedules {
    #[serde(default)]
    pub schedules: Vec<Schedule>,
}

/// Local wall clock: (`YYYY-MM-DD`, minutes since midnight).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalTime {
    pub day: String,
    pub minute: u32,
}

pub(crate) fn path(root: &Path) -> PathBuf {
    root.join(".swarm").join("schedules.json")
}

pub(crate) fn load(root: &Path) -> Schedules {
    std::fs::read_to_string(path(root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn save(root: &Path, schedules: &Schedules) -> Result<(), String> {
    let file = path(root);
    let tmp = file.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(schedules).map_err(|err| err.to_string())? + "\n";
    std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, &file))
        .map_err(|err| format!("cannot write {}: {err}", file.display()))
}

/// `90s`, `15m`, `2h`, `1d` (a bare number means minutes); at least one minute.
pub(crate) fn parse_every(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (number, unit) = text
        .find(|c: char| !c.is_ascii_digit())
        .map_or((text, "m"), |i| (&text[..i], &text[i..]));
    let value: u64 = number
        .parse()
        .map_err(|_| format!("bad interval {text:?} (e.g. 15m, 2h)"))?;
    let seconds = match unit {
        "s" => value,
        "m" | "min" => value * 60,
        "h" => value * 3600,
        "d" => value * 86_400,
        _ => return Err(format!("bad interval unit in {text:?} (use s, m, h, d)")),
    };
    if seconds < 60 {
        return Err("the shortest interval is 1m".into());
    }
    Ok(seconds)
}

/// `HH:MM` (24 h) → minutes since midnight.
pub(crate) fn parse_at(text: &str) -> Result<u32, String> {
    let (hours, minutes) = text
        .trim()
        .split_once(':')
        .ok_or_else(|| format!("bad time {text:?} (use HH:MM)"))?;
    let hours: u32 = hours.parse().map_err(|_| format!("bad time {text:?}"))?;
    let minutes: u32 = minutes.parse().map_err(|_| format!("bad time {text:?}"))?;
    if hours > 23 || minutes > 59 {
        return Err(format!("bad time {text:?}"));
    }
    Ok(hours * 60 + minutes)
}

impl Schedule {
    pub(crate) fn describe(&self) -> String {
        match (&self.every_secs, &self.at) {
            (Some(secs), _) if secs % 3600 == 0 => format!("every {}h", secs / 3600),
            (Some(secs), _) if secs % 60 == 0 => format!("every {}m", secs / 60),
            (Some(secs), _) => format!("every {secs}s"),
            (None, Some(at)) => format!("daily at {at}"),
            (None, None) => "manual".into(),
        }
    }

    pub(crate) fn is_due(&self, now: u64, local: &LocalTime) -> bool {
        if let Some(every) = self.every_secs {
            return now >= self.last_run.unwrap_or(self.created).saturating_add(every);
        }
        if let Some(at) = self.at.as_deref().and_then(|at| parse_at(at).ok()) {
            return local.minute >= at && self.last_day.as_deref() != Some(local.day.as_str());
        }
        false
    }

    pub(crate) fn mark_run(&mut self, now: u64, local: &LocalTime) {
        self.last_run = Some(now);
        if self.at.is_some() {
            self.last_day = Some(local.day.clone());
        }
    }
}

/// The local wall clock (calendar day and minute), for daily schedules.
pub(crate) fn local_now() -> LocalTime {
    #[cfg(unix)]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as libc::time_t)
            .unwrap_or_default();
        // SAFETY: localtime_r writes into the zeroed struct we own and reads only `now`.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
            return LocalTime {
                day: format!(
                    "{:04}-{:02}-{:02}",
                    tm.tm_year + 1900,
                    tm.tm_mon + 1,
                    tm.tm_mday
                ),
                minute: (tm.tm_hour * 60 + tm.tm_min) as u32,
            };
        }
    }
    // Fallback (non-Unix or failure): UTC.
    let now = time::OffsetDateTime::now_utc();
    LocalTime {
        day: format!(
            "{:04}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day()
        ),
        minute: u32::from(now.hour()) * 60 + u32::from(now.minute()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: &str, minute: u32) -> LocalTime {
        LocalTime {
            day: day.into(),
            minute,
        }
    }

    #[test]
    fn parses_intervals_and_times() {
        assert_eq!(parse_every("15m").unwrap(), 900);
        assert_eq!(parse_every("2h").unwrap(), 7200);
        assert_eq!(parse_every("30").unwrap(), 1800, "bare number = minutes");
        assert!(parse_every("10s").is_err(), "shorter than a minute");
        assert!(parse_every("5w").is_err());
        assert_eq!(parse_at("09:30").unwrap(), 570);
        assert!(parse_at("24:00").is_err());
        assert!(parse_at("9h").is_err());
    }

    #[test]
    fn intervals_count_from_creation_then_from_the_last_run() {
        let mut s = Schedule {
            id: "s1".into(),
            every_secs: Some(600),
            at: None,
            to: "coordinator".into(),
            task: "check".into(),
            last_run: None,
            last_day: None,
            created: 1000,
        };
        let local = at("2026-10-01", 0);
        assert!(!s.is_due(1599, &local));
        assert!(s.is_due(1600, &local));
        s.mark_run(1600, &local);
        assert!(!s.is_due(2199, &local));
        assert!(s.is_due(2200, &local));
        assert_eq!(s.describe(), "every 10m");
    }

    #[test]
    fn daily_runs_once_per_day_after_its_time() {
        let mut s = Schedule {
            id: "s2".into(),
            every_secs: None,
            at: Some("09:00".into()),
            to: "coordinator".into(),
            task: "morning report".into(),
            last_run: None,
            last_day: None,
            created: 0,
        };
        assert!(!s.is_due(0, &at("2026-10-01", 8 * 60 + 59)));
        assert!(s.is_due(0, &at("2026-10-01", 9 * 60)));
        s.mark_run(0, &at("2026-10-01", 9 * 60));
        assert!(!s.is_due(0, &at("2026-10-01", 23 * 60)), "once per day");
        assert!(s.is_due(0, &at("2026-10-02", 9 * 60 + 5)), "next day");
        assert_eq!(s.describe(), "daily at 09:00");
    }

    #[test]
    fn local_time_is_sane() {
        let now = local_now();
        assert_eq!(now.day.len(), 10);
        assert!(now.minute < 24 * 60);
    }
}
