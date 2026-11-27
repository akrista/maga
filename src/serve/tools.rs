//! Extra MCP tools: per-harness state and controls, a secrets inventory (names only, never
//! values) and skills (search, install, activate per harness) through the `skills` CLI.
//!
//! Deliberately not exposed: adding or deleting MCP servers (an agent could register an
//! arbitrary command), plugin uninstall, skill removal, guidelines/refs, self-update and
//! settings, because each is either destructive, executes arbitrary code or is a decision that
//! belongs to the human at the TUI.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::{DispatchError, available_names, conflict_brief, reconcile_from_disk};
use crate::domain::model::{HarnessKind, parse_dotenv_str};
use crate::domain::skills::{self, SkillScope};
use crate::domain::storage::StoragePaths;

/// Tools that change files or install software: only available with `MAGA_MCP_MUTATIONS=1`.
pub const MUTATING: [&str; 5] = [
    "toggle_server",
    "sync",
    "set_server_harness",
    "install_skill",
    "set_skill_harness",
];

pub fn is_mutating(name: &str) -> bool {
    MUTATING.contains(&name)
}

fn harness_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "enum": HarnessKind::ALL.iter().map(|h| h.key()).collect::<Vec<_>>(),
        "description": description,
    })
}

pub fn read_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_harnesses",
            "description": "State of every harness (Antigravity, Claude Code, Copilot, Grok, OpenCode): whether maga syncs it, its MCP config file, which MCP servers are active for it, plugin counts, and how secrets reach it (a warning lists servers whose secrets must be stored as literals in that harness's local config).",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "list_plugins",
            "description": "Plugins discovered per harness with enabled state, kind and version. Optionally limited to one harness.",
            "inputSchema": {
                "type": "object",
                "properties": { "harness": harness_schema("Only this harness.") },
            },
        }),
        json!({
            "name": "list_secrets",
            "description": "Secret inventory by NAME only (values are never returned): the ${VAR} variables MCP servers reference, which servers use them, and whether each is defined in maga's .env.local, the environment, or missing. Also lists servers that still hold literal secrets.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "list_skills",
            "description": "Installed skills (global and for the project in the server's working directory) and the harnesses each one is active for.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "search_skills",
            "description": "Search skills.sh by keyword (sends the query to the skills.sh service). Returns installable packages (owner/repo@skill) with install counts.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string", "description": "Search keywords." } },
                "required": ["query"],
            },
        }),
    ]
}

pub fn write_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "set_server_harness",
            "description": "Force an MCP server on or off for ONE harness only, or return it to inheriting its global enabled flag, then sync. Same as the Space key in maga's harness view.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Server name in the canonical registry." },
                    "harness": harness_schema("Harness to override."),
                    "state": { "type": "string", "enum": ["on", "off", "inherit"] },
                },
                "required": ["name", "harness", "state"],
            },
        }),
        json!({
            "name": "install_skill",
            "description": "Install a skill package from skills.sh through the skills CLI, for the harnesses maga syncs. Installed skills are instructions your agents will follow: only install packages the user asked for.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "owner/repo or owner/repo@skill, as returned by search_skills." },
                    "scope": { "type": "string", "enum": ["global", "project"], "description": "Defaults to global; project uses the server's working directory." },
                },
                "required": ["package"],
            },
        }),
        json!({
            "name": "set_skill_harness",
            "description": "Enable or disable an installed skill for one harness (links or unlinks it; the canonical copy stays). Enabling needs the source the skills CLI recorded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Installed skill name." },
                    "harness": harness_schema("Harness to change."),
                    "enabled": { "type": "boolean" },
                    "scope": { "type": "string", "enum": ["global", "project"], "description": "Defaults to global." },
                },
                "required": ["name", "harness", "enabled"],
            },
        }),
    ]
}

/// `None` when `name` is not one of these tools.
pub fn call(
    name: &str,
    args: &Value,
    paths: &StoragePaths,
) -> Option<Result<Value, DispatchError>> {
    Some(match name {
        "list_harnesses" => list_harnesses(paths),
        "list_plugins" => list_plugins(args, paths),
        "list_secrets" => list_secrets(paths),
        "list_skills" => list_skills(),
        "search_skills" => search_skills(args),
        "set_server_harness" => set_server_harness(args, paths),
        "install_skill" => install_skill(args, paths),
        "set_skill_harness" => set_skill_harness(args),
        _ => return None,
    })
}

