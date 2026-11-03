//! Agents on non-Anthropic models through Claude Code: OpenRouter (DeepSeek and others) and a
//! local MLX model (Muse Glimmer 30B, 6-bit).
//!
//! A Claude agent whose model is one of these runs Claude Code against an Anthropic-compatible
//! API. Its folder gets a `.claude/settings.local.json` with that API's base URL and every model
//! class pinned to the model, so subagents and background calls use it too.
//!
//! - `deepseek` (DeepSeek V4.1 Flash 1017) and `openrouter:<id>`: OpenRouter. An `apiKeyHelper`
//!   prints the key, so the key itself is never written down. The command comes from
//!   `~/.hivey/openrouter.json` (`{"key_command": "envsave get es_…"}`), else it reads
//!   `$OPENROUTER_API_KEY` from the agent's environment.
//! - `muse` (Muse Glimmer 30B, MLX 6-bit, `~/models/muse-glimmer-30b-mlx-6bit`): served on this
//!   computer by `mlx_vlm.server` on 127.0.0.1:8092, started by hivey when it isn't running
//!   (log: `~/.hivey/logs/muse-server.log`). All agents share the one server.

use serde_json::{json, Map, Value};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// OpenRouter's Anthropic skin (no `/v1`: that one is the OpenAI-compatible API).
pub(crate) const OPENROUTER_URL: &str = "https://openrouter.ai/api";
/// The OpenRouter id `deepseek` stands for.
pub(crate) const DEEPSEEK: &str = "deepseek/deepseek-v4.1-flash-1017";
/// Where `muse` is served (mlx-vlm's Anthropic `/v1/messages`).
pub(crate) const MUSE_PORT: u16 = 8092;
/// The MLX model folder under `$HOME` that `muse` stands for.
pub(crate) const MUSE_MODEL_DIR: &str = "models/muse-glimmer-30b-mlx-6bit";
/// Muse Glimmer's context window (Claude Code would otherwise assume 200k and overflow it).
const MUSE_CONTEXT: u32 = 131_072;
/// Loading 25 GB of weights takes about a minute; allow for a cold disk.
const MUSE_START_WAIT: Duration = Duration::from_secs(300);
/// Model the agent is started with: Claude Code maps `opus` to the pinned model, which keeps
/// `[1m]` suffixes and paths out of the shell command line.
pub(crate) const LAUNCH_MODEL: &str = "opus";
const FALLBACK_KEY_COMMAND: &str = "printenv OPENROUTER_API_KEY";
/// Env vars this module owns in an agent's settings.local.json.
const OWNED_VARS: [&str; 9] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];
const MODEL_VARS: [&str; 5] = [
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

/// Where a Claude agent's requests go instead of Anthropic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Route {
    pub(crate) base_url: String,
    /// The model id as Claude Code should send it (`[1m]` marks a 1M window).
    pub(crate) model: String,
    pub(crate) auth: Auth,
    pub(crate) max_context: Option<u32>,
    /// A model this computer serves, started on demand.
    pub(crate) local: Option<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Auth {
    /// A shell command that prints the key (Claude Code's `apiKeyHelper`).
    KeyCommand(String),
    /// A fixed token: the local server checks none.
    Token(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Local {
    pub(crate) port: u16,
    pub(crate) model_dir: PathBuf,
}

/// The route a hivey model name stands for; `None` for an ordinary Claude model.
pub(crate) fn resolve(model: &str) -> Option<Route> {
    resolve_with(model, &home_dir(), key_command)
}

fn resolve_with(model: &str, home: &Path, key: impl Fn() -> String) -> Option<Route> {
    let openrouter = |id: String| Route {
        base_url: OPENROUTER_URL.to_string(),
        model: id,
        auth: Auth::KeyCommand(key()),
        max_context: None,
        local: None,
    };
    match model.trim() {
        "deepseek" | "deepseek-flash" | "deepseek-v4.1-flash" => {
            Some(openrouter(format!("{DEEPSEEK}[1m]")))
        }
        "muse" | "muse-glimmer" | "muse-glimmer-6bit" => {
            let model_dir = home.join(MUSE_MODEL_DIR);
            Some(Route {
                base_url: format!("http://127.0.0.1:{MUSE_PORT}"),
                // mlx-vlm serves a request only when its model names the loaded folder.
                model: model_dir.display().to_string(),
                auth: Auth::Token("local"),
                max_context: Some(MUSE_CONTEXT),
                local: Some(Local {
                    port: MUSE_PORT,
                    model_dir,
                }),
            })
        }
        other => other
            .strip_prefix("openrouter:")
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| openrouter(id.to_string())),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The shell command that prints the OpenRouter key.
pub(crate) fn key_command() -> String {
    std::fs::read_to_string(home_dir().join(".hivey/openrouter.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|config| config["key_command"].as_str().map(str::to_string))
        .filter(|command| !command.trim().is_empty())
        .unwrap_or_else(|| FALLBACK_KEY_COMMAND.to_string())
}

/// Starts the local model server unless it is already listening; waits until it serves.
pub(crate) fn ensure_server(local: &Local) -> Result<(), String> {
    if listening(local.port) {
        return Ok(());
    }
    if !local.model_dir.join("config.json").is_file() {
        return Err(format!("no MLX model in {}", local.model_dir.display()));
    }
    let server = find_server().ok_or(
        "mlx_vlm.server not found (install it with `uv tool install mlx-vlm`)".to_string(),
    )?;
    let logs = home_dir().join(".hivey/logs");
    std::fs::create_dir_all(&logs).map_err(|err| format!("{}: {err}", logs.display()))?;
    let log_path = logs.join("muse-server.log");
    let log =
        std::fs::File::create(&log_path).map_err(|err| format!("{}: {err}", log_path.display()))?;
    let err_log = log.try_clone().map_err(|err| err.to_string())?;
    eprintln!(
        "  starting the local model server on 127.0.0.1:{} (log: {})",
        local.port,
        log_path.display()
    );
    let mut child = std::process::Command::new(server)
        .args(["--host", "127.0.0.1", "--port", &local.port.to_string()])
        .arg("--model")
        .arg(&local.model_dir)
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err_log)
        .spawn()
        .map_err(|err| format!("cannot start mlx_vlm.server: {err}"))?;
    let started = Instant::now();
    while started.elapsed() < MUSE_START_WAIT {
        if listening(local.port) {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "mlx_vlm.server exited ({status}); see {}",
                log_path.display()
            ));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(format!(
        "mlx_vlm.server is still loading after {}s; see {}",
        MUSE_START_WAIT.as_secs(),
        log_path.display()
    ))
}

fn listening(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

fn find_server() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    on_path
        .into_iter()
        .chain([home_dir().join(".local/bin")])
        .map(|dir| dir.join("mlx_vlm.server"))
        .find(|path| path.is_file())
}

/// Points the Claude Code agent in `home` at `route`, or takes a previous setup of ours out
/// again when `route` is `None`. Other settings are kept.
pub(crate) fn apply(home: &Path, route: Option<&Route>) -> Result<(), String> {
    if !home.is_dir() {
        return Ok(());
    }
    let path = home.join(".claude").join("settings.local.json");
    let existing = std::fs::read_to_string(&path).ok();
    if existing.is_none() && route.is_none() {
        return Ok(());
    }
    let mut settings = match &existing {
        Some(text) => serde_json::from_str::<Value>(text)
            .map_err(|err| format!("{}: {err}", path.display()))?,
        None => json!({}),
    };
    if !configure(&mut settings, route) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|err| format!("{}: {err}", path.display()))
}

/// Whether these settings route through one of ours (OpenRouter or a local server).
fn ours(vars: &Map<String, Value>) -> bool {
    vars.get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .is_some_and(|url| url == OPENROUTER_URL || url.starts_with("http://127.0.0.1:"))
}

/// Edits the settings in place; whether anything changed.
fn configure(settings: &mut Value, route: Option<&Route>) -> bool {
    let before = settings.clone();
    if !settings.is_object() {
        *settings = json!({});
    }
    let Some(object) = settings.as_object_mut() else {
        return false;
    };
    let mut vars = match object.remove("env") {
        Some(Value::Object(vars)) => vars,
        _ => Map::new(),
    };
    // Undo a previous setup of ours, so switching models leaves nothing stale behind.
    if ours(&vars) {
        for var in OWNED_VARS {
            vars.remove(var);
        }
        object.remove("apiKeyHelper");
    }
    if let Some(route) = route {
        vars.insert("ANTHROPIC_BASE_URL".into(), json!(route.base_url));
        // Empty, so a key in the user's environment isn't sent to someone else's API.
        vars.insert("ANTHROPIC_API_KEY".into(), json!(""));
        for var in MODEL_VARS {
            vars.insert(var.into(), json!(route.model));
        }
        if let Some(tokens) = route.max_context {
            vars.insert(
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS".into(),
                json!(tokens.to_string()),
            );
        }
        match &route.auth {
            Auth::KeyCommand(command) => {
                object.insert("apiKeyHelper".into(), json!(command));
            }
            Auth::Token(token) => {
                vars.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(token));
            }
        }
    }
    if !vars.is_empty() {
        object.insert("env".into(), Value::Object(vars));
    }
    *settings != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_test(model: &str) -> Option<Route> {
        resolve_with(model, Path::new("/h"), || "envsave get es_k".to_string())
    }

    #[test]
    fn model_names_resolve_to_routes() {
        let deepseek = resolve_test("deepseek").expect("deepseek");
        assert_eq!(deepseek.model, "deepseek/deepseek-v4.1-flash-1017[1m]");
        assert_eq!(deepseek.base_url, OPENROUTER_URL);
        assert_eq!(deepseek.auth, Auth::KeyCommand("envsave get es_k".into()));
        assert_eq!(
            resolve_test("openrouter:qwen/qwen3-coder").map(|r| r.model),
            Some("qwen/qwen3-coder".to_string())
        );
        let muse = resolve_test("muse").expect("muse");
        assert_eq!(muse.model, "/h/models/muse-glimmer-30b-mlx-6bit");
        assert_eq!(muse.base_url, "http://127.0.0.1:8092");
        assert_eq!(muse.max_context, Some(131_072));
        assert!(muse.local.is_some());
        assert_eq!(resolve_test("opus"), None);
        assert_eq!(resolve_test("openrouter:"), None);
    }

    #[test]
    fn configure_switches_routes_and_undoes_only_its_own_setup() {
        let original = json!({ "env": { "AGENTS_TTS": "0" }, "permissions": {} });
        let mut settings = original.clone();
        let deepseek = resolve_test("deepseek").expect("deepseek");
        assert!(configure(&mut settings, Some(&deepseek)));
        assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], OPENROUTER_URL);
        assert_eq!(settings["env"]["ANTHROPIC_API_KEY"], "");
        assert_eq!(settings["env"]["AGENTS_TTS"], "0");
        assert_eq!(
            settings["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
            deepseek.model.as_str()
        );
        assert_eq!(settings["apiKeyHelper"], "envsave get es_k");
        assert!(!configure(&mut settings, Some(&deepseek)));

        let muse = resolve_test("muse").expect("muse");
        assert!(configure(&mut settings, Some(&muse)));
        assert_eq!(settings.get("apiKeyHelper"), None);
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "local");
        assert_eq!(settings["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "131072");

        assert!(configure(&mut settings, None));
        assert_eq!(settings, original);

        let mut other =
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://proxy" }, "apiKeyHelper": "x" });
        assert!(!configure(&mut other, None));
    }
}
