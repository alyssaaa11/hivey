//! Quiet mode: one switch for a quiet night. While `~/.hivey/quiet` exists nothing is spoken
//! (`hivey voice say`, so the pet and agents' summaries), hivey plays no notification sounds,
//! and the Slack relay holds its posts (it sends one catch-up message when quiet mode ends).
//! Agents keep working. Toggled by the hivey menu (quiet mode) and `hivey quiet on|off`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

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
}
