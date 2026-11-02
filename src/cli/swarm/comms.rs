//! `hivey swarm comms <slug> …`: how much a swarm's agents may message each other (manifest
//! `"comms"`, read by the message engine and the Slack relay), so a swarm doesn't flood its
//! bus or its Slack channel with chatter. See `crate::swarm::model::Comms`.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::{call, result, swarms, take_value};
use crate::swarm::model::{Comms, SLACK_SCOPES};

const USAGE: &str = "usage: hivey swarm comms <slug> [--preset quiet|normal|chatty] \
[--rate-per-min N] [--pair-wakes N] [--pair-window-min N] [--hourly-wakes N] \
[--broadcasts-per-hour N] [--dedupe-min N] [--slack masters|all|off]";

/// Numeric flags: (flag, manifest field, whether 0 is allowed (= no limit / off)).
const NUMBERS: [(&str, &str, bool); 6] = [
    ("--rate-per-min", "rate_per_min", false),
    ("--pair-wakes", "pair_wakes", false),
    ("--pair-window-min", "pair_window_min", false),
    ("--hourly-wakes", "hourly_wakes", true),
    ("--broadcasts-per-hour", "broadcasts_per_hour", true),
    ("--dedupe-min", "dedupe_min", true),
];

pub(super) fn comms(args: &[String]) -> std::io::Result<i32> {
    let mut rest = args.to_vec();
    let changes = match parse(&mut rest) {
        Ok(changes) => changes,
        Err(err) => {
            eprintln!("error: {err}\n{USAGE}");
            return Ok(2);
        }
    };
    let [slug] = rest.as_slice() else {
        eprintln!("{USAGE}");
        return Ok(2);
    };
    let Some(swarm) = swarms()?.into_iter().find(|s| s["slug"] == slug.as_str()) else {
        eprintln!("error: no swarm or agent {slug:?} (hivey swarm directory)");
        return Ok(1);
    };
    let root = PathBuf::from(swarm["root"].as_str().unwrap_or_default());
    if changes.is_empty() {
        return match read_manifest(&root) {
            Ok(manifest) => {
                print_effective(&manifest);
                Ok(0)
            }
            Err(err) => {
                eprintln!("error: {err}");
                Ok(1)
            }
        };
    }
    match update_comms(&root, changes) {
        Ok(manifest) => {
            let response = call("import", json!({ "root": root }))?;
            if result(&response).is_none() {
                return Ok(1);
            }
            print_effective(&manifest);
            Ok(0)
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

/// The fields to merge into the manifest's `"comms"`: a preset first, then single flags.
fn parse(args: &mut Vec<String>) -> Result<Map<String, Value>, String> {
    let mut changes = Map::new();
    if let Some(name) = take_value(args, "--preset")? {
        let preset = Comms::preset(&name)
            .ok_or(format!("unknown preset {name:?} (quiet, normal, chatty)"))?;
        let Value::Object(fields) = preset.to_value() else {
            unreachable!("Comms::to_value is an object");
        };
        changes.extend(fields);
    }
    for (flag, field, zero_ok) in NUMBERS {
        if let Some(text) = take_value(args, flag)? {
            let n: u64 = text
                .parse()
                .map_err(|_| format!("{flag} needs a whole number, got {text:?}"))?;
            if n == 0 && !zero_ok {
                return Err(format!("{flag} must be at least 1"));
            }
            changes.insert(field.into(), json!(n));
        }
    }
    if let Some(scope) = take_value(args, "--slack")? {
        if !SLACK_SCOPES.contains(&scope.as_str()) {
            return Err(format!(
                "--slack must be one of {}",
                SLACK_SCOPES.join(", ")
            ));
        }
        changes.insert("slack".into(), json!(scope));
    }
    if let Some(flag) = args.iter().find(|arg| arg.starts_with("--")) {
        return Err(format!("unknown option {flag}"));
    }
    Ok(changes)
}

fn print_effective(manifest: &Value) {
    let comms = Comms::from_value(manifest.get("comms"));
    println!(
        "{}",
        serde_json::to_string_pretty(&comms.to_value()).unwrap_or_default()
    );
}

fn read_manifest(root: &Path) -> Result<Value, String> {
    let path = crate::swarm::model::manifest_path(root);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("invalid JSON in {}: {err}", path.display()))
}

/// Merges `changes` into the manifest's `"comms"` (atomic write); returns the new manifest.
fn update_comms(root: &Path, changes: Map<String, Value>) -> Result<Value, String> {
    let path = crate::swarm::model::manifest_path(root);
    let mut manifest = read_manifest(root)?;
    merge(&mut manifest, changes);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    )
    .and_then(|_| std::fs::rename(&tmp, &path))
    .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(manifest)
}

fn merge(manifest: &mut Value, changes: Map<String, Value>) {
    if !manifest["comms"].is_object() {
        manifest["comms"] = json!({});
    }
    for (key, value) in changes {
        manifest["comms"][key] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn preset_then_flags_override_it() {
        let mut rest = args(&["app", "--preset", "quiet", "--hourly-wakes", "0"]);
        let changes = parse(&mut rest).unwrap();
        assert_eq!(rest, ["app"]);
        assert_eq!(changes["rate_per_min"], 5);
        assert_eq!(changes["hourly_wakes"], 0, "0 = no hourly limit");
        assert_eq!(changes["slack"], "masters");
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse(&mut args(&["app", "--rate-per-min", "0"])).is_err());
        assert!(parse(&mut args(&["app", "--dedupe-min", "soon"])).is_err());
        assert!(parse(&mut args(&["app", "--slack", "loud"])).is_err());
        assert!(parse(&mut args(&["app", "--preset", "loud"])).is_err());
        assert!(parse(&mut args(&["app", "--rate", "3"])).is_err());
    }

    #[test]
    fn merges_into_existing_comms_and_keeps_other_fields() {
        let mut manifest = json!({"slug": "app", "comms": {"dedupe_min": 3, "slack": "off"}});
        let mut changes = Map::new();
        changes.insert("rate_per_min".into(), json!(7));
        merge(&mut manifest, changes);
        assert_eq!(manifest["slug"], "app");
        let comms = Comms::from_value(manifest.get("comms"));
        assert_eq!(
            (comms.rate_per_min, comms.dedupe_min),
            (7, 3),
            "unchanged fields stay"
        );
        assert_eq!(comms.slack.as_deref(), Some("off"));
    }
}
