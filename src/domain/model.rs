use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Transport mechanism used by an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    Http,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

/// Canonical representation of an MCP server managed by `maga`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub transport: Transport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

fn default_enabled() -> bool {
    true
}

/// Normalizes an optional MCP server description by trimming outer whitespace and returning
/// `None` if the trimmed string is empty.
pub fn normalize_mcp_description(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

impl McpServerConfig {
    pub fn new_stdio(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            enabled: true,
            description: None,
            transport: Transport::Stdio,
            command: Some(command.into()),
            args,
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
        }
    }

    pub fn new_http(url: impl Into<String>) -> Self {
        Self {
            enabled: true,
            description: None,
            transport: Transport::Http,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            url: Some(url.into()),
            headers: BTreeMap::new(),
        }
    }

    /// Trims `self.description` in-place and sets it to `None` when blank.
    pub fn normalize_description(&mut self) {
        self.description = normalize_mcp_description(self.description.take());
    }

    /// Validates that required fields for the selected transport are present and non-empty.
    pub fn validate(&self) -> Result<(), String> {
        match self.transport {
            Transport::Stdio => {
                let cmd = self.command.as_deref().unwrap_or("").trim();
                if cmd.is_empty() {
                    return Err("Stdio MCP server requires a non-empty `command`.".to_string());
                }
            }
            Transport::Http => {
                let url = self.url.as_deref().unwrap_or("").trim();
                if url.is_empty() {
                    return Err("HTTP MCP server requires a non-empty `url`.".to_string());
                }
            }
        }
        Ok(())
    }

    /// Compares core definition fields (ignoring `enabled` and `description`) to detect definition drift.
    pub fn same_definition(&self, other: &Self) -> bool {
        self.transport == other.transport
            && self.command == other.command
            && self.args == other.args
            && self.env == other.env
            && self.url == other.url
            && self.headers == other.headers
    }

    /// Compares harness-visible fields (`enabled` and transport definition, ignoring `description`).
    pub fn same_harness_state(&self, other: &Self) -> bool {
        self.enabled == other.enabled && self.same_definition(other)
    }

    /// Primary endpoint summary (`command` + `args` or `url`) for list filtering and display.
    pub fn endpoint_summary(&self) -> String {
        match self.transport {
            Transport::Stdio => {
                let cmd = self.command.as_deref().unwrap_or("");
                if self.args.is_empty() {
                    cmd.to_string()
                } else {
                    format!("{} {}", cmd, self.args.join(" "))
                }
            }
            Transport::Http => self.url.clone().unwrap_or_default(),
        }
    }
}

/// Periodic auto-refresh settings in `~/.config/maga/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_refresh_interval")]
    pub interval_secs: u64,
}

fn default_refresh_interval() -> u64 {
    5
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_refresh_interval(),
        }
    }
}

pub const DEFAULT_AGY_CONFIG_PATH: &str = "~/.gemini/config/mcp_config.json";
pub const DEFAULT_CLAUDE_CONFIG_PATH: &str = "~/.claude.json";
pub const DEFAULT_COPILOT_CONFIG_PATH: &str = "~/.copilot/mcp-config.json";
pub const DEFAULT_GROK_CONFIG_PATH: &str = "~/.grok/config.toml";
pub const DEFAULT_OPENCODE_CONFIG_PATH: &str = "~/.config/opencode/opencode.json";

/// Supported agentic harnesses managed by `maga`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessKind {
    Antigravity,
    Claude,
    Copilot,
    Grok,
    OpenCode,
}

impl HarnessKind {
    pub const ALL: [Self; 5] = [
        Self::Antigravity,
        Self::Claude,
        Self::Copilot,
        Self::Grok,
        Self::OpenCode,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Antigravity => "antigravity",
            Self::Claude => "claude",
            Self::Copilot => "copilot",
            Self::Grok => "grok",
            Self::OpenCode => "opencode",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Agy",
            Self::Claude => "Claude",
            Self::Copilot => "Copilot",
            Self::Grok => "Grok",
            Self::OpenCode => "OpenCode",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Claude => "Claude Code",
            Self::Copilot => "Copilot",
            Self::Grok => "Grok",
            Self::OpenCode => "OpenCode",
        }
    }

    pub fn default_config_path(self) -> &'static str {
        match self {
            Self::Antigravity => DEFAULT_AGY_CONFIG_PATH,
            Self::Claude => DEFAULT_CLAUDE_CONFIG_PATH,
            Self::Copilot => DEFAULT_COPILOT_CONFIG_PATH,
            Self::Grok => DEFAULT_GROK_CONFIG_PATH,
            Self::OpenCode => DEFAULT_OPENCODE_CONFIG_PATH,
        }
    }
}

