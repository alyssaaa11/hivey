//! `hiver swarm directory` and `hiver swarm profile`: who else is registered in this hiver,
//! what each swarm or solo agent does (manifest `"profile"`), and whether it is busy, so an
//! agent can judge where to send work — after asking the user.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{call, canonical_pane_id, launch::split_list, result, swarms, take_value};

pub(super) const DIRECTORY_RULE: &str = "Ask the user before sending work to any of these, \
and don't disturb one that is working.";

/// One directory entry, built from a `list` swarm.
fn entry(swarm: &Value, own_pane: Option<&str>) -> Value {
    let agents: Vec<&Value> = swarm["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|agent| agent["role"] != "script")
        .collect();
    let master = agents.iter().find(|agent| agent["role"] == "master");
    let status = master
        .and_then(|master| master["status"].as_str())
        .unwrap_or("gone");
    let working = agents
        .iter()
        .filter(|agent| agent["status"] == "working")
        .count();
    let slug = swarm["slug"].as_str().unwrap_or("?");
    let profile = &swarm["profile"];
    let list = |field: &str| -> Vec<String> {
        profile[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    };
    let you = own_pane.is_some_and(|own| {
        agents
            .iter()
            .any(|agent| agent["pane_id"].as_str() == Some(own))
    });
    json!({
        "slug": slug,
        "kind": if swarm["solo"] == true { "agent" } else { "swarm" },
        "description": profile["description"].as_str().unwrap_or(""),
        "skills": list("skills"),
        "tools": list("tools"),
        "status": status,
        "busy": matches!(status, "working" | "blocked"),
        "working": working,
        "members": agents.len(),
        "paused": swarm["paused"] == true,
        "address": format!("{slug}/master"),
        "root": swarm["root"],
        "you": you,
    })
}

fn own_pane() -> Option<String> {
    std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR)
        .ok()
        .map(|pane| canonical_pane_id(&pane))
}

pub(super) fn directory(json_out: bool) -> std::io::Result<i32> {
    let own = own_pane();
    let entries: Vec<Value> = swarms()?
        .iter()
        .map(|swarm| entry(swarm, own.as_deref()))
        .collect();
    if json_out {
        println!("{}", json!({ "entries": entries, "rule": DIRECTORY_RULE }));
        return Ok(0);
    }
    if entries.is_empty() {
        println!("no swarms or agents registered (hiver swarm register <root>)");
        return Ok(0);
    }
    println!("hiver directory · {} registered\n", entries.len());
    for entry in &entries {
        print_entry(entry);
    }
    println!("{DIRECTORY_RULE}");
    println!("Send with: hiver msg send <address> \"…\"");
    Ok(0)
}

fn print_entry(entry: &Value) {
    let kind = entry["kind"].as_str().unwrap_or("swarm");
    let glyph = if kind == "agent" { "★" } else { "◆" };
    let status = entry["status"].as_str().unwrap_or("gone");
    let state = match (kind, entry["paused"] == true) {
        (_, true) => format!("{status}, paused"),
        ("agent", _) => status.to_string(),
        _ => format!(
            "{status}, {}/{} working",
            entry["working"], entry["members"]
        ),
    };
    let busy = if entry["busy"] == true {
        "  ⚠ busy: don't disturb"
    } else {
        ""
    };
    let you = if entry["you"] == true { "  (you)" } else { "" };
    println!(
        "{glyph} {:<20} {kind:<5} {state}{busy}{you}",
        entry["slug"].as_str().unwrap_or("?")
    );
    match entry["description"].as_str().filter(|d| !d.is_empty()) {
        Some(about) => println!("    {about}"),
        None => println!("    (no description: hiver swarm profile <slug> --description …)"),
    }
    let join = |field: &str| {
        entry[field]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    let (skills, tools) = (join("skills"), join("tools"));
    if !skills.is_empty() {
        println!("    skills: {skills}");
    }
    if !tools.is_empty() {
        println!("    tools:  {tools}");
    }
    println!("    address: {}\n", entry["address"].as_str().unwrap_or(""));
}

/// `hiver swarm profile <slug> [--description T] [--skills a,b] [--tools x,y]`.
pub(super) fn profile(args: &[String]) -> std::io::Result<i32> {
    let mut rest = args.to_vec();
    let parsed = (|| {
        Ok::<_, String>((
            take_value(&mut rest, "--description")?,
            take_value(&mut rest, "--skills")?,
            take_value(&mut rest, "--tools")?,
        ))
    })();
    let (description, skills, tools) = match parsed {
        Ok(values) => values,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(2);
        }
    };
    let [slug] = rest.as_slice() else {
        eprintln!(
            "usage: hiver swarm profile <slug> [--description TEXT] [--skills a,b] [--tools x,y]"
        );
        return Ok(2);
    };
    let Some(swarm) = swarms()?.into_iter().find(|s| s["slug"] == slug.as_str()) else {
        eprintln!("error: no swarm or agent {slug:?} (hiver swarm directory)");
        return Ok(1);
    };
    let root = PathBuf::from(swarm["root"].as_str().unwrap_or_default());
    if description.is_none() && skills.is_none() && tools.is_none() {
        println!(
            "{}",
            serde_json::to_string_pretty(&swarm["profile"]).unwrap_or_default()
        );
        return Ok(0);
    }
    let mut changes = serde_json::Map::new();
    if let Some(description) = description {
        changes.insert("description".into(), json!(description));
    }
    for (field, value) in [("skills", skills), ("tools", tools)] {
        if let Some(value) = value {
            changes.insert(field.into(), json!(split_list(&value)));
        }
    }
    match update_profile(&root, changes) {
        Ok(profile) => {
            let response = call("import", json!({ "root": root }))?;
            if result(&response).is_none() {
                return Ok(1);
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&profile).unwrap_or_default()
            );
            Ok(0)
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

/// Merges `changes` into the manifest's `"profile"`; returns the new profile.
fn update_profile(root: &Path, changes: serde_json::Map<String, Value>) -> Result<Value, String> {
    let path = crate::swarm::model::manifest_path(root);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let mut manifest: Value = serde_json::from_str(&text)
        .map_err(|err| format!("invalid JSON in {}: {err}", path.display()))?;
    if !manifest["profile"].is_object() {
        manifest["profile"] = json!({});
    }
    for (key, value) in changes {
        manifest["profile"][key] = value;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    )
    .and_then(|_| std::fs::rename(&tmp, &path))
    .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(manifest["profile"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_reports_kind_profile_and_busy_master() {
        let swarm = json!({
            "slug": "seo", "root": "/tmp/seo", "solo": true, "paused": false,
            "profile": { "description": "Audits SEO", "skills": ["archify"], "tools": ["gh"] },
            "agents": [
                { "key": "seo", "role": "master", "status": "working", "pane_id": "w1:p1" },
                { "key": "dashboard", "role": "script", "status": "script", "pane_id": "w1:p2" },
            ],
        });
        let entry = entry(&swarm, Some("w1:p1"));
        assert_eq!(entry["kind"], "agent");
        assert_eq!(entry["busy"], true);
        assert_eq!(entry["members"], 1, "addons are not members");
        assert_eq!(entry["skills"], json!(["archify"]));
        assert_eq!(entry["address"], "seo/master");
        assert_eq!(entry["you"], true);
    }

    #[test]
    fn swarm_without_profile_is_idle_when_its_master_is() {
        let swarm = json!({
            "slug": "ideas", "root": "/tmp/ideas",
            "agents": [
                { "key": "coordinator", "role": "master", "status": "idle" },
                { "key": "scout", "role": "worker", "status": "working" },
            ],
        });
        let entry = entry(&swarm, None);
        assert_eq!(entry["kind"], "swarm");
        assert_eq!(entry["busy"], false);
        assert_eq!(entry["working"], 1);
        assert_eq!(entry["description"], "");
    }

    #[test]
    fn update_profile_merges_fields() {
        let root = std::env::temp_dir().join(format!("hiver-profile-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".swarm")).unwrap();
        std::fs::write(
            crate::swarm::model::manifest_path(&root),
            r#"{"slug":"x","profile":{"description":"old","tools":["gh"]}}"#,
        )
        .unwrap();
        let mut changes = serde_json::Map::new();
        changes.insert("description".into(), json!("new"));
        let profile = update_profile(&root, changes).unwrap();
        assert_eq!(profile, json!({ "description": "new", "tools": ["gh"] }));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
