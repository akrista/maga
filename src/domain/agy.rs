use crate::domain::model::{McpServerConfig, Transport, normalize_mcp_server_name};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Represents a parsed Antigravity `mcp_config.json` document while preserving unknown top-level keys.
#[derive(Debug, Clone, Default)]
pub struct AgyDocument {
    pub raw_root: Map<String, Value>,
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl AgyDocument {
    pub fn parse_json_str(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }
        let root: Map<String, Value> =
            serde_json::from_str(input).map_err(|e| format!("Invalid Antigravity JSON: {e}"))?;
        let mut servers = BTreeMap::new();

        if let Some(Value::Object(mcp_servers)) = root.get("mcpServers") {
            let mut spaced = Vec::new();
            for (name, val) in mcp_servers {
                if let Value::Object(obj) = val
                    && let Some(cfg) = parse_agy_server(obj)
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
        let mut mcp_servers = Map::new();

        for (name, server) in canonical {
            mcp_servers.insert(name.clone(), serialize_agy_server(server));
        }

        root.insert("mcpServers".to_string(), Value::Object(mcp_servers));
        serde_json::to_string_pretty(&root)
            .map_err(|e| format!("Failed to serialize Antigravity JSON: {e}"))
    }
}

fn parse_agy_server(obj: &Map<String, Value>) -> Option<McpServerConfig> {
    let disabled = obj
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
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

fn serialize_agy_server(server: &McpServerConfig) -> Value {
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
    fn parses_legacy_server_url_and_modern_agy_entries() {
        let fixture = r#"{
          "mcpServers": {
            "Astro Docs": {
              "disabled": true,
              "serverUrl": "https://mcp.docs.astro.build/mcp"
            },
            "Context7": {
              "headers": {
                "CONTEXT7_API_KEY": "k1"
              },
              "serverUrl": "https://mcp.context7.com/mcp"
            },
            "sqlite-mcp": {
              "type": "stdio",
              "command": "/bin/sqlite-mcp",
              "args": ["mcp"]
            }
          }
        }"#;

        let doc = AgyDocument::parse_json_str(fixture).expect("parse agy json");
        assert!(doc.has_raw_spaced_keys());
        assert_eq!(doc.servers.len(), 3);
        assert!(!doc.servers.contains_key("Astro Docs"));

        let astro = &doc.servers["Astro-Docs"];
        assert!(!astro.enabled);
        assert_eq!(astro.transport, Transport::Http);
        assert_eq!(
            astro.url.as_deref(),
            Some("https://mcp.docs.astro.build/mcp")
        );

        let ctx7 = &doc.servers["Context7"];
        assert!(ctx7.enabled);
        assert_eq!(
            ctx7.headers.get("CONTEXT7_API_KEY").map(String::as_str),
            Some("k1")
        );

        let out_json = doc
            .to_json_string(&doc.servers)
            .expect("serialize agy json");
        let reparsed = AgyDocument::parse_json_str(&out_json).expect("reparse agy json");
        assert!(!reparsed.has_raw_spaced_keys());
        assert_eq!(doc.servers, reparsed.servers);
        assert!(!out_json.contains("serverUrl"));
        assert!(!out_json.contains("Astro Docs"));
        assert!(out_json.contains("Astro-Docs"));
    }
}