/// Per-harness tool settings inside `[tools.<harness>]` in `~/.config/maga/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolHarnessConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

impl Default for ToolHarnessConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            config_path: None,
        }
    }
}

fn default_agy_tool_config() -> ToolHarnessConfig {
    ToolHarnessConfig {
        enabled: true,
        config_path: Some(DEFAULT_AGY_CONFIG_PATH.to_string()),
    }
}

fn default_claude_tool_config() -> ToolHarnessConfig {
    ToolHarnessConfig {
        enabled: true,
        config_path: Some(DEFAULT_CLAUDE_CONFIG_PATH.to_string()),
    }
}

fn default_copilot_tool_config() -> ToolHarnessConfig {
    ToolHarnessConfig {
        enabled: true,
        config_path: Some(DEFAULT_COPILOT_CONFIG_PATH.to_string()),
    }
}

fn default_grok_tool_config() -> ToolHarnessConfig {
    ToolHarnessConfig {
        enabled: true,
        config_path: Some(DEFAULT_GROK_CONFIG_PATH.to_string()),
    }
}

fn default_opencode_tool_config() -> ToolHarnessConfig {
    ToolHarnessConfig {
        enabled: true,
        config_path: Some(DEFAULT_OPENCODE_CONFIG_PATH.to_string()),
    }
}

/// Tool harness configuration table (`[tools]`) in `~/.config/maga/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_agy_tool_config")]
    pub antigravity: ToolHarnessConfig,
    #[serde(default = "default_claude_tool_config")]
    pub claude: ToolHarnessConfig,
    #[serde(default = "default_copilot_tool_config")]
    pub copilot: ToolHarnessConfig,
    #[serde(default = "default_grok_tool_config")]
    pub grok: ToolHarnessConfig,
    #[serde(default = "default_opencode_tool_config")]
    pub opencode: ToolHarnessConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            antigravity: default_agy_tool_config(),
            claude: default_claude_tool_config(),
            copilot: default_copilot_tool_config(),
            grok: default_grok_tool_config(),
            opencode: default_opencode_tool_config(),
        }
    }
}

impl ToolsConfig {
    pub fn get(&self, kind: HarnessKind) -> &ToolHarnessConfig {
        match kind {
            HarnessKind::Antigravity => &self.antigravity,
            HarnessKind::Claude => &self.claude,
            HarnessKind::Copilot => &self.copilot,
            HarnessKind::Grok => &self.grok,
            HarnessKind::OpenCode => &self.opencode,
        }
    }

    pub fn get_mut(&mut self, kind: HarnessKind) -> &mut ToolHarnessConfig {
        match kind {
            HarnessKind::Antigravity => &mut self.antigravity,
            HarnessKind::Claude => &mut self.claude,
            HarnessKind::Copilot => &mut self.copilot,
            HarnessKind::Grok => &mut self.grok,
            HarnessKind::OpenCode => &mut self.opencode,
        }
    }

    pub fn is_enabled(&self, kind: HarnessKind) -> bool {
        self.get(kind).enabled
    }

    pub fn enabled_harnesses(&self) -> Vec<HarnessKind> {
        HarnessKind::ALL
            .into_iter()
            .filter(|k| self.is_enabled(*k))
            .collect()
    }

    pub fn enabled_short_names(&self, sep: &str) -> String {
        let names: Vec<&str> = self
            .enabled_harnesses()
            .into_iter()
            .map(HarnessKind::short_name)
            .collect();
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(sep)
        }
    }

    pub fn enabled_count_label(&self) -> String {
        match self.enabled_harnesses().len() {
            0 => "no harnesses".to_string(),
            1 => "1 harness".to_string(),
            n => format!("{n} harnesses"),
        }
    }
}

/// Selectable visual color themes for the `maga` TUI (`[ui].theme` in `config.toml`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    Indigo,
    #[default]
    Blush,
    Elite,
}

