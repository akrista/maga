//! MCP server mode: newline-delimited JSON-RPC 2.0 over stdio (hand-rolled, no async runtime).
//!
//! Invariants (design D1/D3/D4/D6): stdout carries protocol frames only — every diagnostic
//! goes to stderr; no server-side state — every tool call reconciles from disk; the
//! mutation gate is read exactly once at startup.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use crate::domain::model::{HarnessKind, MagaConfig, McpServerConfig};
use crate::domain::probe::{DEFAULT_PROBE_TIMEOUT, ServerProbeState, probe_server};
use crate::domain::reconcile::{
    ConflictKind, ConvergenceStatus, McpConflict, ReconcileOutcome, reconcile_harnesses_with_env,
};
use crate::domain::storage::{HeadlessSyncError, LoadedConfigs, StoragePaths};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// D1: diagnostics go to stderr, best-effort — a client may close or exhaust stderr at
/// any time, and a broken stderr must never terminate the server (mcp-serve-resilience).
fn log_diagnostic(message: String) {
    let _ = writeln!(io::stderr(), "{message}");
}

/// Run the MCP server loop until stdin closes. Never touches stdout outside protocol frames.
pub fn run(paths: StoragePaths, mutations_enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    log_diagnostic(format!(
        "maga MCP server v{} listening on stdio (mutations: {})",
        env!("CARGO_PKG_VERSION"),
        if mutations_enabled {
            "enabled"
        } else {
            "read-only"
        }
    ));
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_frame(&line, mutations_enabled, &paths) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_frame(line: &str, mutations: bool, paths: &StoragePaths) -> Option<Value> {
    let frame: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("Parse error: {e}"),
            ));
        }
    };
    if frame.is_array() {
        return Some(error_response(
            Value::Null,
            -32600,
            "Batch requests are not supported",
        ));
    }
    let object = match frame.as_object() {
        Some(object) => object,
        None => {
            return Some(error_response(
                Value::Null,
                -32600,
                "Invalid request: expected a JSON object",
            ));
        }
    };
    // A frame without `method` is a client response; maga never sends server requests.
    let method = object.get("method").and_then(Value::as_str)?.to_string();
    let id = match object.get("id") {
        // Notifications are logged to stderr and never answered.
        None => {
            log_diagnostic(format!("maga: notification {method}"));
            return None;
        }
        Some(id) => id.clone(),
    };
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    Some(match dispatch(&method, &params, mutations, paths) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(DispatchError::Rpc(code, message)) => error_response(id, code, &message),
        Err(DispatchError::Tool(message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "result": tool_text(message, true) })
        }
    })
}

enum DispatchError {
    /// JSON-RPC protocol error: malformed request, unknown method or invalid params.
    Rpc(i64, String),
    /// Tool-level failure surfaced as `isError: true` so the agent can read and react.
    Tool(String),
}

fn dispatch(
    method: &str,
    params: &Value,
    mutations: bool,
    paths: &StoragePaths,
) -> Result<Value, DispatchError> {
    match method {
        "initialize" => Ok(initialize_result(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions(mutations) })),
        "tools/call" => tools_call(params, mutations, paths),
        other => Err(DispatchError::Rpc(
            -32601,
            format!("Method not found: {other}"),
        )),
    }
}

fn initialize_result(params: &Value) -> Value {
    json!({
        "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(MCP_PROTOCOL_VERSION),
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "maga", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn tool_definitions(mutations: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "list_servers",
            "description": "List every MCP server in maga's canonical registry with enabled state, transport, endpoint and conflict status.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
        json!({
            "name": "probe_server",
            "description": "Health-check MCP servers by performing a real MCP handshake (stdio or http). Probes one server by name, or all of them.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "Optional server name to probe a single server." } },
            },
        }),
        json!({
            "name": "check_drift",
            "description": "Check whether every enabled harness is converged with the canonical registry, without writing any files.",
            "inputSchema": { "type": "object", "properties": {} },
        }),
    ];
    if mutations {
        tools.push(json!({
            "name": "toggle_server",
            "description": "Enable or disable an MCP server in the canonical registry and synchronize the change to every enabled harness.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Server name in the canonical registry." },
                    "enabled": { "type": "boolean", "description": "Desired enabled state." },
                },
                "required": ["name", "enabled"],
            },
        }));
        tools.push(json!({
            "name": "sync",
            "description": "Reconcile and synchronize the canonical registry to all enabled harnesses. Fails fast on conflicts unless force is true.",
            "inputSchema": {
                "type": "object",
                "properties": { "force": { "type": "boolean", "description": "Push canonical mcp.toml authoritatively, overriding conflicts." } },
            },
        }));
    }
    tools
}

