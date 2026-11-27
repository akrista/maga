//! Per-harness secret delivery. `~/.config/maga/.env.local` stays the single source of truth;
//! when syncing, each harness gets the secrets through the mechanism its official docs
//! describe, always inside user-local, unversioned files.
//!
//! | Harness      | Mechanism                                                                    |
//! |--------------|------------------------------------------------------------------------------|
//! | Claude Code  | `${VAR}` in `~/.claude.json`, values in `~/.claude/settings.json` -> `env`    |
//! | OpenCode     | `{file:<maga dir>/secrets/VAR}` in `opencode.json`, one local file per secret |
//! | Antigravity  | no documented env/file mechanism -> literal value in its local config         |
//! | Copilot CLI  | only `${VAR}` from the host environment -> literal value in its local config  |
//! | Grok         | only `${VAR}` from the host environment -> literal value in its local config  |

use crate::domain::model::{HarnessKind, McpServerConfig, interpolate_string};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretDelivery {
    /// Keep `${VAR}`; the harness reads the value from the `env` block of its own settings file.
    SettingsEnv,
    /// Replace references with `{file:...}` pointing at a per-secret local file.
    FileRef,
    /// No documented secret mechanism: resolve the value into the harness config.
    Literal,
}

pub fn delivery(kind: HarnessKind) -> SecretDelivery {
    match kind {
        HarnessKind::Claude => SecretDelivery::SettingsEnv,
        HarnessKind::OpenCode => SecretDelivery::FileRef,
        HarnessKind::Antigravity | HarnessKind::Copilot | HarnessKind::Grok => {
            SecretDelivery::Literal
        }
    }
}

/// One-line description of how secrets reach `kind` (shown in the harness view).
pub fn describe(kind: HarnessKind) -> &'static str {
    match kind {
        HarnessKind::Claude => "${VAR} references; values in ~/.claude/settings.json `env`",
        HarnessKind::OpenCode => "{file:…} references; one local file per secret in maga/secrets",
        HarnessKind::Antigravity => {
            "no documented env/file mechanism: values stored as literals in its local config"
        }
        HarnessKind::Copilot => {
            "no env-file mechanism (only ${VAR} from the host environment): values stored as literals"
        }
        HarnessKind::Grok => {
            "no env-file mechanism (only ${VAR} from the host environment): values stored as literals"
        }
    }
}

/// A server prepared for one harness plus the secret values that must be persisted next to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub server: McpServerConfig,
    /// `VAR -> value` the delivery mechanism needs on disk (settings env or secret files).
    pub values: BTreeMap<String, String>,
    /// Secret values ended up literally inside the harness config.
    pub literal: bool,
}

pub fn deliver(
    server: &McpServerConfig,
    kind: HarnessKind,
    env: &BTreeMap<String, String>,
    secrets_dir: &Path,
) -> Delivered {
    let referenced = server.referenced_env_vars();
    match delivery(kind) {
        SecretDelivery::SettingsEnv => Delivered {
            server: server.clone(),
            values: referenced
                .into_iter()
                .filter_map(|v| env.get(&v).map(|val| (v, val.clone())))
                .collect(),
            literal: false,
        },
        SecretDelivery::Literal => Delivered {
            server: server.interpolate(env),
            values: BTreeMap::new(),
            literal: !referenced.is_empty(),
        },
        SecretDelivery::FileRef => {
            // OpenCode documents substitution for `environment` and `headers` only; the other
            // fields are resolved to literals.
            let mut out = server.interpolate(env);
            out.env = server.env.clone();
            out.headers = server.headers.clone();
            let mut values = BTreeMap::new();
            for value in out.env.values_mut().chain(out.headers.values_mut()) {
                let with_files = replace_braced_refs(value, |var| {
                    env.get(var).map(|secret| {
                        values.insert(var.to_string(), secret.clone());
                        format!("{{file:{}}}", ref_path(secrets_dir, var))
                    })
                });
                *value = interpolate_string(&with_files, env);
            }
            let literal = [&server.command, &server.url]
                .into_iter()
                .flatten()
                .chain(&server.args)
                .any(|s| s.contains('$'));
            Delivered {
                server: out,
                values,
                literal,
            }
        }
    }
}

/// `~/…` when under the home directory, forward slashes always.
fn ref_path(dir: &Path, var: &str) -> String {
    let path = dir.join(var);
    let shown = std::env::home_dir()
        .and_then(|home| {
            path.strip_prefix(&home)
                .ok()
                .map(|rel| format!("~/{}", rel.display()))
        })
        .unwrap_or_else(|| path.display().to_string());
    shown.replace('\\', "/")
}