impl ThemeName {
    pub const ALL: [Self; 3] = [Self::Indigo, Self::Blush, Self::Elite];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Indigo => "indigo",
            Self::Blush => "blush",
            Self::Elite => "elite",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Indigo => "indigo (Arcane Night)",
            Self::Blush => "blush (Warm Sunset)",
            Self::Elite => "elite (Celestial Periwinkle)",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Indigo => Self::Blush,
            Self::Blush => Self::Elite,
            Self::Elite => Self::Indigo,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Indigo => Self::Elite,
            Self::Blush => Self::Indigo,
            Self::Elite => Self::Blush,
        }
    }
}

/// Mascot rendering mode (`[ui].mascot_mode` in `config.toml`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MascotMode {
    Rich,
    #[default]
    Lite,
}

impl MascotMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rich => "rich",
            Self::Lite => "lite",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Rich => "rich (Companion Box)",
            Self::Lite => "lite (ASCII Splash)",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Rich => Self::Lite,
            Self::Lite => Self::Rich,
        }
    }
}

/// User interface preferences (`[ui]` table in `~/.config/maga/config.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub theme: ThemeName,
    #[serde(default)]
    pub mascot_mode: MascotMode,
    #[serde(default)]
    pub background: bool,
    #[serde(default = "default_enabled")]
    pub hero_mascot: bool,
    #[serde(default)]
    pub hide_ui: bool,
    #[serde(default = "default_enabled")]
    pub animations: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: ThemeName::Blush,
            mascot_mode: MascotMode::Lite,
            background: false,
            hero_mascot: true,
            hide_ui: false,
            animations: true,
            editor: None,
        }
    }
}

/// Root structure of `~/.config/maga/config.toml` for general `maga` settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MagaConfig {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub refresh: RefreshConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(flatten)]
    pub settings: toml::Table,
}

impl MagaConfig {
    pub fn from_toml_str(input: &str) -> Result<Self, String> {
        let mut cfg: Self = toml::from_str(input).map_err(|e| format!("Invalid TOML: {e}"))?;
        for kind in HarnessKind::ALL {
            let entry = cfg.tools.get_mut(kind);
            if entry
                .config_path
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
            {
                entry.config_path = Some(kind.default_config_path().to_string());
            }
        }
        Ok(cfg)
    }

    pub fn to_toml_string(&self) -> Result<String, String> {
        let body =
            toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize TOML: {e}"))?;
        let header = "\
# maga General Configuration\n\
# Note: MCP server definitions are managed separately in mcp.toml.\n\
#\n\
# [ui]\n\
#   theme         - Active TUI color theme: \"indigo\" | \"blush\" | \"elite\"\n\
#   mascot_mode   - Companion mode: \"rich\" (moe/chibi companion box) | \"lite\" (ASCII splash)\n\
#   background    - Enable full-TUI Riverflow ASCII background watermark (true/false)\n\
#   hero_mascot   - Enable high-res Hero Mascot Showcase via [m] key and empty state (true/false)\n\
#   hide_ui       - Hide foreground UI panels to display only the background art via [b] key (true/false)\n\
#   animations    - Enable arcane ASCII flow/glitch and tachyonfx micro-animations (true/false)\n\
#\n\
# [refresh]\n\
#   enabled       - Enable periodic background disk reload & reconciliation (true/false)\n\
#   interval_secs - Polling interval in seconds for auto-refresh (0 disables timer)\n\
#\n\
# [tools.antigravity] / [tools.claude] / [tools.copilot] / [tools.grok] / [tools.opencode]\n\
#   enabled       - Sync MCP servers with this harness (true/false)\n\
#   config_path   - Path to the harness MCP config file (supports leading ~/)\n\n";
        Ok(format!("{header}{body}"))
    }

    /// Returns `true` if the raw TOML string already contains all standard `maga` configuration
    /// tables and keys (`[ui]`, `[refresh]`, and all `[tools.<harness>]` tables, including `config_path`).
    pub fn has_all_default_keys(raw: &str) -> bool {
        let Ok(table) = toml::from_str::<toml::Table>(raw) else {
            return false;
        };
        let Some(ui) = table.get("ui").and_then(|v| v.as_table()) else {
            return false;
        };
        if !ui.contains_key("theme")
            || !ui.contains_key("mascot_mode")
            || !ui.contains_key("background")
            || !ui.contains_key("hero_mascot")
            || !ui.contains_key("hide_ui")
            || !ui.contains_key("animations")
        {
            return false;
        }
        let Some(refresh) = table.get("refresh").and_then(|v| v.as_table()) else {
            return false;
        };
        if !refresh.contains_key("enabled") || !refresh.contains_key("interval_secs") {
            return false;
        }
        let Some(tools) = table.get("tools").and_then(|v| v.as_table()) else {
            return false;
        };
        for kind in HarnessKind::ALL {
            let Some(h_table) = tools.get(kind.key()).and_then(|v| v.as_table()) else {
                return false;
            };
            if !h_table.contains_key("enabled") {
                return false;
            }
            let Some(path_str) = h_table.get("config_path").and_then(|v| v.as_str()) else {
                return false;
            };
            if path_str.trim().is_empty() {
                return false;
            }
        }
        true
    }