fn tools_call(
    params: &Value,
    mutations: bool,
    paths: &StoragePaths,
) -> Result<Value, DispatchError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| DispatchError::Rpc(-32602, "tools/call requires a string 'name'".into()))?;
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Value::Object(Default::default()),
        Some(args) if args.is_object() => args.clone(),
        Some(_) => {
            return Err(DispatchError::Rpc(
                -32602,
                "tools/call 'arguments' must be an object".into(),
            ));
        }
    };
    let outcome = match name {
        "list_servers" => list_servers(paths),
        "probe_server" => probe_server_tool(&args, paths),
        "check_drift" => check_drift(paths),
        "toggle_server" | "sync" if !mutations => Err(DispatchError::Tool(read_only_error(name))),
        "toggle_server" => toggle_server(&args, paths),
        "sync" => sync(&args, paths),
        other => Err(DispatchError::Rpc(-32602, format!("Unknown tool: {other}"))),
    };
    match outcome {
        Ok(value) => Ok(tool_text(
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into()),
            false,
        )),
        Err(DispatchError::Tool(message)) => Ok(tool_text(message, true)),
        Err(rpc) => Err(rpc),
    }
}

fn tool_text(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn read_only_error(tool: &str) -> String {
    format!(
        "maga is running in read-only mode: the '{tool}' tool requires the server process to be \
         started with MAGA_MCP_MUTATIONS=1 (set it in the `env` block of this MCP registration). \
         No files were modified."
    )
}

/// D3: reconcile from disk on every call — concurrent TUI/CLI edits are always picked up.
fn reconcile_from_disk(paths: &StoragePaths) -> Result<(LoadedConfigs, ReconcileOutcome), String> {
    let loaded = LoadedConfigs::load(paths)?;
    let outcome = reconcile_harnesses_with_env(
        &loaded.mcp.mcps,
        &loaded.enabled_harness_servers(),
        Some(&paths.load_env_context()),
    );
    Ok((loaded, outcome))
}

fn available_names(canonical: &BTreeMap<String, McpServerConfig>) -> String {
    if canonical.is_empty() {
        return "(none)".into();
    }
    canonical.keys().cloned().collect::<Vec<_>>().join(", ")
}

fn conflict_brief(conflicts: &[McpConflict]) -> String {
    conflicts
        .iter()
        .map(|c| format!("{} ({})", c.name, c.differing_fields().join("/")))
        .collect::<Vec<_>>()
        .join("; ")
}

fn list_servers(paths: &StoragePaths) -> Result<Value, DispatchError> {
    let (_, outcome) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    let servers: Vec<Value> = outcome
        .canonical
        .iter()
        .map(|(name, cfg)| {
            json!({
                "name": name,
                "enabled": cfg.enabled,
                "transport": cfg.transport.as_str(),
                "endpoint": cfg.endpoint_summary(),
                "conflicted": outcome.conflicts.iter().any(|c| c.matches_name(name)),
                "description": cfg.description,
            })
        })
        .collect();
    Ok(json!({
        "servers": servers,
        "conflicts": summarize_conflicts(&outcome.conflicts),
        "total": servers.len(),
        "conflict_count": outcome.conflicts.len(),
    }))
}

fn check_drift(paths: &StoragePaths) -> Result<Value, DispatchError> {
    let loaded = LoadedConfigs::load(paths).map_err(DispatchError::Tool)?;
    match loaded.check_headless(paths) {
        ConvergenceStatus::Converged => Ok(json!({ "status": "converged" })),
        ConvergenceStatus::Drift {
            drift_harnesses,
            imported_count,
        } => Ok(json!({
            "status": "drift",
            "drift_harnesses": drift_harnesses.iter().map(|h| h.display_name()).collect::<Vec<_>>(),
            "imported_count": imported_count,
        })),
        ConvergenceStatus::Conflicted { conflicts } => Ok(json!({
            "status": "conflicted",
            "conflicts": summarize_conflicts(&conflicts),
        })),
    }
}

fn probe_server_tool(args: &Value, paths: &StoragePaths) -> Result<Value, DispatchError> {
    let (_, outcome) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    let requested = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let targets: Vec<(String, McpServerConfig)> = match requested {
        Some(name) => {
            let cfg = outcome.canonical.get(name).ok_or_else(|| {
                DispatchError::Tool(format!(
                    "Unknown server '{name}'. Available servers: {}",
                    available_names(&outcome.canonical)
                ))
            })?;
            vec![(name.to_string(), cfg.clone())]
        }
        None => outcome
            .canonical
            .iter()
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect(),
    };
    // Blocking probes are fine here: the server handles one request at a time anyway.
    let results = targets
        .into_iter()
        .map(|(name, cfg)| probe_result(&name, probe_server(&cfg, DEFAULT_PROBE_TIMEOUT)))
        .collect::<Vec<_>>();
    Ok(json!({ "results": results }))
}

fn probe_result(name: &str, state: ServerProbeState) -> Value {
    match state {
        ServerProbeState::Online(outcome) => json!({
            "name": name,
            "status": "online",
            "latency_ms": outcome.latency_ms,
            "server_info": outcome.server_info,
            "protocol_version": outcome.protocol_version,
            "tools": outcome
                .tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description }))
                .collect::<Vec<_>>(),
        }),
        ServerProbeState::Failed { error, latency_ms } => json!({
            "name": name,
            "status": "failed",
            "latency_ms": latency_ms,
            "error": error,
        }),
        ServerProbeState::Probing => json!({
            "name": name,
            "status": "failed",
            "error": "probe did not complete",
        }),
    }
}