fn invalid(message: impl Into<String>) -> DispatchError {
    DispatchError::Rpc(-32602, message.into())
}

fn str_arg<'a>(args: &'a Value, key: &str, tool: &str) -> Result<&'a str, DispatchError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("{tool} requires a string '{key}' argument")))
}

fn harness_arg(args: &Value, key: &str, tool: &str) -> Result<HarnessKind, DispatchError> {
    let raw = str_arg(args, key, tool)?.to_ascii_lowercase();
    HarnessKind::ALL
        .into_iter()
        .find(|h| {
            raw == h.key()
                || raw == h.display_name().to_ascii_lowercase()
                || raw == h.short_name().to_ascii_lowercase()
        })
        .ok_or_else(|| {
            invalid(format!(
                "Unknown harness '{raw}'. Valid values: {}",
                HarnessKind::ALL
                    .iter()
                    .map(|h| h.key())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

fn scope_arg(args: &Value) -> Result<SkillScope, DispatchError> {
    match args.get("scope").and_then(Value::as_str) {
        None | Some("global") => Ok(SkillScope::Global),
        Some("project") => Ok(SkillScope::Project),
        Some(other) => Err(invalid(format!(
            "Invalid scope '{other}': use 'global' or 'project'"
        ))),
    }
}

fn working_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn list_harnesses(paths: &StoragePaths) -> Result<Value, DispatchError> {
    let (loaded, outcome) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    let env = paths.load_env_context();
    let secrets_dir = paths.secrets_dir();

    let harnesses: Vec<Value> = HarnessKind::ALL
        .into_iter()
        .map(|kind| {
            let tool = loaded.config.tools.get(kind);
            let config_path = paths.effective_harness_path(kind, &loaded.config);
            let (active, inactive): (Vec<_>, Vec<_>) = outcome
                .canonical
                .iter()
                .partition(|(_, cfg)| cfg.enabled_for_harness(kind));
            let literal_servers: Vec<&str> = active
                .iter()
                .filter(|(_, cfg)| {
                    crate::domain::secrets::deliver(
                        &cfg.for_harness_export(cfg!(windows)),
                        kind,
                        &env,
                        &secrets_dir,
                    )
                    .literal
                })
                .map(|(name, _)| name.as_str())
                .collect();
            let (plugins_total, plugins_enabled) =
                loaded.harness_plugins.get(&kind).map_or((0, 0), |p| {
                    (p.len(), p.iter().filter(|x| x.enabled).count())
                });
            let delivery = match crate::domain::secrets::delivery(kind) {
                crate::domain::secrets::SecretDelivery::SettingsEnv => "settings_env",
                crate::domain::secrets::SecretDelivery::FileRef => "file_reference",
                crate::domain::secrets::SecretDelivery::Literal => "literal_in_local_config",
            };
            json!({
                "harness": kind.key(),
                "name": kind.display_name(),
                "synced": tool.enabled,
                "config_file": config_path.display().to_string(),
                "config_file_exists": config_path.exists(),
                "config_path_overridden": tool
                    .config_path
                    .as_deref()
                    .is_some_and(|p| p.trim() != kind.default_config_path()),
                "reads_mcp_at_startup": kind.reads_mcp_at_startup(),
                "active_servers": active.iter().map(|(n, _)| n).collect::<Vec<_>>(),
                "inactive_servers": inactive.iter().map(|(n, _)| n).collect::<Vec<_>>(),
                "plugins": { "total": plugins_total, "enabled": plugins_enabled },
                "secrets": {
                    "delivery": delivery,
                    "how": crate::domain::secrets::describe(kind),
                    "servers_with_literal_secrets": literal_servers,
                },
            })
        })
        .collect();
    Ok(json!({ "harnesses": harnesses, "conflict_count": outcome.conflicts.len() }))
}

fn list_plugins(args: &Value, paths: &StoragePaths) -> Result<Value, DispatchError> {
    let only = match args.get("harness") {
        None | Some(Value::Null) => None,
        Some(_) => Some(harness_arg(args, "harness", "list_plugins")?),
    };
    let (loaded, _) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    let plugins: Vec<Value> = loaded
        .harness_plugins
        .iter()
        .filter(|(kind, _)| only.is_none_or(|o| o == **kind))
        .flat_map(|(kind, list)| {
            list.iter().map(move |p| {
                json!({
                    "harness": kind.key(),
                    "id": p.id,
                    "name": p.display_name,
                    "enabled": p.enabled,
                    "kind": p.kind.as_str(),
                    "version": p.version,
                    "description": p.description,
                })
            })
        })
        .collect();
    Ok(json!({ "total": plugins.len(), "plugins": plugins }))
}

fn list_secrets(paths: &StoragePaths) -> Result<Value, DispatchError> {
    let (_, outcome) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    let env = paths.load_env_context();
    let dotenv_path = paths.maga_dotenv_path();
    let dotenv_names: BTreeSet<String> = std::fs::read_to_string(&dotenv_path)
        .map(|text| parse_dotenv_str(&text).into_keys().collect())
        .unwrap_or_default();

    let mut used_by: std::collections::BTreeMap<String, Vec<&str>> = Default::default();
    for (server, cfg) in &outcome.canonical {
        for var in cfg.referenced_env_vars() {
            used_by.entry(var).or_default().push(server.as_str());
        }
    }
    let variables: Vec<Value> = used_by
        .into_iter()
        .map(|(name, servers)| {
            let source = if dotenv_names.contains(&name) {
                ".env.local"
            } else if env.contains_key(&name) {
                "environment"
            } else {
                "missing"
            };
            json!({ "name": name, "defined": source != "missing", "source": source, "used_by": servers })
        })
        .collect();
    let literal: Vec<&str> = outcome
        .canonical
        .iter()
        .filter(|(_, cfg)| cfg.has_literal_secrets())
        .map(|(name, _)| name.as_str())
        .collect();
    Ok(json!({
        "note": "Values are never returned. Add missing variables to the .env.local file below.",
        "dotenv_file": dotenv_path.display().to_string(),
        "dotenv_variables": dotenv_names,
        "variables": variables,
        "servers_with_literal_secrets": literal,
    }))
}

fn list_skills() -> Result<Value, DispatchError> {
    let installed = skills::list_installed(&working_dir()).map_err(DispatchError::Tool)?;
    let rows: Vec<Value> = installed
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "scope": s.scope,
                "source": s.source,
                "harnesses": HarnessKind::ALL
                    .iter()
                    .filter(|h| s.enabled_for(**h))
                    .map(|h| h.key())
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "total": rows.len(), "skills": rows }))
}

fn search_skills(args: &Value) -> Result<Value, DispatchError> {
    let query = str_arg(args, "query", "search_skills")?;
    if query.chars().count() > 100 || query.chars().any(char::is_control) || query.starts_with('-')
    {
        return Err(invalid(
            "'query' must be at most 100 characters, without control characters or a leading '-'",
        ));
    }
    let hits = skills::search(query, &working_dir()).map_err(DispatchError::Tool)?;
    let rows: Vec<Value> = hits
        .iter()
        .take(20)
        .map(|h| json!({ "package": h.package, "installs": h.installs, "url": h.url }))
        .collect();
    Ok(json!({ "query": query, "total": rows.len(), "results": rows }))
}

/// `owner/repo` or `owner/repo@skill`: no URLs, local paths or option-looking input.
fn validate_package(package: &str) -> Result<(), String> {
    let ok_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    let (repo, skill) = match package.split_once('@') {
        Some((repo, skill)) => (repo, Some(skill)),
        None => (package, None),
    };
    let valid_repo = repo.split_once('/').is_some_and(|(owner, name)| {
        [owner, name].iter().all(|p| {
            !p.is_empty()
                && !p.starts_with('-')
                && !matches!(*p, "." | "..")
                && p.chars().all(ok_char)
        })
    });
    let valid_skill =
        skill.is_none_or(|s| !s.is_empty() && !s.starts_with('-') && s.chars().all(ok_char));
    if valid_repo && valid_skill {
        Ok(())
    } else {
        Err(format!(
            "Invalid package '{package}': expected owner/repo or owner/repo@skill (letters, digits, '-', '_', '.')"
        ))
    }
}

fn install_skill(args: &Value, paths: &StoragePaths) -> Result<Value, DispatchError> {
    let package = str_arg(args, "package", "install_skill")?;
    validate_package(package).map_err(DispatchError::Tool)?;
    let scope = scope_arg(args)?;
    let loaded = crate::domain::storage::LoadedConfigs::load(paths).map_err(DispatchError::Tool)?;
    let harnesses = loaded.config.tools.enabled_harnesses();
    skills::run_skills(
        &skills::install_args(package, scope, &harnesses),
        &working_dir(),
    )
    .map_err(DispatchError::Tool)?;
    Ok(json!({
        "ok": true,
        "package": package,
        "scope": if scope == SkillScope::Project { "project" } else { "global" },
        "harnesses": harnesses.iter().map(|h| h.key()).collect::<Vec<_>>(),
    }))
}

fn set_skill_harness(args: &Value) -> Result<Value, DispatchError> {
    let name = str_arg(args, "name", "set_skill_harness")?;
    let kind = harness_arg(args, "harness", "set_skill_harness")?;
    let enable = args
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("set_skill_harness requires a boolean 'enabled' argument"))?;
    let scope = scope_arg(args)?;
    let cwd = working_dir();

    let installed = skills::list_installed(&cwd).map_err(DispatchError::Tool)?;
    let skill = installed
        .iter()
        .find(|s| s.name == name && s.scope() == scope)
        .ok_or_else(|| {
            DispatchError::Tool(format!(
                "Skill '{name}' is not installed in that scope. Installed: {}",
                installed
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
    let cli_args = skills::set_enabled_args(skill, kind, enable).map_err(DispatchError::Tool)?;
    skills::run_skills(&cli_args, &cwd).map_err(DispatchError::Tool)?;
    Ok(json!({ "ok": true, "name": name, "harness": kind.key(), "enabled": enable }))
}

fn set_server_harness(args: &Value, paths: &StoragePaths) -> Result<Value, DispatchError> {
    let name = str_arg(args, "name", "set_server_harness")?;
    let kind = harness_arg(args, "harness", "set_server_harness")?;
    let state = str_arg(args, "state", "set_server_harness")?;
    if !matches!(state, "on" | "off" | "inherit") {
        return Err(invalid("'state' must be 'on', 'off' or 'inherit'"));
    }

    let (mut loaded, outcome) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    if !outcome.conflicts.is_empty() {
        return Err(DispatchError::Tool(format!(
            "Cannot change servers while conflicts are unresolved ({} conflict(s): {}). \
             Resolve them in the maga TUI, then try again.",
            outcome.conflicts.len(),
            conflict_brief(&outcome.conflicts),
        )));
    }
    let mut canonical = outcome.canonical;
    let cfg = canonical.get_mut(name).ok_or_else(|| {
        DispatchError::Tool(format!(
            "Unknown server '{name}'. Available servers: {}",
            available_names(&loaded.mcp.mcps)
        ))
    })?;
    match state {
        "inherit" => {
            cfg.harnesses.remove(&kind);
        }
        wanted => {
            let on = wanted == "on";
            // Same normalisation as the TUI: matching the global flag is plain inheriting.
            if on == cfg.enabled {
                cfg.harnesses.remove(&kind);
            } else {
                cfg.harnesses.insert(kind, on);
            }
        }
    }
    let effective = cfg.enabled_for_harness(kind);
    loaded
        .save_all(paths, &canonical)
        .map_err(DispatchError::Tool)?;

    Ok(json!({
        "ok": true,
        "name": name,
        "harness": kind.key(),
        "state": state,
        "active_for_harness": effective,
        "notes": super::next_session_notes(&loaded.config),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_validation_rejects_urls_paths_and_options() {
        for ok in ["owner/repo", "owner/repo@skill", "a-b/c_d.e@f-g"] {
            assert!(validate_package(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "owner",
            "-rf/x",
            "owner/-x",
            "owner/repo@",
            "owner/repo@-x",
            "https://github.com/o/r",
            "./local/path",
            "../x/y",
            "o/r extra",
            "o/r;rm",
            "o/r/s",
        ] {
            assert!(validate_package(bad).is_err(), "{bad}");
        }
    }

    // ── Protocol-level tests: real frames through `handle_frame`, CLI faked per thread ──

    use crate::domain::skills::test_support;
    use std::cell::RefCell;
    use std::rc::Rc;

    const LIVE_SECRET: &str = "sk-live-SECRETVALUE99";

    fn setup(mcp_toml: &str) -> (tempfile::TempDir, StoragePaths) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        std::fs::write(&paths.mcp_toml, mcp_toml).expect("seed mcp.toml");
        (dir, paths)
    }

    /// Returns `(is_error, text)` of a tools/call frame.
    fn call(paths: &StoragePaths, mutations: bool, name: &str, args: Value) -> (bool, String) {
        let frame = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args },
        });
        let response = super::super::handle_frame(&frame.to_string(), mutations, paths)
            .expect("a response frame");
        if let Some(error) = response.get("error") {
            return (
                true,
                error["message"].as_str().unwrap_or_default().to_string(),
            );
        }
        let result = &response["result"];
        (
            result["isError"].as_bool().unwrap_or(false),
            result["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    }

    fn parsed(paths: &StoragePaths, mutations: bool, name: &str, args: Value) -> Value {
        let (is_error, text) = call(paths, mutations, name, args);
        assert!(!is_error, "{name} failed: {text}");
        serde_json::from_str(&text).expect("JSON tool output")
    }

    const REGISTRY: &str = "[mcps.api]\n\
        enabled = true\n\
        transport = \"http\"\n\
        url = \"https://api.example.com/mcp?apiKey=sk-live-SECRETVALUE99\"\n\
        [mcps.api.headers]\n\
        Authorization = \"Bearer sk-live-SECRETVALUE99\"\n\
        [mcps.docs]\n\
        enabled = true\n\
        transport = \"http\"\n\
        url = \"https://docs.example.com/mcp\"\n\
        [mcps.docs.headers]\n\
        X-Api-Key = \"${DOCS_API_KEY}\"\n";

    #[test]
    fn new_tools_are_listed_and_mutating_ones_are_gated() {
        let (_dir, paths) = setup(REGISTRY);
        let listed = |mutations| -> Vec<String> {
            let frame = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
            let r = super::super::handle_frame(&frame.to_string(), mutations, &paths).unwrap();
            r["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        let read_only = listed(false);
        for name in [
            "list_harnesses",
            "list_plugins",
            "list_secrets",
            "list_skills",
            "search_skills",
        ] {
            assert!(read_only.contains(&name.to_string()), "{name}");
        }
        for name in MUTATING {
            assert!(
                !read_only.contains(&name.to_string()),
                "{name} must be hidden"
            );
            let (is_error, text) = call(&paths, false, name, json!({}));
            assert!(is_error && text.contains("read-only"), "{name}: {text}");
        }
        assert_eq!(listed(true).len(), read_only.len() + MUTATING.len());
    }

    #[test]
    fn no_tool_output_ever_contains_a_secret_value() {
        let (dir, paths) = setup(REGISTRY);
        std::fs::write(
            dir.path().join(".env.local"),
            "DOCS_API_KEY=\"docs-key-VALUE-12345\"\n",
        )
        .unwrap();

        for name in [
            "list_servers",
            "list_harnesses",
            "list_secrets",
            "list_plugins",
            "check_drift",
        ] {
            let (_, text) = call(&paths, true, name, json!({}));
            assert!(
                !text.contains(LIVE_SECRET),
                "{name} leaked the literal secret: {text}"
            );
            assert!(
                !text.contains("docs-key-VALUE-12345"),
                "{name} leaked a .env.local value"
            );
        }

        // Literal secrets are masked in endpoints and listed by name only.
        let servers = parsed(&paths, false, "list_servers", json!({}));
        let api = servers["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "api")
            .unwrap();
        assert!(api["endpoint"].as_str().unwrap().contains("apiKey=***"));

        // The last line of defense masks values even inside free-form error text.
        let (is_error, text) = call(
            &paths,
            true,
            "install_skill",
            json!({ "package": "owner/docs-key-VALUE-12345;x" }),
        );
        assert!(is_error);
        assert!(
            text.contains("***") && !text.contains("docs-key-VALUE-12345"),
            "{text}"
        );
    }

    #[test]
    fn list_secrets_reports_names_sources_and_literal_servers() {
        let (dir, paths) = setup(REGISTRY);
        let missing = parsed(&paths, false, "list_secrets", json!({}));
        let var = &missing["variables"][0];
        assert_eq!(var["name"], "DOCS_API_KEY");
        assert_eq!(var["source"], "missing");
        assert_eq!(var["defined"], false);
        assert_eq!(var["used_by"], json!(["docs"]));
        assert_eq!(missing["servers_with_literal_secrets"], json!(["api"]));

        std::fs::write(
            dir.path().join(".env.local"),
            "DOCS_API_KEY=\"abcdef123456\"\n",
        )
        .unwrap();
        let defined = parsed(&paths, false, "list_secrets", json!({}));
        assert_eq!(defined["variables"][0]["source"], ".env.local");
        assert_eq!(defined["dotenv_variables"], json!(["DOCS_API_KEY"]));
    }

    #[test]
    fn list_harnesses_reports_state_and_secret_delivery() {
        let (dir, paths) = setup(REGISTRY);
        std::fs::write(
            dir.path().join(".env.local"),
            "DOCS_API_KEY=\"abcdef123456\"\n",
        )
        .unwrap();
        let out = parsed(&paths, false, "list_harnesses", json!({}));
        let harnesses = out["harnesses"].as_array().unwrap();
        assert_eq!(harnesses.len(), 5);
        let by = |key: &str| harnesses.iter().find(|h| h["harness"] == key).unwrap();

        assert_eq!(by("claude")["secrets"]["delivery"], "settings_env");
        assert_eq!(by("opencode")["secrets"]["delivery"], "file_reference");
        let grok = by("grok");
        assert_eq!(grok["secrets"]["delivery"], "literal_in_local_config");
        assert_eq!(
            grok["secrets"]["servers_with_literal_secrets"],
            json!(["docs"])
        );
        assert_eq!(grok["synced"], true);
        assert_eq!(grok["active_servers"], json!(["api", "docs"]));
        assert!(
            grok["config_file"]
                .as_str()
                .unwrap()
                .contains("grok_config.toml")
        );
        assert_eq!(
            by("claude")["secrets"]["servers_with_literal_secrets"],
            json!([])
        );
    }

    #[test]
    fn set_server_harness_forces_inherits_and_normalizes() {
        let (dir, paths) = setup(REGISTRY);
        std::fs::write(
            dir.path().join(".env.local"),
            "DOCS_API_KEY=\"docs-key-123456\"
",
        )
        .unwrap();
        let active = |paths: &StoragePaths, harness: &str| -> bool {
            let out = parsed(paths, false, "list_servers", json!({}));
            let docs = out["servers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["name"] == "docs")
                .unwrap();
            docs["harnesses"][harness].as_bool().unwrap()
        };
        assert!(active(&paths, "claude") && active(&paths, "grok"));

        let off = parsed(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "docs", "harness": "Claude Code", "state": "off" }),
        );
        assert_eq!(off["active_for_harness"], false);
        assert!(
            !active(&paths, "claude") && active(&paths, "grok"),
            "only that harness"
        );
        assert!(
            std::fs::read_to_string(&paths.mcp_toml)
                .unwrap()
                .contains("claude = false")
        );

        // "on" equals the global flag, so it is stored as plain inheriting.
        parsed(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "docs", "harness": "claude", "state": "on" }),
        );
        assert!(active(&paths, "claude"));
        assert!(
            !std::fs::read_to_string(&paths.mcp_toml)
                .unwrap()
                .contains("claude = ")
        );

        parsed(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "docs", "harness": "grok", "state": "off" }),
        );
        parsed(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "docs", "harness": "grok", "state": "inherit" }),
        );
        assert!(active(&paths, "grok"));

        // Bad input is rejected without writing.
        let (e1, m1) = call(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "nope", "harness": "grok", "state": "off" }),
        );
        assert!(e1 && m1.contains("Unknown server"), "{m1}");
        let (e2, m2) = call(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "docs", "harness": "vim", "state": "off" }),
        );
        assert!(e2 && m2.contains("Unknown harness"), "{m2}");
        let (e3, m3) = call(
            &paths,
            true,
            "set_server_harness",
            json!({ "name": "docs", "harness": "grok", "state": "maybe" }),
        );
        assert!(e3 && m3.contains("'state'"), "{m3}");
    }

    #[test]
    fn skills_tools_wrap_the_cli_and_validate_input() {
        let (_dir, paths) = setup(REGISTRY);
        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let log = Rc::clone(&calls);
        test_support::install(move |args| {
            log.borrow_mut().push(args.to_vec());
            match args.first().map(String::as_str) {
                Some("ls") if args.iter().any(|a| a == "-g") => Ok(r#"[
                    {"name":"tui-design","path":"/p","scope":"global","agents":["Claude Code"],"source":"o/r"},
                    {"name":"hand-made","path":null,"scope":"global","agents":[],"source":null}]"#.to_string()),
                Some("ls") => Ok("[]".to_string()),
                Some("find") => Ok("o/r@tui-design 1.4K installs\n└ https://skills.sh/o/r/tui-design\n".to_string()),
                _ => Ok(String::new()),
            }
        });

        let listed = parsed(&paths, false, "list_skills", json!({}));
        assert_eq!(listed["total"], 2);
        let tui = listed["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "tui-design")
            .unwrap();
        assert_eq!(tui["harnesses"], json!(["claude"]));

        let found = parsed(&paths, false, "search_skills", json!({ "query": "tui" }));
        assert_eq!(found["results"][0]["package"], "o/r@tui-design");
        let (bad, _) = call(&paths, false, "search_skills", json!({ "query": "--help" }));
        assert!(bad, "option-looking queries are rejected");

        // Install passes the validated package and the synced harnesses only.
        calls.borrow_mut().clear();
        let ok = parsed(
            &paths,
            true,
            "install_skill",
            json!({ "package": "o/r@tui-design", "scope": "project" }),
        );
        assert_eq!(ok["scope"], "project");
        let installed = calls.borrow()[0].clone();
        assert_eq!(&installed[..3], ["add", "o/r@tui-design", "-y"]);
        assert!(!installed.contains(&"-g".to_string()));
        assert_eq!(installed.iter().filter(|a| *a == "-a").count(), 5);

        calls.borrow_mut().clear();
        for bad in ["https://github.com/o/r", "../local", "-g", "o/r;rm -rf"] {
            let (is_error, _) = call(&paths, true, "install_skill", json!({ "package": bad }));
            assert!(is_error, "{bad}");
        }
        assert!(
            calls.borrow().is_empty(),
            "the CLI is never reached with invalid packages"
        );

        // Per-harness activation: unlink works, relinking needs a recorded source.
        calls.borrow_mut().clear();
        parsed(
            &paths,
            true,
            "set_skill_harness",
            json!({ "name": "tui-design", "harness": "claude", "enabled": false }),
        );
        assert_eq!(
            calls.borrow().last().unwrap(),
            &[
                "remove",
                "-s",
                "tui-design",
                "-y",
                "-g",
                "-a",
                "claude-code"
            ]
        );
        parsed(
            &paths,
            true,
            "set_skill_harness",
            json!({ "name": "tui-design", "harness": "grok", "enabled": true }),
        );
        assert_eq!(
            calls.borrow().last().unwrap(),
            &["add", "o/r", "-s", "tui-design", "-y", "-g", "-a", "grok"]
        );
        let (e, m) = call(
            &paths,
            true,
            "set_skill_harness",
            json!({ "name": "hand-made", "harness": "grok", "enabled": true }),
        );
        assert!(e && m.contains("no recorded source"), "{m}");
        let (e, m) = call(
            &paths,
            true,
            "set_skill_harness",
            json!({ "name": "ghost", "harness": "grok", "enabled": true }),
        );
        assert!(e && m.contains("not installed"), "{m}");
    }
}