    /// Returns the configured external editor from top-level `editor` or `[ui].editor` if non-empty.
    pub fn configured_editor(&self) -> Option<&str> {
        self.settings
            .get("editor")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.ui
                    .editor
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
    }

    /// Extracts and removes a legacy `[mcps]` table from `config.toml`, validating all entries.
    pub fn take_legacy_mcps(&mut self) -> Result<Option<McpRegistry>, String> {
        let Some(mcps_val) = self.settings.remove("mcps") else {
            return Ok(None);
        };
        let mut table = toml::Table::new();
        table.insert("mcps".to_string(), mcps_val);
        let mut registry: McpRegistry = toml::Value::Table(table)
            .try_into()
            .map_err(|e| format!("Invalid legacy [mcps] table in config.toml: {e}"))?;
        registry.normalize_names();
        registry.normalize_descriptions();
        registry.validate()?;
        Ok(Some(registry))
    }
}

/// Normalizes an MCP server identifier by trimming outer whitespace and collapsing internal
/// whitespace sequences into a single hyphen (`'-'`).
pub fn normalize_mcp_server_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Validates that an MCP server identifier is non-empty and contains no whitespace characters.
pub fn validate_mcp_server_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("MCP server name cannot be empty.".to_string());
    }
    if name.chars().any(char::is_whitespace) {
        return Err("MCP server name cannot contain spaces or whitespace.".to_string());
    }
    Ok(())
}

/// Root structure of `~/.config/maga/mcp.toml` holding canonical MCP server definitions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRegistry {
    #[serde(default)]
    pub mcps: BTreeMap<String, McpServerConfig>,
}

impl McpRegistry {
    /// Normalizes any whitespace-containing server keys in `self.mcps` in-place and returns `true`
    /// if any key was modified. If both a spaced key and its normalized equivalent exist in the same
    /// map, the already-normalized key takes precedence.
    pub fn normalize_names(&mut self) -> bool {
        let had_spaced = self
            .mcps
            .keys()
            .any(|k| k.trim().is_empty() || k.chars().any(char::is_whitespace));
        if !had_spaced {
            return false;
        }
        let old = std::mem::take(&mut self.mcps);
        // First insert already-valid keys so they take precedence over legacy spaced duplicates
        let mut spaced = Vec::new();
        for (name, cfg) in old {
            let norm = normalize_mcp_server_name(&name);
            if norm == name {
                self.mcps.insert(name, cfg);
            } else {
                spaced.push((norm, cfg));
            }
        }
        for (norm, cfg) in spaced {
            if !norm.is_empty() {
                self.mcps.entry(norm).or_insert(cfg);
            }
        }
        true
    }

    pub fn normalize_descriptions(&mut self) {
        for server in self.mcps.values_mut() {
            server.normalize_description();
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, server) in &self.mcps {
            validate_mcp_server_name(name)?;
            server
                .validate()
                .map_err(|e| format!("Server '{name}': {e}"))?;
        }
        Ok(())
    }

    pub fn from_toml_str(input: &str) -> Result<Self, String> {
        let mut cfg: Self = toml::from_str(input).map_err(|e| format!("Invalid TOML: {e}"))?;
        cfg.normalize_names();
        cfg.normalize_descriptions();
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml_string(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize TOML: {e}"))
    }
}

/// Single-entry wrapper used when editing a single MCP server in `$EDITOR`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleServerSnippet {
    pub name: String,
    #[serde(flatten)]
    pub config: McpServerConfig,
}

