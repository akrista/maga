use crate::domain::model::{McpServerConfig, Transport, normalize_mcp_server_name};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Represents a parsed Claude Code `~/.claude.json` document while preserving all unrelated top-level keys
/// and `plugin:*` entries in `disabledMcpServers`.
#[derive(Debug, Clone, Default)]
pub struct ClaudeDocument {
    pub raw_root: Map<String, Value>,
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl ClaudeDocument {
    pub fn parse_json_str(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }
        let root: Map<String, Value> =
            serde_json::from_str(input).map_err(|e| format!("Invalid Claude Code JSON: {e}"))?;

        let disabled_set: BTreeSet<String> = root
            .get("disabledMcpServers")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .flat_map(|s| [s.to_string(), normalize_mcp_server_name(s)])
                    .collect()
            })
            .unwrap_or_default();

        let mut servers = BTreeMap::new();
        if let Some(Value::Object(mcp_servers)) = root.get("mcpServers") {
            let mut spaced = Vec::new();
            for (name, val) in mcp_servers {
                if let Value::Object(obj) = val
                    && let Some(mut cfg) = parse_claude_server(obj)
                {
                    let norm = normalize_mcp_server_name(name);
                    if norm.is_empty() {
                        continue;
                    }
                    if disabled_set.contains(name) || disabled_set.contains(&norm) {
                        cfg.enabled = false;
                    }
                    if &norm == name {
                        servers.insert(norm, cfg);
                    } else {
                        spaced.push((norm, cfg));
                    }
                }
            }
            for (norm, cfg) in spaced {
                servers.entry(norm).or_insert(cfg);
            }
        }

        Ok(Self {
            raw_root: root,
            servers,
        })
    }

    pub fn has_raw_spaced_keys(&self) -> bool {
        let root_mcp_spaced = self
            .raw_root
            .get("mcpServers")
            .and_then(Value::as_object)
            .is_some_and(|m| m.keys().any(|k| normalize_mcp_server_name(k) != *k));
        let array_has_spaced = |obj: &Map<String, Value>| {
            for key in ["disabledMcpServers", "enabledMcpServers"] {
                if let Some(Value::Array(arr)) = obj.get(key)
                    && arr
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|s| !s.starts_with("plugin:") && normalize_mcp_server_name(s) != s)
                {
                    return true;
                }
            }
            false
        };
        if root_mcp_spaced || array_has_spaced(&self.raw_root) {
            return true;
        }
        if let Some(Value::Object(projects)) = self.raw_root.get("projects") {
            for env_val in projects.values() {
                if let Value::Object(env_obj) = env_val
                    && array_has_spaced(env_obj)
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn to_json_string(
        &self,
        canonical: &BTreeMap<String, McpServerConfig>,
    ) -> Result<String, String> {
        let mut root = self.raw_root.clone();
        let raw_mcp_names: BTreeSet<String> = root
            .get("mcpServers")
            .and_then(Value::as_object)
            .map(|m| {
                m.keys()
                    .flat_map(|k| [k.clone(), normalize_mcp_server_name(k)])
                    .collect()
            })
            .unwrap_or_default();

        let mut mcp_servers = Map::new();
        for (name, server) in canonical {
            mcp_servers.insert(name.clone(), serialize_claude_server(server));
        }
        root.insert("mcpServers".to_string(), Value::Object(mcp_servers));

        sync_mcp_toggle_arrays(&mut root, canonical, &self.servers, &raw_mcp_names);

        if let Some(Value::Object(projects)) = root.get_mut("projects") {
            for env_val in projects.values_mut() {
                if let Value::Object(env_obj) = env_val {
                    sync_mcp_toggle_arrays(env_obj, canonical, &self.servers, &raw_mcp_names);
                }
            }
        }

        serde_json::to_string_pretty(&root)
            .map_err(|e| format!("Failed to serialize Claude Code JSON: {e}"))
    }
}

fn sync_mcp_toggle_arrays(
    target: &mut Map<String, Value>,
    canonical: &BTreeMap<String, McpServerConfig>,
    previous_servers: &BTreeMap<String, McpServerConfig>,
    raw_mcp_names: &BTreeSet<String>,
) {
    let is_removed_or_spaced_entry = |entry: &str| {
        if normalize_mcp_server_name(entry) != entry {
            return true;
        }
        let norm = normalize_mcp_server_name(entry);
        !canonical.contains_key(entry)
            && (previous_servers.contains_key(entry)
                || previous_servers.contains_key(&norm)
                || raw_mcp_names.contains(entry)
                || raw_mcp_names.contains(&norm))
    };

    // Preserve any non-canonical / plugin:* entries in disabledMcpServers
    let existing_disabled: Vec<String> = target
        .get("disabledMcpServers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut new_disabled: Vec<String> = existing_disabled
        .into_iter()
        .filter(|entry| {
            entry.starts_with("plugin:")
                || (!is_removed_or_spaced_entry(entry)
                    && canonical.get(entry).is_none_or(|cfg| !cfg.enabled))
        })
        .collect();

    for (name, server) in canonical {
        if !server.enabled && !new_disabled.contains(name) {
            new_disabled.push(name.clone());
        }
    }

    target.insert("disabledMcpServers".to_string(), json!(new_disabled));

    // Update enabledMcpServers so disabled/deleted servers are removed and enabled ones are present
    let existing_enabled: Vec<String> = target
        .get("enabledMcpServers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut new_enabled: Vec<String> = existing_enabled
        .into_iter()
        .filter(|entry| {
            entry.starts_with("plugin:")
                || (!is_removed_or_spaced_entry(entry)
                    && canonical.get(entry).is_none_or(|cfg| cfg.enabled))
        })
        .collect();

    for (name, server) in canonical {
        if server.enabled && !new_enabled.contains(name) {
            new_enabled.push(name.clone());
        }
    }

    target.insert("enabledMcpServers".to_string(), json!(new_enabled));
}

fn parse_claude_server(obj: &Map<String, Value>) -> Option<McpServerConfig> {
    let inline_disabled = obj
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let enabled = !inline_disabled;

    let url = obj
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| obj.get("serverUrl").and_then(Value::as_str))
        .map(ToString::to_string);

    let command = obj
        .get("command")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let transport = match obj.get("type").and_then(Value::as_str) {
        Some("http" | "sse") => Transport::Http,
        Some("stdio") => Transport::Stdio,
        _ => {
            if url.is_some() {
                Transport::Http
            } else if command.is_some() {
                Transport::Stdio
            } else {
                return None;
            }
        }
    };

    let args = obj
        .get("args")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let env = extract_string_map(obj.get("env"));
    let headers = extract_string_map(obj.get("headers"));

    Some(McpServerConfig {
        enabled,
        description: None,
        transport,
        command,
        args,
        env,
        url,
        headers,
    })
}

fn serialize_claude_server(server: &McpServerConfig) -> Value {
    let mut obj = Map::new();
    match server.transport {
        Transport::Stdio => {
            obj.insert("type".to_string(), json!("stdio"));
            if let Some(cmd) = &server.command {
                obj.insert("command".to_string(), json!(cmd));
            }
            if !server.args.is_empty() {
                obj.insert("args".to_string(), json!(server.args));
            }
            if !server.env.is_empty() {
                obj.insert("env".to_string(), json!(server.env));
            }
        }
        Transport::Http => {
            obj.insert("type".to_string(), json!("http"));
            if let Some(url) = &server.url {
                obj.insert("url".to_string(), json!(url));
            }
            if !server.headers.is_empty() {
                obj.insert("headers".to_string(), json!(server.headers));
            }
        }
    }
    Value::Object(obj)
}

fn extract_string_map(val: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(map)) = val {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unrelated_claude_keys_and_plugin_entries() {
        let fixture = r#"{
          "numStartups": 42,
          "oauthAccount": { "email": "dev@example.com" },
          "disabledMcpServers": [
            "Astro Docs",
            "plugin:laravel-boost:laravel-boost",
            "plugin:Notion:notion"
          ],
          "enabledMcpServers": [
            "context7"
          ],
          "mcpServers": {
            "Astro Docs": {
              "type": "http",
              "url": "https://mcp.docs.astro.build/mcp"
            },
            "context7": {
              "type": "http",
              "url": "https://mcp.context7.com/mcp"
            }
          }
        }"#;

        let mut doc = ClaudeDocument::parse_json_str(fixture).expect("parse claude json");
        assert!(doc.has_raw_spaced_keys());
        assert!(!doc.servers.contains_key("Astro Docs"));
        assert!(!doc.servers["Astro-Docs"].enabled);
        assert!(doc.servers["context7"].enabled);

        // Flip states: enable Astro-Docs, disable context7
        doc.servers.get_mut("Astro-Docs").unwrap().enabled = true;
        doc.servers.get_mut("context7").unwrap().enabled = false;

        let out = doc
            .to_json_string(&doc.servers)
            .expect("serialize claude json");
        let raw: Map<String, Value> = serde_json::from_str(&out).expect("parse raw json");

        assert_eq!(raw.get("numStartups"), Some(&json!(42)));
        assert_eq!(
            raw.get("oauthAccount"),
            Some(&json!({ "email": "dev@example.com" }))
        );

        let disabled = raw["disabledMcpServers"]
            .as_array()
            .expect("disabled array");
        assert!(disabled.contains(&json!("plugin:laravel-boost:laravel-boost")));
        assert!(disabled.contains(&json!("plugin:Notion:notion")));
        assert!(disabled.contains(&json!("context7")));
        assert!(!disabled.contains(&json!("Astro Docs")));
        assert!(!disabled.contains(&json!("Astro-Docs")));

        let enabled = raw["enabledMcpServers"].as_array().expect("enabled array");
        assert!(enabled.contains(&json!("Astro-Docs")));
        assert!(!enabled.contains(&json!("Astro Docs")));
        assert!(!enabled.contains(&json!("context7")));
    }

    #[test]
    fn synchronizes_toggle_arrays_across_all_project_environments_and_purges_deleted() {
        let fixture = r#"{
          "disabledMcpServers": [
            "Astro Docs",
            "obsolete-mcp"
          ],
          "enabledMcpServers": [
            "context7"
          ],
          "mcpServers": {
            "Astro Docs": {
              "type": "http",
              "url": "https://mcp.docs.astro.build/mcp"
            },
            "context7": {
              "type": "http",
              "url": "https://mcp.context7.com/mcp"
            },
            "obsolete-mcp": {
              "type": "stdio",
              "command": "/usr/bin/obsolete"
            }
          },
          "projects": {
            "/home/dev/project-a": {
              "allowedTools": ["Bash"],
              "disabledMcpServers": [
                "context7",
                "plugin:Notion:notion",
                "obsolete-mcp"
              ],
              "enabledMcpServers": [
                "Astro Docs",
                "obsolete-mcp"
              ]
            },
            "/home/dev/project-b": {
              "hasTrustDialogAccepted": true
            }
          }
        }"#;

        let mut doc = ClaudeDocument::parse_json_str(fixture).expect("parse claude json");

        // Enable Astro-Docs, disable context7, delete obsolete-mcp
        doc.servers.get_mut("Astro-Docs").unwrap().enabled = true;
        doc.servers.get_mut("context7").unwrap().enabled = false;
        doc.servers.remove("obsolete-mcp");

        let out = doc
            .to_json_string(&doc.servers)
            .expect("serialize claude json");
        let raw: Map<String, Value> = serde_json::from_str(&out).expect("parse raw json");

        // Root arrays
        let root_disabled = raw["disabledMcpServers"].as_array().unwrap();
        let root_enabled = raw["enabledMcpServers"].as_array().unwrap();
        assert!(root_disabled.contains(&json!("context7")));
        assert!(!root_disabled.contains(&json!("Astro Docs")));
        assert!(!root_disabled.contains(&json!("Astro-Docs")));
        assert!(!root_disabled.contains(&json!("obsolete-mcp")));
        assert!(root_enabled.contains(&json!("Astro-Docs")));
        assert!(!root_enabled.contains(&json!("Astro Docs")));
        assert!(!root_enabled.contains(&json!("context7")));
        assert!(!root_enabled.contains(&json!("obsolete-mcp")));

        // Environment A: unrelated keys & plugin:* preserved, canonical synced, deleted/spaced purged
        let proj_a = raw["projects"]["/home/dev/project-a"].as_object().unwrap();
        assert_eq!(proj_a.get("allowedTools"), Some(&json!(["Bash"])));
        let a_disabled = proj_a["disabledMcpServers"].as_array().unwrap();
        let a_enabled = proj_a["enabledMcpServers"].as_array().unwrap();
        assert!(a_disabled.contains(&json!("plugin:Notion:notion")));
        assert!(a_disabled.contains(&json!("context7")));
        assert!(!a_disabled.contains(&json!("Astro Docs")));
        assert!(!a_disabled.contains(&json!("Astro-Docs")));
        assert!(!a_disabled.contains(&json!("obsolete-mcp")));
        assert!(a_enabled.contains(&json!("Astro-Docs")));
        assert!(!a_enabled.contains(&json!("Astro Docs")));
        assert!(!a_enabled.contains(&json!("context7")));
        assert!(!a_enabled.contains(&json!("obsolete-mcp")));

        // Environment B: arrays initialized with canonical state, unrelated keys preserved
        let proj_b = raw["projects"]["/home/dev/project-b"].as_object().unwrap();
        assert_eq!(proj_b.get("hasTrustDialogAccepted"), Some(&json!(true)));
        let b_disabled = proj_b["disabledMcpServers"].as_array().unwrap();
        let b_enabled = proj_b["enabledMcpServers"].as_array().unwrap();
        assert_eq!(b_disabled, &vec![json!("context7")]);
        assert_eq!(b_enabled, &vec![json!("Astro-Docs")]);
    }
}