fn toggle_server(args: &Value, paths: &StoragePaths) -> Result<Value, DispatchError> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            DispatchError::Rpc(-32602, "toggle_server requires a 'name' argument".into())
        })?;
    let enabled = args
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            DispatchError::Rpc(
                -32602,
                "toggle_server requires a boolean 'enabled' argument".into(),
            )
        })?;

    let (mut loaded, outcome) = reconcile_from_disk(paths).map_err(DispatchError::Tool)?;
    if !outcome.conflicts.is_empty() {
        return Err(DispatchError::Tool(format!(
            "Cannot toggle servers while conflicts are unresolved ({} conflict(s): {}). \
             Resolve them in the maga TUI, then try again.",
            outcome.conflicts.len(),
            conflict_brief(&outcome.conflicts),
        )));
    }
    let mut cfg = outcome.canonical.get(name).cloned().ok_or_else(|| {
        DispatchError::Tool(format!(
            "Unknown server '{name}'. Available servers: {}",
            available_names(&outcome.canonical)
        ))
    })?;
    cfg.enabled = enabled;
    let mut canonical = outcome.canonical;
    canonical.insert(name.to_string(), cfg);
    loaded
        .save_all(paths, &canonical)
        .map_err(DispatchError::Tool)?;

    Ok(json!({
        "ok": true,
        "name": name,
        "enabled": enabled,
        "synced_harnesses": enabled_harness_names(&loaded.config),
        "notes": next_session_notes(&loaded.config),
    }))
}

fn sync(args: &Value, paths: &StoragePaths) -> Result<Value, DispatchError> {
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
    let mut loaded = LoadedConfigs::load(paths).map_err(DispatchError::Tool)?;
    match loaded.sync_headless(paths, force) {
        Ok(true) => Ok(json!({
            "ok": true,
            "synced": true,
            "forced": force,
            "message": "Canonical registry synchronized to all enabled harnesses.",
            "notes": next_session_notes(&loaded.config),
        })),
        Ok(false) => Ok(json!({
            "ok": true,
            "synced": false,
            "forced": force,
            "message": "All enabled harnesses are already converged (nothing to sync).",
            "notes": next_session_notes(&loaded.config),
        })),
        Err(HeadlessSyncError::Conflicted(conflicts)) => Err(DispatchError::Tool(format!(
            "Sync aborted: {} conflict(s) detected across harnesses ({}). Re-run with \
             force=true to push canonical mcp.toml authoritatively, or resolve in the maga TUI. \
             No files were modified.",
            conflicts.len(),
            conflict_brief(&conflicts),
        ))),
        Err(HeadlessSyncError::Io(e)) => Err(DispatchError::Tool(format!("Sync failed: {e}"))),
    }
}

fn summarize_conflicts(conflicts: &[McpConflict]) -> Vec<Value> {
    conflicts
        .iter()
        .map(|c| {
            let kind = match c.kind {
                ConflictKind::DefinitionDrift => "definition_drift",
                ConflictKind::CaseCollision { .. } => "case_collision",
                ConflictKind::UnresolvedEnv { .. } => "unresolved_env",
            };
            json!({
                "name": c.name,
                "kind": kind,
                "differing_fields": c.differing_fields(),
                "variants": c
                    .variants
                    .iter()
                    .map(|v| json!({ "name": v.name, "sources": v.sources_label() }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn enabled_harness_names(config: &MagaConfig) -> Vec<&'static str> {
    HarnessKind::ALL
        .iter()
        .filter(|h| config.tools.is_enabled(**h))
        .map(|h| h.display_name())
        .collect()
}

/// D5: note whenever an affected harness only reads MCP config at startup.
fn next_session_notes(config: &MagaConfig) -> Vec<String> {
    HarnessKind::ALL
        .iter()
        .filter(|h| config.tools.is_enabled(**h) && h.reads_mcp_at_startup())
        .map(|h| {
            format!(
                "{} reads MCP config only at startup: changes apply on its next session.",
                h.display_name()
            )
        })
        .collect()
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
