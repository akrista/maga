use crate::domain::model::{McpServerConfig, Transport, normalize_mcp_server_name};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Represents a parsed GitHub Copilot `~/.copilot/mcp-config.json` document while preserving
/// unknown top-level keys and per-server metadata such as `"tools"` and `"timeout"`.
#[derive(Debug, Clone, Default)]
pub struct CopilotDocument {
    pub raw_root: Map<String, Value>,
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl CopilotDocument {
    pub fn parse_json_str(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }
        let root: Map<String, Value> =
            serde_json::from_str(input).map_err(|e| format!("Invalid Copilot JSON: {e}"))?;
        let mut servers = BTreeMap::new();

        if let Some(Value::Object(mcp_servers)) = root.get("mcpServers") {
            let mut spaced = Vec::new();
            for (name, val) in mcp_servers {
                if let Value::Object(obj) = val
                    && let Some(cfg) = parse_copilot_server(obj)
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
        self.raw_root
            .get("mcpServers")
            .and_then(Value::as_object)
            .is_some_and(|m| m.keys().any(|k| k.chars().any(char::is_whitespace)))
    }

    pub fn to_json_string(
        &self,
        canonical: &BTreeMap<String, McpServerConfig>,
    ) -> Result<String, String> {
        let mut root = self.raw_root.clone();
        let existing_servers = root
            .get("mcpServers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let mut mcp_servers = Map::new();
        for (name, server) in canonical {
            let existing_obj = existing_servers
                .get(name)
                .or_else(|| {
                    existing_servers
                        .iter()
                        .find(|(raw_k, _)| normalize_mcp_server_name(raw_k) == *name)
                        .map(|(_, v)| v)
                })
                .and_then(Value::as_object);
            mcp_servers.insert(name.clone(), serialize_copilot_server(server, existing_obj));
        }

        root.insert("mcpServers".to_string(), Value::Object(mcp_servers));
        serde_json::to_string_pretty(&root)
            .map_err(|e| format!("Failed to serialize Copilot JSON: {e}"))
    }
}

fn parse_copilot_server(obj: &Map<String, Value>) -> Option<McpServerConfig> {
    let disabled = obj
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || obj.get("enabled").and_then(Value::as_bool) == Some(false);
    let enabled = !disabled;

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
        Some("http" | "sse" | "remote") => Transport::Http,
        Some("stdio" | "local") => Transport::Stdio,
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

fn serialize_copilot_server(
    server: &McpServerConfig,
    existing: Option<&Map<String, Value>>,
) -> Value {
    let mut obj = existing.cloned().unwrap_or_default();
    for key in [
        "type",
        "command",
        "args",
        "env",
        "url",
        "serverUrl",
        "headers",
        "disabled",
        "enabled",
    ] {
        obj.remove(key);
    }

    if !obj.contains_key("tools") {
        obj.insert("tools".to_string(), json!(["*"]));
    }

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

    if !server.enabled {
        obj.insert("disabled".to_string(), json!(true));
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
    fn parses_and_serializes_copilot_preserving_tools_timeout_and_top_level_keys() {
        let fixture = r#"{
          "customRootKey": true,
          "mcpServers": {
            "Astro Docs": {
              "tools": ["search_docs"],
              "timeout": 15000,
              "type": "http",
              "url": "https://mcp.docs.astro.build/mcp",
              "disabled": true
            },
            "local-srv": {
              "type": "local",
              "command": "npx",
              "args": ["-y", "@example/mcp"],
              "env": { "API_KEY": "secret" }
            }
          }
        }"#;

        let mut doc = CopilotDocument::parse_json_str(fixture).expect("parse copilot json");
        assert!(doc.has_raw_spaced_keys());
        assert_eq!(doc.servers.len(), 2);
        assert!(!doc.servers.contains_key("Astro Docs"));

        let astro = &doc.servers["Astro-Docs"];
        assert!(!astro.enabled);
        assert_eq!(astro.transport, Transport::Http);

        let local = &doc.servers["local-srv"];
        assert!(local.enabled);
        assert_eq!(local.transport, Transport::Stdio);
        assert_eq!(local.command.as_deref(), Some("npx"));

        // Enable Astro-Docs, disable local-srv, and add a new server without pre-existing tools
        doc.servers.get_mut("Astro-Docs").unwrap().enabled = true;
        doc.servers.get_mut("local-srv").unwrap().enabled = false;
        doc.servers.insert(
            "new-http".to_string(),
            McpServerConfig::new_http("https://new.example.com/mcp"),
        );

        let out = doc
            .to_json_string(&doc.servers)
            .expect("serialize copilot json");
        let raw: Map<String, Value> = serde_json::from_str(&out).expect("parse raw json");
        assert_eq!(raw.get("customRootKey"), Some(&json!(true)));

        let mcp_servers = raw["mcpServers"].as_object().unwrap();
        assert!(!mcp_servers.contains_key("Astro Docs"));
        let astro_raw = mcp_servers["Astro-Docs"].as_object().unwrap();
        assert_eq!(astro_raw.get("tools"), Some(&json!(["search_docs"])));
        assert_eq!(astro_raw.get("timeout"), Some(&json!(15000)));
        assert!(!astro_raw.contains_key("disabled"));

        let local_raw = mcp_servers["local-srv"].as_object().unwrap();
        assert_eq!(local_raw.get("tools"), Some(&json!(["*"])));
        assert_eq!(local_raw.get("disabled"), Some(&json!(true)));

        let new_raw = mcp_servers["new-http"].as_object().unwrap();
        assert_eq!(new_raw.get("tools"), Some(&json!(["*"])));
        assert_eq!(new_raw.get("type"), Some(&json!("http")));

        let reparsed = CopilotDocument::parse_json_str(&out).expect("reparse copilot json");
        assert!(!reparsed.has_raw_spaced_keys());
        assert_eq!(doc.servers, reparsed.servers);
    }
}