impl SingleServerSnippet {
    pub fn to_editor_toml(&self) -> Result<String, String> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to format snippet as TOML: {e}"))?;
        Ok(format!(
            "# maga MCP Server Definition\n\
             # Save and quit to apply changes. Leave file empty or invalid to abort.\n\
             # Note: `name` must not contain spaces.\n\
             # Optional: description = \"Notes or comments about this MCP server\"\n\
             # transport = \"stdio\" (requires `command`) or \"http\" (requires `url`)\n\n\
             {body}"
        ))
    }

    pub fn from_editor_toml(input: &str) -> Result<Self, String> {
        let mut snippet: Self =
            toml::from_str(input).map_err(|e| format!("TOML syntax error: {e}"))?;
        validate_mcp_server_name(&snippet.name)?;
        snippet.config.normalize_description();
        snippet.config.validate()?;
        Ok(snippet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_mcp_registry_toml() {
        let mut cfg = McpRegistry::default();

        let mut http = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        http.headers
            .insert("CONTEXT7_API_KEY".to_string(), "secret".to_string());
        cfg.mcps.insert("context7".to_string(), http);

        let mut stdio =
            McpServerConfig::new_stdio("/usr/local/bin/sqlite-mcp", vec!["mcp".to_string()]);
        stdio.enabled = false;
        stdio.env.insert("APP_ENV".to_string(), "local".to_string());
        cfg.mcps.insert("sqlite-mcp".to_string(), stdio);

        let serialized = cfg.to_toml_string().expect("serialize toml");
        let parsed = McpRegistry::from_toml_str(&serialized).expect("parse toml");
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn round_trip_maga_config_and_extract_legacy_mcps() {
        let raw = r#"
theme = "minimal"

[mcps.context7]
enabled = true
transport = "http"
url = "https://mcp.context7.com/mcp"
"#;
        let mut cfg =
            MagaConfig::from_toml_str(raw).expect("parse general config with legacy mcps");
        let migrated = cfg
            .take_legacy_mcps()
            .expect("extract legacy mcps")
            .expect("legacy mcps present");
        assert!(migrated.mcps.contains_key("context7"));
        assert!(!cfg.settings.contains_key("mcps"));
        assert_eq!(
            cfg.settings.get("theme").and_then(|v| v.as_str()),
            Some("minimal")
        );

        let serialized = cfg.to_toml_string().expect("serialize general config");
        assert!(!serialized.contains("[mcps"));
        assert!(serialized.contains("[refresh]"));
        assert!(serialized.contains("[tools.antigravity]"));
        assert!(serialized.contains("[tools.claude]"));
        assert!(serialized.contains("[tools.copilot]"));
        assert!(serialized.contains("[tools.grok]"));
        assert!(serialized.contains("[tools.opencode]"));
        let reparsed = MagaConfig::from_toml_str(&serialized).expect("reparse general config");
        assert_eq!(cfg, reparsed);
    }

    #[test]
    fn parses_refresh_and_tools_config_with_defaults_and_overrides() {
        let default_cfg = MagaConfig::from_toml_str("").expect("parse empty config");
        assert_eq!(default_cfg.ui.theme, ThemeName::Blush);
        assert_eq!(default_cfg.ui.mascot_mode, MascotMode::Lite);
        assert!(!default_cfg.ui.background);
        assert!(default_cfg.ui.hero_mascot);
        assert!(!default_cfg.ui.hide_ui);
        assert!(default_cfg.ui.animations);
        assert!(default_cfg.refresh.enabled);
        assert_eq!(default_cfg.refresh.interval_secs, 5);
        for kind in HarnessKind::ALL {
            assert!(default_cfg.tools.is_enabled(kind));
            assert_eq!(
                default_cfg.tools.get(kind).config_path.as_deref(),
                Some(kind.default_config_path())
            );
        }

        let custom_raw = r#"
[ui]
theme = "indigo"
mascot_mode = "rich"
background = true
hero_mascot = false
animations = false

[refresh]
enabled = false
interval_secs = 15

[tools.antigravity]
enabled = true
config_path = "~/.gemini/custom_mcp.json"

[tools.claude]
enabled = false

[tools.grok]
enabled = false
config_path = "~/.grok/custom.toml"
"#;
        let custom_cfg = MagaConfig::from_toml_str(custom_raw).expect("parse custom config");
        assert_eq!(custom_cfg.ui.theme, ThemeName::Indigo);
        assert_eq!(custom_cfg.ui.mascot_mode, MascotMode::Rich);
        assert!(custom_cfg.ui.background);
        assert!(!custom_cfg.ui.hero_mascot);
        assert!(!custom_cfg.ui.animations);
        assert!(!custom_cfg.refresh.enabled);
        assert_eq!(custom_cfg.refresh.interval_secs, 15);
        assert!(custom_cfg.tools.antigravity.enabled);
        assert_eq!(
            custom_cfg.tools.antigravity.config_path.as_deref(),
            Some("~/.gemini/custom_mcp.json")
        );
        assert!(!custom_cfg.tools.claude.enabled);
        assert_eq!(
            custom_cfg.tools.claude.config_path.as_deref(),
            Some(DEFAULT_CLAUDE_CONFIG_PATH)
        );
        assert!(custom_cfg.tools.copilot.enabled);
        assert_eq!(
            custom_cfg.tools.copilot.config_path.as_deref(),
            Some(DEFAULT_COPILOT_CONFIG_PATH)
        );
        assert!(!custom_cfg.tools.grok.enabled);
        assert_eq!(
            custom_cfg.tools.grok.config_path.as_deref(),
            Some("~/.grok/custom.toml")
        );
        assert!(custom_cfg.tools.opencode.enabled);
        assert_eq!(
            custom_cfg.tools.opencode.config_path.as_deref(),
            Some(DEFAULT_OPENCODE_CONFIG_PATH)
        );
        assert_eq!(
            custom_cfg.tools.enabled_short_names(" + "),
            "Agy + Copilot + OpenCode"
        );
        assert_eq!(custom_cfg.tools.enabled_count_label(), "3 harnesses");
        assert_eq!(ToolsConfig::default().enabled_count_label(), "5 harnesses");
        let mut single_tools = ToolsConfig::default();
        for kind in [
            HarnessKind::Claude,
            HarnessKind::Copilot,
            HarnessKind::Grok,
            HarnessKind::OpenCode,
        ] {
            single_tools.get_mut(kind).enabled = false;
        }
        assert_eq!(single_tools.enabled_count_label(), "1 harness");
        single_tools.antigravity.enabled = false;
        assert_eq!(single_tools.enabled_count_label(), "no harnesses");
        assert!(!MagaConfig::has_all_default_keys(""));
        assert!(!MagaConfig::has_all_default_keys(custom_raw));

        let populated_toml = custom_cfg
            .to_toml_string()
            .expect("serialize populated config");
        assert!(populated_toml.contains("# maga General Configuration"));
        assert!(populated_toml.contains("[ui]"));
        assert!(populated_toml.contains("theme = \"indigo\""));
        assert!(populated_toml.contains("mascot_mode = \"rich\""));
        assert!(populated_toml.contains("background = true"));
        assert!(populated_toml.contains("hero_mascot = false"));
        assert!(populated_toml.contains("animations = false"));
        assert!(populated_toml.contains("config_path = \"~/.claude.json\""));
        assert!(populated_toml.contains("config_path = \"~/.copilot/mcp-config.json\""));
        assert!(populated_toml.contains("config_path = \"~/.config/opencode/opencode.json\""));
        assert!(MagaConfig::has_all_default_keys(&populated_toml));

        let reparsed = MagaConfig::from_toml_str(&populated_toml).expect("reparse populated");
        assert_eq!(reparsed.ui.theme, ThemeName::Indigo);
        assert_eq!(reparsed.ui.mascot_mode, MascotMode::Rich);
        assert!(reparsed.ui.background);
        assert!(!reparsed.ui.hero_mascot);
        assert!(!reparsed.ui.animations);
    }

    #[test]
    fn validates_required_transport_fields() {
        let invalid_stdio = McpServerConfig::new_stdio("   ", vec![]);
        assert!(invalid_stdio.validate().is_err());

        let invalid_http = McpServerConfig::new_http("");
        assert!(invalid_http.validate().is_err());
    }

    #[test]
    fn round_trip_single_server_editor_snippet() {
        let snippet = SingleServerSnippet {
            name: "filesystem".to_string(),
            config: McpServerConfig::new_stdio(
                "npx",
                vec!["-y".to_string(), "filesystem".to_string()],
            ),
        };
        let text = snippet.to_editor_toml().expect("render editor toml");
        let parsed = SingleServerSnippet::from_editor_toml(&text).expect("parse editor toml");
        assert_eq!(snippet, parsed);
    }

    #[test]
    fn normalizes_and_validates_mcp_server_names_without_spaces() {
        assert_eq!(normalize_mcp_server_name("Astro Docs"), "Astro-Docs");
        assert_eq!(
            normalize_mcp_server_name("  my   custom\tmcp  "),
            "my-custom-mcp"
        );
        assert!(validate_mcp_server_name("astro-docs").is_ok());
        assert!(validate_mcp_server_name("Astro-Docs_v2").is_ok());
        assert!(validate_mcp_server_name("").is_err());
        assert!(validate_mcp_server_name("   ").is_err());
        assert!(validate_mcp_server_name("Astro Docs").is_err());
        assert!(validate_mcp_server_name("astro\tdocs").is_err());

        // SingleServerSnippet rejects spaced names from $EDITOR
        let bad_editor = r#"
name = "Astro Docs"
enabled = true
transport = "http"
url = "https://mcp.docs.astro.build/mcp"
"#;
        let err = SingleServerSnippet::from_editor_toml(bad_editor).unwrap_err();
        assert!(err.contains("cannot contain spaces"));

        // McpRegistry::from_toml_str auto-normalizes spaced table keys on load
        let raw_registry = r#"
[mcps."Astro Docs"]
enabled = true
transport = "http"
url = "https://mcp.docs.astro.build/mcp"
"#;
        let reg = McpRegistry::from_toml_str(raw_registry).expect("auto-normalize mcp.toml keys");
        assert!(reg.mcps.contains_key("Astro-Docs"));
        assert!(!reg.mcps.contains_key("Astro Docs"));

        // Direct McpRegistry::validate rejects unnormalized spaced keys in memory
        let mut unnormalized = McpRegistry::default();
        unnormalized.mcps.insert(
            "Astro Docs".to_string(),
            McpServerConfig::new_http("https://mcp.docs.astro.build/mcp"),
        );
        assert!(unnormalized.validate().is_err());
    }

    #[test]
    fn normalizes_and_round_trips_optional_description_and_same_harness_state() {
        let mut s1 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        s1.description = Some("  Upstash Context7 docs server  ".to_string());
        s1.normalize_description();
        assert_eq!(
            s1.description.as_deref(),
            Some("Upstash Context7 docs server")
        );

        let s2 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        assert!(s1.same_definition(&s2));
        assert!(s1.same_harness_state(&s2));
        assert_ne!(s1, s2);

        let mut reg = McpRegistry::default();
        reg.mcps.insert("context7".to_string(), s1.clone());
        let mut blank_desc = McpServerConfig::new_stdio("npx", vec!["-y".to_string()]);
        blank_desc.description = Some("   ".to_string());
        reg.mcps.insert("blank-mcp".to_string(), blank_desc);

        let toml_text = reg.to_toml_string().expect("serialize mcp.toml");
        let parsed = McpRegistry::from_toml_str(&toml_text).expect("parse mcp.toml");
        assert_eq!(
            parsed.mcps["context7"].description.as_deref(),
            Some("Upstash Context7 docs server")
        );
        assert_eq!(parsed.mcps["blank-mcp"].description, None);

        let reserialized = parsed
            .to_toml_string()
            .expect("reserialize normalized mcp.toml");
        assert!(reserialized.contains("description = \"Upstash Context7 docs server\""));
        // blank-mcp should not emit a description key
        let blank_section = reserialized
            .split("[mcps.blank-mcp]")
            .nth(1)
            .unwrap()
            .split("[mcps.")
            .next()
            .unwrap();
        assert!(!blank_section.contains("description"));

        let snippet = SingleServerSnippet {
            name: "context7".to_string(),
            config: s1,
        };
        let editor_toml = snippet.to_editor_toml().expect("editor toml");
        assert!(editor_toml.contains("# Optional: description ="));
        assert!(editor_toml.contains("description = \"Upstash Context7 docs server\""));
        let parsed_snippet =
            SingleServerSnippet::from_editor_toml(&editor_toml).expect("parse editor toml");
        assert_eq!(parsed_snippet, snippet);
    }

    #[test]
    fn reads_configured_editor_from_top_level_or_ui_table() {
        let empty = MagaConfig::default();
        assert_eq!(empty.configured_editor(), None);

        let top_level = MagaConfig::from_toml_str("editor = \"nvim\"\n").expect("top-level editor");
        assert_eq!(top_level.configured_editor(), Some("nvim"));

        let ui_level =
            MagaConfig::from_toml_str("[ui]\neditor = \"code --wait\"\n").expect("ui editor");
        assert_eq!(ui_level.configured_editor(), Some("code --wait"));
    }
}