fn is_var_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Replaces each `${NAME}` for which `f` returns `Some`; everything else is kept verbatim.
fn replace_braced_refs(input: &str, mut f: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) if is_var_name(&after[..end]) => {
                match f(&after[..end]) {
                    Some(replacement) => out.push_str(&replacement),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Inverse of the OpenCode delivery: `{env:NAME}` and `{file:…/secrets/NAME}` become `${NAME}`,
/// so reconciliation and imports see the canonical form.
pub fn restore_secret_refs(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let restored = after.find('}').and_then(|end| {
            let body = &after[..end];
            let name: Option<String> = if let Some(name) = body.strip_prefix("env:") {
                Some(name.to_string())
            } else {
                body.strip_prefix("file:").and_then(|path| {
                    let path = path.replace('\\', "/");
                    let (dir, name) = path.rsplit_once('/')?;
                    dir.ends_with("secrets").then(|| name.to_string())
                })
            };
            name.filter(|n| is_var_name(n)).map(|n| (n, end))
        });
        match restored {
            Some((name, end)) => {
                out.push_str(&format!("${{{name}}}"));
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Applies [`restore_secret_refs`] to every string field of a server read from a harness.
pub fn restore_server(server: &mut McpServerConfig) {
    let restore = |s: &mut String| *s = restore_secret_refs(s);
    server.command.iter_mut().for_each(restore);
    server.args.iter_mut().for_each(restore);
    server.url.iter_mut().for_each(restore);
    server.env.values_mut().for_each(restore);
    server.headers.values_mut().for_each(restore);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> McpServerConfig {
        let mut s = McpServerConfig::new_http("https://api.example.com/mcp?k=${API_KEY}");
        s.headers
            .insert("Authorization".into(), "Bearer ${API_TOKEN}".into());
        s.env.insert("PLAIN".into(), "x".into());
        s.env
            .insert("DB_PASS".into(), "${DB_PASS:-fallback}".into());
        s
    }

    fn env() -> BTreeMap<String, String> {
        [("API_KEY", "k1"), ("API_TOKEN", "t1")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn claude_keeps_references_and_returns_values_for_settings_env() {
        let d = deliver(&server(), HarnessKind::Claude, &env(), Path::new("/s"));
        assert_eq!(d.server, server());
        assert_eq!(d.values["API_TOKEN"], "t1");
        assert_eq!(d.values["API_KEY"], "k1");
        assert!(!d.literal);
    }

    #[test]
    fn opencode_uses_file_refs_in_env_and_headers_and_literals_elsewhere() {
        let d = deliver(
            &server(),
            HarnessKind::OpenCode,
            &env(),
            Path::new("/data/maga/secrets"),
        );
        assert_eq!(
            d.server.headers["Authorization"],
            "Bearer {file:/data/maga/secrets/API_TOKEN}"
        );
        assert_eq!(d.server.env["DB_PASS"], "fallback", "defaults resolve");
        assert_eq!(d.server.env["PLAIN"], "x");
        // url has no documented substitution: the value is resolved into it.
        assert_eq!(
            d.server.url.as_deref(),
            Some("https://api.example.com/mcp?k=k1")
        );
        assert!(d.literal);
        assert_eq!(d.values["API_TOKEN"], "t1");
        assert!(
            !d.values.contains_key("API_KEY"),
            "url secret is not a file"
        );
    }

    #[test]
    fn other_harnesses_get_literal_values() {
        for kind in [
            HarnessKind::Antigravity,
            HarnessKind::Copilot,
            HarnessKind::Grok,
        ] {
            let d = deliver(&server(), kind, &env(), Path::new("/s"));
            assert_eq!(d.server.headers["Authorization"], "Bearer t1");
            assert!(d.literal && d.values.is_empty());
        }
        let plain = McpServerConfig::new_stdio("node", vec![]);
        assert!(!deliver(&plain, HarnessKind::Grok, &env(), Path::new("/s")).literal);
    }

    #[test]
    fn restore_maps_env_and_secret_file_refs_back_to_canonical() {
        assert_eq!(
            restore_secret_refs("Bearer {file:~/.config/maga/secrets/API_TOKEN}"),
            "Bearer ${API_TOKEN}"
        );
        assert_eq!(restore_secret_refs("{env:MCP_API_KEY}"), "${MCP_API_KEY}");
        assert_eq!(
            restore_secret_refs("C:\\x\\maga\\secrets\\A").as_str(),
            "C:\\x\\maga\\secrets\\A"
        );
        assert_eq!(
            restore_secret_refs("{file:C:\\x\\maga\\secrets\\A}"),
            "${A}"
        );
        // Unrelated braces and other file refs are left alone.
        assert_eq!(restore_secret_refs("{file:~/.keys/a}"), "{file:~/.keys/a}");
        assert_eq!(restore_secret_refs("{\"a\": 1} {"), "{\"a\": 1} {");
    }
}
