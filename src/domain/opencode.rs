use crate::domain::model::{McpServerConfig, Transport, normalize_mcp_server_name};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Represents a parsed OpenCode `~/.config/opencode/opencode.json` document while preserving
/// `"$schema"`, `"instructions"`, `"permission"`, `"plugin"`, `"plugins"`, `"provider"`,
/// and per-server fields such as `"oauth"` and `"cwd"`.
#[derive(Debug, Clone, Default)]
pub struct OpenCodeDocument {
    pub raw_root: Map<String, Value>,
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl OpenCodeDocument {
    pub fn parse_json_str(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }
        let root: Map<String, Value> =
            serde_json::from_str(input).map_err(|e| format!("Invalid OpenCode JSON: {e}"))?;
        let mut servers = BTreeMap::new();

        if let Some(Value::Object(mcp)) = root.get("mcp") {
            let source_map = if mcp.len() == 1
                && let Some(Value::Object(nested)) = mcp.get("servers")
                && !nested.contains_key("type")
                && !nested.contains_key("url")
                && !nested.contains_key("command")
            {
                nested
            } else {
                mcp
            };

            let mut spaced = Vec::new();
            for (name, val) in source_map {
                if let Value::Object(obj) = val
                    && let Some(cfg) = parse_opencode_server(obj)
                {
                    let norm = normalize_mcp_server_name(name);
                    if norm.is_empty() {
                        continue;
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
        if let Some(Value::Object(mcp)) = self.raw_root.get("mcp") {
            let source_map = if mcp.len() == 1
                && let Some(Value::Object(nested)) = mcp.get("servers")
                && !nested.contains_key("type")
                && !nested.contains_key("url")
                && !nested.contains_key("command")
            {
                nested
            } else {
                mcp
            };
            return source_map
                .keys()
                .any(|k| k.chars().any(char::is_whitespace));
        }
        false
    }

    pub fn to_json_string(
        &self,
        canonical: &BTreeMap<String, McpServerConfig>,
    ) -> Result<String, String> {
        let mut root = self.raw_root.clone();
        let existing_mcp = root
            .get("mcp")
            .and_then(Value::as_object)
            .map(|mcp| {
                if mcp.len() == 1
                    && let Some(Value::Object(nested)) = mcp.get("servers")
                    && !nested.contains_key("type")
                    && !nested.contains_key("url")
                    && !nested.contains_key("command")
                {
                    nested.clone()
                } else {
                    mcp.clone()
                }
            })
            .unwrap_or_default();

        let mut mcp_servers = Map::new();
        for (name, server) in canonical {
            let existing_obj = existing_mcp
                .get(name)
                .or_else(|| {
                    existing_mcp
                        .iter()
                        .find(|(raw_k, _)| normalize_mcp_server_name(raw_k) == *name)
                        .map(|(_, v)| v)
                })
                .and_then(Value::as_object);
            mcp_servers.insert(
                name.clone(),
                serialize_opencode_server(server, existing_obj),
            );
        }

        root.insert("mcp".to_string(), Value::Object(mcp_servers));
        serde_json::to_string_pretty(&root)
            .map_err(|e| format!("Failed to serialize OpenCode JSON: {e}"))
    }
}

fn parse_opencode_server(obj: &Map<String, Value>) -> Option<McpServerConfig> {
    let enabled = if let Some(en) = obj.get("enabled").and_then(Value::as_bool) {
        en
    } else if let Some(dis) = obj.get("disabled").and_then(Value::as_bool) {
        !dis
    } else {
        true
    };

    let url = obj
        .get("url")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let (command, args) = match obj.get("command") {
        Some(Value::Array(arr)) => {
            let mut parts: Vec<String> = arr
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect();
            if parts.is_empty() {
                (None, Vec::new())
            } else {
                let cmd = parts.remove(0);
                (Some(cmd), parts)
            }
        }
        Some(Value::String(cmd)) => {
            let extra_args = obj
                .get("args")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            (Some(cmd.clone()), extra_args)
        }
        _ => (None, Vec::new()),
    };

    let transport = match obj.get("type").and_then(Value::as_str) {
        Some("remote" | "http" | "sse") => Transport::Http,
        Some("local" | "stdio") => Transport::Stdio,
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

    let env = if obj.contains_key("environment") {
        extract_string_map(obj.get("environment"))
    } else {
        extract_string_map(obj.get("env"))
    };
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

fn serialize_opencode_server(
    server: &McpServerConfig,
    existing: Option<&Map<String, Value>>,
) -> Value {
    let mut obj = existing.cloned().unwrap_or_default();
    for key in [
        "type",
        "command",
        "args",
        "environment",
        "env",
        "url",
        "headers",
        "enabled",
        "disabled",
    ] {
        obj.remove(key);
    }

    match server.transport {
        Transport::Stdio => {
            obj.insert("type".to_string(), json!("local"));
            let mut cmd_vec = Vec::new();
            if let Some(cmd) = &server.command {
                cmd_vec.push(cmd.clone());
            }
            cmd_vec.extend(server.args.iter().cloned());
            obj.insert("command".to_string(), json!(cmd_vec));
            if !server.env.is_empty() {
                obj.insert("environment".to_string(), json!(server.env));
            }
        }
        Transport::Http => {
            obj.insert("type".to_string(), json!("remote"));
            if let Some(url) = &server.url {
                obj.insert("url".to_string(), json!(url));
            }
            if !server.headers.is_empty() {
                obj.insert("headers".to_string(), json!(server.headers));
            }
        }
    }

    if !server.enabled {
        obj.insert("enabled".to_string(), json!(false));
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
    fn parses_and_serializes_opencode_local_and_remote_preserving_metadata() {
        let fixture = r#"{
          "$schema": "https://opencode.ai/config.json",
          "instructions": ["maga-rules.md"],
          "permission": "allow",
          "plugin": ["opencode-wakatime"],
          "provider": {
            "openai": {
              "options": { "baseURL": "https://api.example.com/" }
            }
          },
          "mcp": {
            "everything": {
              "type": "local",
              "command": ["npx", "-y", "@modelcontextprotocol/server-everything"],
              "cwd": ".",
              "environment": {
                "LOG_LEVEL": "info"
              }
            },
            "Astro Docs": {
              "type": "remote",
              "url": "https://mcp.docs.astro.build/mcp",
              "oauth": true
            },
            "sentry": {
              "type": "remote",
              "url": "https://mcp.sentry.dev/mcp",
              "enabled": false,
              "oauth": false,
              "headers": {
                "Authorization": "Bearer token"
              }
            }
          }
        }"#;

        let mut doc = OpenCodeDocument::parse_json_str(fixture).expect("parse opencode json");
        assert!(doc.has_raw_spaced_keys());
        assert_eq!(doc.servers.len(), 3);
        assert!(doc.servers.contains_key("Astro-Docs"));
        assert!(!doc.servers.contains_key("Astro Docs"));

        let everything = &doc.servers["everything"];
        assert!(everything.enabled);
        assert_eq!(everything.transport, Transport::Stdio);
        assert_eq!(everything.command.as_deref(), Some("npx"));
        assert_eq!(
            everything.args,
            vec!["-y", "@modelcontextprotocol/server-everything"]
        );
        assert_eq!(
            everything.env.get("LOG_LEVEL").map(String::as_str),
            Some("info")
        );

        let sentry = &doc.servers["sentry"];
        assert!(!sentry.enabled);
        assert_eq!(sentry.transport, Transport::Http);
        assert_eq!(sentry.url.as_deref(), Some("https://mcp.sentry.dev/mcp"));

        // Flip states
        doc.servers.get_mut("everything").unwrap().enabled = false;
        doc.servers.get_mut("sentry").unwrap().enabled = true;

        let out = doc
            .to_json_string(&doc.servers)
            .expect("serialize opencode json");
        let raw: Map<String, Value> = serde_json::from_str(&out).expect("parse raw json");

        assert_eq!(
            raw.get("$schema"),
            Some(&json!("https://opencode.ai/config.json"))
        );
        assert_eq!(raw.get("permission"), Some(&json!("allow")));
        assert!(raw.contains_key("provider"));

        let mcp = raw["mcp"].as_object().unwrap();
        assert!(!mcp.contains_key("Astro Docs"));
        let astro_raw = mcp["Astro-Docs"].as_object().unwrap();
        assert_eq!(astro_raw.get("oauth"), Some(&json!(true)));
        let ev_raw = mcp["everything"].as_object().unwrap();
        assert_eq!(ev_raw.get("type"), Some(&json!("local")));
        assert_eq!(
            ev_raw.get("command"),
            Some(&json!([
                "npx",
                "-y",
                "@modelcontextprotocol/server-everything"
            ]))
        );
        assert_eq!(ev_raw.get("cwd"), Some(&json!(".")));
        assert_eq!(ev_raw.get("enabled"), Some(&json!(false)));

        let sentry_raw = mcp["sentry"].as_object().unwrap();
        assert_eq!(sentry_raw.get("type"), Some(&json!("remote")));
        assert_eq!(sentry_raw.get("oauth"), Some(&json!(false)));
        assert!(!sentry_raw.contains_key("enabled"));

        let reparsed = OpenCodeDocument::parse_json_str(&out).expect("reparse opencode json");
        assert!(!reparsed.has_raw_spaced_keys());
        assert_eq!(doc.servers, reparsed.servers);
    }
}
