use crate::domain::model::{McpServerConfig, Transport, normalize_mcp_server_name};
use std::collections::BTreeMap;
use toml::{Table, Value};

/// Represents a parsed Grok `~/.grok/config.toml` document while preserving all non-MCP
/// TOML tables (`[cli]`, `[models]`, `[ui]`, etc.) and per-server timeout fields.
#[derive(Debug, Clone, Default)]
pub struct GrokDocument {
    pub raw_root: Table,
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl GrokDocument {
    pub fn parse_toml_str(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }
        let root: Table = toml::from_str(input).map_err(|e| format!("Invalid Grok TOML: {e}"))?;
        let mut servers = BTreeMap::new();

        if let Some(Value::Table(mcp_servers)) = root.get("mcp_servers") {
            let mut spaced = Vec::new();
            for (name, val) in mcp_servers {
                if let Value::Table(obj) = val
                    && let Some(cfg) = parse_grok_server(obj)
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
            .get("mcp_servers")
            .and_then(Value::as_table)
            .is_some_and(|m| m.keys().any(|k| k.chars().any(char::is_whitespace)))
    }

    pub fn to_toml_string(
        &self,
        canonical: &BTreeMap<String, McpServerConfig>,
    ) -> Result<String, String> {
        let mut root = self.raw_root.clone();
        let existing_servers = root
            .get("mcp_servers")
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();

        let mut mcp_servers = Table::new();
        for (name, server) in canonical {
            let existing_obj = existing_servers
                .get(name)
                .or_else(|| {
                    existing_servers
                        .iter()
                        .find(|(raw_k, _)| normalize_mcp_server_name(raw_k) == *name)
                        .map(|(_, v)| v)
                })
                .and_then(Value::as_table);
            mcp_servers.insert(name.clone(), serialize_grok_server(server, existing_obj));
        }

        root.insert("mcp_servers".to_string(), Value::Table(mcp_servers));
        toml::to_string_pretty(&root).map_err(|e| format!("Failed to serialize Grok TOML: {e}"))
    }
}

fn parse_grok_server(obj: &Table) -> Option<McpServerConfig> {
    let enabled = if let Some(disabled) = obj.get("disabled").and_then(Value::as_bool) {
        !disabled
    } else {
        obj.get("enabled").and_then(Value::as_bool).unwrap_or(true)
    };

    let url = obj
        .get("url")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let command = obj
        .get("command")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let transport = match obj
        .get("transport")
        .or_else(|| obj.get("type"))
        .and_then(Value::as_str)
    {
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

    let env = extract_toml_string_map(obj.get("env"));
    let headers = extract_toml_string_map(obj.get("headers"));

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

fn serialize_grok_server(server: &McpServerConfig, existing: Option<&Table>) -> Value {
    let mut obj = existing.cloned().unwrap_or_default();
    for key in [
        "command",
        "args",
        "env",
        "url",
        "headers",
        "enabled",
        "disabled",
        "transport",
        "type",
    ] {
        obj.remove(key);
    }

    match server.transport {
        Transport::Stdio => {
            if let Some(cmd) = &server.command {
                obj.insert("command".to_string(), Value::String(cmd.clone()));
            }
            if !server.args.is_empty() {
                let arr = server
                    .args
                    .iter()
                    .map(|a| Value::String(a.clone()))
                    .collect();
                obj.insert("args".to_string(), Value::Array(arr));
            }
            if !server.env.is_empty() {
                let mut env_tbl = Table::new();
                for (k, v) in &server.env {
                    env_tbl.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("env".to_string(), Value::Table(env_tbl));
            }
        }
        Transport::Http => {
            if let Some(url) = &server.url {
                obj.insert("url".to_string(), Value::String(url.clone()));
            }
            if !server.headers.is_empty() {
                let mut hdr_tbl = Table::new();
                for (k, v) in &server.headers {
                    hdr_tbl.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("headers".to_string(), Value::Table(hdr_tbl));
            }
        }
    }

    obj.insert("enabled".to_string(), Value::Boolean(server.enabled));

    Value::Table(obj)
}

fn extract_toml_string_map(val: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(Value::Table(map)) = val {
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
    fn parses_and_serializes_grok_config_preserving_non_mcp_tables_and_timeouts() {
        let fixture = r#"
[cli]
installer = "internal"

[models]
default = "grok-4.5"

[mcp_servers.firecrawl]
command = "npx"
args = ["-y", "firecrawl-mcp"]
env = { FIRECRAWL_API_KEY = "fc-secret" }
startup_timeout_sec = 60
tool_timeout_sec = 120
enabled = true

[mcp_servers."Astro Docs"]
url = "https://mcp.docs.astro.build/mcp"
headers = { "X-Custom" = "val" }
enabled = false
"#;

        let mut doc = GrokDocument::parse_toml_str(fixture).expect("parse grok toml");
        assert!(doc.has_raw_spaced_keys());
        assert_eq!(doc.servers.len(), 2);
        assert!(!doc.servers.contains_key("Astro Docs"));

        let fc = &doc.servers["firecrawl"];
        assert!(fc.enabled);
        assert_eq!(fc.transport, Transport::Stdio);
        assert_eq!(fc.command.as_deref(), Some("npx"));
        assert_eq!(
            fc.env.get("FIRECRAWL_API_KEY").map(String::as_str),
            Some("fc-secret")
        );

        let astro = &doc.servers["Astro-Docs"];
        assert!(!astro.enabled);
        assert_eq!(astro.transport, Transport::Http);

        // Toggle both and serialize
        doc.servers.get_mut("firecrawl").unwrap().enabled = false;
        doc.servers.get_mut("Astro-Docs").unwrap().enabled = true;

        let out = doc
            .to_toml_string(&doc.servers)
            .expect("serialize grok toml");
        let raw: Table = toml::from_str(&out).expect("parse raw toml");

        // Non-MCP tables preserved
        assert_eq!(
            raw["cli"].as_table().unwrap()["installer"].as_str(),
            Some("internal")
        );
        assert_eq!(
            raw["models"].as_table().unwrap()["default"].as_str(),
            Some("grok-4.5")
        );

        // Timeout fields preserved on firecrawl, and Astro Docs normalized to Astro-Docs
        let mcp_tbl = raw["mcp_servers"].as_table().unwrap();
        assert!(!mcp_tbl.contains_key("Astro Docs"));
        assert!(mcp_tbl.contains_key("Astro-Docs"));
        let fc_raw = mcp_tbl["firecrawl"].as_table().unwrap();
        assert_eq!(fc_raw["startup_timeout_sec"].as_integer(), Some(60));
        assert_eq!(fc_raw["tool_timeout_sec"].as_integer(), Some(120));
        assert_eq!(fc_raw["enabled"].as_bool(), Some(false));

        let reparsed = GrokDocument::parse_toml_str(&out).expect("reparse grok toml");
        assert!(!reparsed.has_raw_spaced_keys());
        assert_eq!(doc.servers, reparsed.servers);
    }
}
