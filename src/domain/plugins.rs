use crate::domain::model::{HarnessKind, MagaConfig};
use crate::domain::storage::{StoragePaths, write_atomic_with_backup};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml::{Table as TomlTable, Value as TomlValue};

/// Classification of a harness plugin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    #[default]
    Bundle,
    Npm,
    Local,
}

impl PluginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bundle => "plugin",
            Self::Npm => "npm",
            Self::Local => "local",
        }
    }
}

/// Discovered counts and itemized names of customizations bundled inside a plugin directory on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundledCounts {
    pub skills: usize,
    pub hooks: usize,
    pub mcps: usize,
    pub skill_names: Vec<String>,
    pub hook_names: Vec<String>,
    pub mcp_names: Vec<String>,
}

impl BundledCounts {
    pub fn is_empty(&self) -> bool {
        self.skills == 0 && self.hooks == 0 && self.mcps == 0
    }

    pub fn summary_badge(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if self.skills > 0 {
            let label = if self.skills == 1 { "skill" } else { "skills" };
            parts.push(format!("{} {label}", self.skills));
        }
        if self.hooks > 0 {
            let label = if self.hooks == 1 { "hook" } else { "hooks" };
            parts.push(format!("{} {label}", self.hooks));
        }
        if self.mcps > 0 {
            let label = if self.mcps == 1 { "mcp" } else { "mcps" };
            parts.push(format!("{} {label}", self.mcps));
        }
        Some(parts.join(", "))
    }
}

/// Discovered plugin belonging to a specific agentic harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessPlugin {
    pub harness: HarnessKind,
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub kind: PluginKind,
    pub source: Option<String>,
    pub version: Option<String>,
    pub install_path: Option<PathBuf>,
    pub config_path: PathBuf,
    pub bundled: BundledCounts,
}

impl HarnessPlugin {
    pub fn badge_type_label(&self) -> &'static str {
        self.kind.as_str()
    }
}

fn default_true() -> bool {
    true
}

/// Persisted entry for a single plugin inside `~/.config/maga/plugins.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginLedgerEntry {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PluginKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Root structure of `~/.config/maga/plugins.toml` storing per-harness plugin state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginsLedger {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub antigravity: BTreeMap<String, PluginLedgerEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub claude: BTreeMap<String, PluginLedgerEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub copilot: BTreeMap<String, PluginLedgerEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grok: BTreeMap<String, PluginLedgerEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub opencode: BTreeMap<String, PluginLedgerEntry>,
}

impl PluginsLedger {
    pub fn from_toml_str(input: &str) -> Result<Self, String> {
        if input.trim().is_empty() {
            return Ok(Self::default());
        }
        toml::from_str(input).map_err(|e| format!("Invalid plugins.toml: {e}"))
    }

    pub fn to_toml_string(&self) -> Result<String, String> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize plugins.toml: {e}"))?;
        let header = "\
# maga Per-Harness Plugin Ledger\n\
# Tracks discovered plugin states across Antigravity, Claude Code, Copilot, Grok, and OpenCode.\n\n";
        Ok(format!("{header}{body}"))
    }

    pub fn is_empty(&self) -> bool {
        self.antigravity.is_empty()
            && self.claude.is_empty()
            && self.copilot.is_empty()
            && self.grok.is_empty()
            && self.opencode.is_empty()
    }

    pub fn map_for(&self, kind: HarnessKind) -> &BTreeMap<String, PluginLedgerEntry> {
        match kind {
            HarnessKind::Antigravity => &self.antigravity,
            HarnessKind::Claude => &self.claude,
            HarnessKind::Copilot => &self.copilot,
            HarnessKind::Grok => &self.grok,
            HarnessKind::OpenCode => &self.opencode,
        }
    }

    pub fn map_for_mut(&mut self, kind: HarnessKind) -> &mut BTreeMap<String, PluginLedgerEntry> {
        match kind {
            HarnessKind::Antigravity => &mut self.antigravity,
            HarnessKind::Claude => &mut self.claude,
            HarnessKind::Copilot => &mut self.copilot,
            HarnessKind::Grok => &mut self.grok,
            HarnessKind::OpenCode => &mut self.opencode,
        }
    }

    pub fn record_plugin(&mut self, plugin: &HarnessPlugin) {
        self.map_for_mut(plugin.harness).insert(
            plugin.id.clone(),
            PluginLedgerEntry {
                enabled: plugin.enabled,
                kind: Some(plugin.kind),
                source: plugin.source.clone(),
                version: plugin.version.clone(),
            },
        );
    }

    pub fn sync_from_discovered(
        &mut self,
        discovered: &BTreeMap<HarnessKind, Vec<HarnessPlugin>>,
    ) -> bool {
        let before = self.clone();
        for (kind, plugins) in discovered {
            if *kind != HarnessKind::OpenCode {
                let active_ids: BTreeSet<&str> = plugins.iter().map(|p| p.id.as_str()).collect();
                self.map_for_mut(*kind)
                    .retain(|id, _| active_ids.contains(id.as_str()));
            }
            for p in plugins {
                self.record_plugin(p);
            }
        }
        *self != before
    }
}

/// Extracts an optional non-empty `"description"` field from `plugin.json`, `.claude-plugin/plugin.json`, or `package.json`.
pub fn extract_manifest_description(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return None;
    }
    for candidate in [
        dir.join("plugin.json"),
        dir.join(".claude-plugin").join("plugin.json"),
        dir.join("package.json"),
    ] {
        if candidate.is_file()
            && let Ok(text) = fs::read_to_string(&candidate)
            && let Ok(val) = serde_json::from_str::<JsonValue>(&text)
            && let Some(desc) = val.get("description").and_then(JsonValue::as_str)
        {
            let trimmed = desc.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Extracts an optional non-empty `"name"` field from `plugin.json` or `.claude-plugin/plugin.json`.
pub fn extract_manifest_name(dir: &Path) -> Option<String> {
    if !dir.is_dir() {
        return None;
    }
    for candidate in [
        dir.join("plugin.json"),
        dir.join(".claude-plugin").join("plugin.json"),
    ] {
        if candidate.is_file()
            && let Ok(text) = fs::read_to_string(&candidate)
            && let Ok(val) = serde_json::from_str::<JsonValue>(&text)
            && let Some(name) = val.get("name").and_then(JsonValue::as_str)
        {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Best-effort Grok registry fallback for a human-readable name when the install dir lacks a manifest `name`.
fn extract_grok_registry_display_name(installed_dir: &Path, id: &str) -> Option<String> {
    let registry_path = installed_dir.join("registry.json");
    if !registry_path.is_file() {
        return None;
    }
    let text = fs::read_to_string(&registry_path).ok()?;
    let val: JsonValue = serde_json::from_str(&text).ok()?;
    let entry = val.get("repos")?.as_object()?.get(id)?;

    if let Some(name) = entry
        .get("marketplace")
        .and_then(|m| m.get("source_display_name"))
        .and_then(JsonValue::as_str)
    {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(plugins) = entry.get("plugins").and_then(JsonValue::as_object)
        && plugins.len() == 1
    {
        return plugins.keys().next().cloned();
    }

    None
}

fn plugin_path_basename(id: &str) -> String {
    id.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(id)
        .to_string()
}

/// Scans an installed plugin directory to collect bundled skill, hook, and MCP server names and counts.
pub fn inspect_bundled_contents(dir: &Path) -> BundledCounts {
    if !dir.is_dir() {
        return BundledCounts::default();
    }
    let mut skill_set = BTreeSet::new();
    let mut hook_set = BTreeSet::new();
    let mut mcp_set = BTreeSet::new();

    // Collect skills: subdirectories in `skills/` or `.agents/skills/`
    for skills_root in [dir.join("skills"), dir.join(".agents").join("skills")] {
        if let Ok(entries) = fs::read_dir(&skills_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir()
                    && let Some(name) = entry.file_name().to_str()
                    && !name.starts_with('.')
                {
                    skill_set.insert(name.to_string());
                }
            }
        }
    }

    // Collect hooks: `hooks.json`, `hooks/hooks.json`, or files inside `hooks/`
    let root_hooks = dir.join("hooks.json");
    let nested_hooks = dir.join("hooks").join("hooks.json");
    if root_hooks.is_file() {
        for h in collect_hooks_in_json(&root_hooks) {
            hook_set.insert(h);
        }
    } else if nested_hooks.is_file() {
        for h in collect_hooks_in_json(&nested_hooks) {
            hook_set.insert(h);
        }
    } else if dir.join("hooks").is_dir()
        && let Ok(entries) = fs::read_dir(dir.join("hooks"))
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && let Some(name) = p.file_stem().and_then(|s| s.to_str())
                && !name.starts_with('.')
            {
                hook_set.insert(name.to_string());
            }
        }
    }

    // Collect MCP servers: `mcp_config.json`, `mcp.json`, `.mcp.json`, or `plugin.json` inline `mcpServers`
    for mcp_file in [
        dir.join("mcp_config.json"),
        dir.join("mcp.json"),
        dir.join(".mcp.json"),
        dir.join("plugin.json"),
        dir.join(".claude-plugin").join("plugin.json"),
    ] {
        if mcp_file.is_file() {
            for m in collect_mcps_in_json(&mcp_file) {
                mcp_set.insert(m);
            }
        }
    }

    let skill_names: Vec<String> = skill_set.into_iter().collect();
    let hook_names: Vec<String> = hook_set.into_iter().collect();
    let mcp_names: Vec<String> = mcp_set.into_iter().collect();

    BundledCounts {
        skills: skill_names.len(),
        hooks: hook_names.len(),
        mcps: mcp_names.len(),
        skill_names,
        hook_names,
        mcp_names,
    }
}

fn collect_hooks_in_json(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return vec!["hooks.json".to_string()];
    };
    let Ok(val) = serde_json::from_str::<JsonValue>(&text) else {
        return vec!["hooks.json".to_string()];
    };
    if let Some(obj) = val.get("hooks").and_then(JsonValue::as_object) {
        if !obj.is_empty() {
            return obj.keys().cloned().collect();
        }
        return vec!["hooks.json".to_string()];
    }
    if let Some(obj) = val.as_object() {
        if !obj.is_empty() {
            return obj.keys().cloned().collect();
        }
        return vec!["hooks.json".to_string()];
    }
    vec!["hooks.json".to_string()]
}

fn collect_mcps_in_json(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<JsonValue>(&text) else {
        return Vec::new();
    };
    if let Some(obj) = val.get("mcpServers").and_then(JsonValue::as_object) {
        return obj.keys().cloned().collect();
    }
    if let Some(obj) = val.get("mcp").and_then(JsonValue::as_object) {
        return obj.keys().cloned().collect();
    }
    Vec::new()
}

/// Discovers plugins across all enabled harnesses (`tools.<harness>.enabled == true`),
/// updating `ledger` with newly discovered entries.
pub fn discover_all_harness_plugins(
    paths: &StoragePaths,
    config: &MagaConfig,
    ledger: &mut PluginsLedger,
) -> BTreeMap<HarnessKind, Vec<HarnessPlugin>> {
    let mut out = BTreeMap::new();
    for kind in HarnessKind::ALL {
        if !config.tools.is_enabled(kind) {
            continue;
        }
        let list = match kind {
            HarnessKind::Antigravity => discover_agy_plugins(paths, config),
            HarnessKind::Claude => discover_claude_plugins(paths, config),
            HarnessKind::Copilot => discover_copilot_plugins(paths, config),
            HarnessKind::Grok => discover_grok_plugins(paths, config),
            HarnessKind::OpenCode => discover_opencode_plugins(paths, config, ledger),
        };
        out.insert(kind, list);
    }
    ledger.sync_from_discovered(&out);
    out
}

/// Toggles `plugin.enabled` on disk in the owning harness's native config file and updates `ledger` + `plugins.toml`.
pub fn toggle_harness_plugin(
    paths: &StoragePaths,
    config: &MagaConfig,
    ledger: &mut PluginsLedger,
    plugin: &mut HarnessPlugin,
) -> Result<(), String> {
    let new_enabled = !plugin.enabled;
    match plugin.harness {
        HarnessKind::Antigravity => {
            let cfg_path = paths.effective_agy_plugin_config_path(config);
            set_agy_plugin_enabled(&cfg_path, &plugin.id, new_enabled)?;
        }
        HarnessKind::Claude => {
            let settings_path = paths.effective_claude_settings_path(config);
            set_claude_plugin_enabled(&settings_path, &plugin.id, new_enabled)?;
        }
        HarnessKind::Copilot => {
            let cfg_path = paths.effective_copilot_plugin_config_path(config);
            set_copilot_plugin_enabled(&cfg_path, &plugin.id, new_enabled)?;
        }
        HarnessKind::Grok => {
            let grok_path = paths.effective_grok_path(config);
            set_grok_plugin_enabled(&grok_path, &plugin.id, new_enabled)?;
        }
        HarnessKind::OpenCode => {
            let opencode_path = paths.effective_opencode_path(config);
            set_opencode_plugin_enabled(&opencode_path, &plugin.id, plugin.kind, new_enabled)?;
        }
    }

    plugin.enabled = new_enabled;
    ledger.record_plugin(plugin);
    let ledger_str = ledger.to_toml_string()?;
    write_atomic_with_backup(&paths.plugins_toml, &ledger_str)?;
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("Failed to remove plugin directory {}: {e}", path.display()))?;
    }
    Ok(())
}

fn prune_empty_ancestors(mut dir: Option<&Path>, stop_at: &Path) {
    while let Some(curr) = dir {
        if curr == stop_at || !curr.starts_with(stop_at) {
            break;
        }
        let is_empty = fs::read_dir(curr)
            .map(|mut it| it.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(curr);
            dir = curr.parent();
        } else {
            break;
        }
    }
}

/// Removes a plugin entry from the owning harness's native configuration/registry files,
/// deletes its installed directory on disk, and removes it from `plugins.toml`.
pub fn remove_harness_plugin(
    paths: &StoragePaths,
    config: &MagaConfig,
    ledger: &mut PluginsLedger,
    harness: HarnessKind,
    id: &str,
    install_path: Option<&Path>,
) -> Result<(), String> {
    match harness {
        HarnessKind::Antigravity => {
            let cfg_path = paths.effective_agy_plugin_config_path(config);
            if cfg_path.is_file() {
                let text = fs::read_to_string(&cfg_path)
                    .map_err(|e| format!("Failed to read {}: {e}", cfg_path.display()))?;
                if !text.trim().is_empty() {
                    let mut root: JsonMap<String, JsonValue> = serde_json::from_str(&text)
                        .map_err(|e| format!("Invalid Antigravity config.json: {e}"))?;
                    if let Some(plugins_obj) =
                        root.get_mut("plugins").and_then(JsonValue::as_object_mut)
                    {
                        plugins_obj.remove(id);
                    }
                    let serialized = serde_json::to_string_pretty(&root)
                        .map_err(|e| format!("Failed to serialize Antigravity config.json: {e}"))?;
                    write_atomic_with_backup(&cfg_path, &serialized)?;
                }
            }
            if let Some(dir) = install_path {
                remove_dir_if_exists(dir)?;
            }
            let agy_plugin_dir = paths.effective_agy_plugins_dir(config).join(id);
            remove_dir_if_exists(&agy_plugin_dir)?;
        }
        HarnessKind::Claude => {
            let settings_path = paths.effective_claude_settings_path(config);
            if settings_path.is_file() {
                let text = fs::read_to_string(&settings_path)
                    .map_err(|e| format!("Failed to read {}: {e}", settings_path.display()))?;
                if !text.trim().is_empty() {
                    let mut root: JsonMap<String, JsonValue> = serde_json::from_str(&text)
                        .map_err(|e| format!("Invalid Claude settings.json: {e}"))?;
                    if let Some(en_obj) = root
                        .get_mut("enabledPlugins")
                        .and_then(JsonValue::as_object_mut)
                    {
                        en_obj.remove(id);
                    }
                    let serialized = serde_json::to_string_pretty(&root)
                        .map_err(|e| format!("Failed to serialize Claude settings.json: {e}"))?;
                    write_atomic_with_backup(&settings_path, &serialized)?;
                }
            }

            let installed_path = paths.effective_claude_installed_plugins_path(config);
            let mut recorded_install_paths = Vec::new();
            if installed_path.is_file() {
                let text = fs::read_to_string(&installed_path)
                    .map_err(|e| format!("Failed to read {}: {e}", installed_path.display()))?;
                if !text.trim().is_empty() {
                    let mut root: JsonMap<String, JsonValue> = serde_json::from_str(&text)
                        .map_err(|e| format!("Invalid Claude installed_plugins.json: {e}"))?;
                    if let Some(plugins_obj) =
                        root.get_mut("plugins").and_then(JsonValue::as_object_mut)
                        && let Some(removed) = plugins_obj.remove(id)
                    {
                        if let Some(arr) = removed.as_array() {
                            for item in arr {
                                if let Some(p) = item.get("installPath").and_then(JsonValue::as_str)
                                {
                                    recorded_install_paths.push(PathBuf::from(p));
                                }
                            }
                        } else if let Some(p) =
                            removed.get("installPath").and_then(JsonValue::as_str)
                        {
                            recorded_install_paths.push(PathBuf::from(p));
                        }
                    }
                    let serialized = serde_json::to_string_pretty(&root).map_err(|e| {
                        format!("Failed to serialize Claude installed_plugins.json: {e}")
                    })?;
                    write_atomic_with_backup(&installed_path, &serialized)?;
                }
            }

            let claude_plugins_root = installed_path.parent().map(Path::to_path_buf);
            if let Some(dir) = install_path {
                let parent = dir.parent().map(Path::to_path_buf);
                remove_dir_if_exists(dir)?;
                if let Some(ref stop) = claude_plugins_root {
                    prune_empty_ancestors(parent.as_deref(), stop);
                }
            }
            for dir in recorded_install_paths {
                let parent = dir.parent().map(Path::to_path_buf);
                remove_dir_if_exists(&dir)?;
                if let Some(ref stop) = claude_plugins_root {
                    prune_empty_ancestors(parent.as_deref(), stop);
                }
            }
        }
        HarnessKind::Copilot => {
            let cfg_path = paths.effective_copilot_plugin_config_path(config);
            let mut recorded_cache_paths = Vec::new();
            if cfg_path.is_file() {
                let text = fs::read_to_string(&cfg_path)
                    .map_err(|e| format!("Failed to read {}: {e}", cfg_path.display()))?;
                if !text.trim().is_empty() {
                    let mut root: JsonMap<String, JsonValue> = serde_json::from_str(&text)
                        .map_err(|e| format!("Invalid Copilot config.json: {e}"))?;
                    if let Some(arr) = root
                        .get_mut("installedPlugins")
                        .and_then(JsonValue::as_array_mut)
                    {
                        for item in arr.iter() {
                            if item.get("name").and_then(JsonValue::as_str) == Some(id)
                                && let Some(cp) = item.get("cache_path").and_then(JsonValue::as_str)
                            {
                                recorded_cache_paths.push(PathBuf::from(cp));
                            }
                        }
                        arr.retain(|item| item.get("name").and_then(JsonValue::as_str) != Some(id));
                    }
                    let serialized = serde_json::to_string_pretty(&root)
                        .map_err(|e| format!("Failed to serialize Copilot config.json: {e}"))?;
                    write_atomic_with_backup(&cfg_path, &serialized)?;
                }
            }

            let installed_dir = paths.effective_copilot_installed_plugins_dir(config);
            if let Some(dir) = install_path {
                let parent = dir.parent().map(Path::to_path_buf);
                remove_dir_if_exists(dir)?;
                prune_empty_ancestors(parent.as_deref(), &installed_dir);
            }
            for dir in recorded_cache_paths {
                let parent = dir.parent().map(Path::to_path_buf);
                remove_dir_if_exists(&dir)?;
                prune_empty_ancestors(parent.as_deref(), &installed_dir);
            }
            remove_dir_if_exists(&installed_dir.join(id))?;
        }
        HarnessKind::Grok => {
            let grok_path = paths.effective_grok_path(config);
            if grok_path.is_file() {
                let text = fs::read_to_string(&grok_path)
                    .map_err(|e| format!("Failed to read {}: {e}", grok_path.display()))?;
                if !text.trim().is_empty() {
                    let mut root: TomlTable = toml::from_str(&text)
                        .map_err(|e| format!("Invalid Grok config.toml: {e}"))?;
                    if let Some(plugins_tbl) =
                        root.get_mut("plugins").and_then(TomlValue::as_table_mut)
                    {
                        for key in ["disabled", "paths"] {
                            if let Some(arr) =
                                plugins_tbl.get_mut(key).and_then(TomlValue::as_array_mut)
                            {
                                arr.retain(|v| v.as_str() != Some(id));
                            }
                        }
                    }
                    let serialized = toml::to_string_pretty(&root)
                        .map_err(|e| format!("Failed to serialize Grok config.toml: {e}"))?;
                    write_atomic_with_backup(&grok_path, &serialized)?;
                }
            }
            if let Some(dir) = install_path {
                remove_dir_if_exists(dir)?;
            }
            let grok_plugin_dir = paths.effective_grok_installed_plugins_dir(config).join(id);
            remove_dir_if_exists(&grok_plugin_dir)?;
        }
        HarnessKind::OpenCode => {
            let opencode_path = paths.effective_opencode_path(config);
            if opencode_path.is_file() {
                let text = fs::read_to_string(&opencode_path)
                    .map_err(|e| format!("Failed to read {}: {e}", opencode_path.display()))?;
                if !text.trim().is_empty() {
                    let mut root: JsonMap<String, JsonValue> = serde_json::from_str(&text)
                        .map_err(|e| format!("Invalid OpenCode opencode.json: {e}"))?;
                    let neg_id = format!("-{id}");
                    for key in ["plugin", "plugins"] {
                        if let Some(arr) = root.get_mut(key).and_then(JsonValue::as_array_mut) {
                            arr.retain(|v| {
                                v.as_str() != Some(id)
                                    && v.as_str() != Some(&neg_id)
                                    && v.get("package").and_then(JsonValue::as_str) != Some(id)
                            });
                        }
                    }
                    let serialized = serde_json::to_string_pretty(&root)
                        .map_err(|e| format!("Failed to serialize OpenCode opencode.json: {e}"))?;
                    write_atomic_with_backup(&opencode_path, &serialized)?;
                }
            }
            if let Some(dir) = install_path {
                remove_dir_if_exists(dir)?;
            }
            let oc_plugin_dir = paths.effective_opencode_plugins_dir(config).join(id);
            remove_dir_if_exists(&oc_plugin_dir)?;
        }
    }

    ledger.map_for_mut(harness).remove(id);
    let ledger_str = ledger.to_toml_string()?;
    write_atomic_with_backup(&paths.plugins_toml, &ledger_str)?;
    Ok(())
}

// ============================================================================
// 1. Antigravity (`~/.gemini/config/config.json` + `~/.gemini/config/plugins/`)
// ============================================================================

pub fn discover_agy_plugins(paths: &StoragePaths, config: &MagaConfig) -> Vec<HarnessPlugin> {
    let cfg_path = paths.effective_agy_plugin_config_path(config);
    let plugins_dir = paths.effective_agy_plugins_dir(config);

    let mut cfg_states: BTreeMap<String, bool> = BTreeMap::new();
    if cfg_path.is_file()
        && let Ok(text) = fs::read_to_string(&cfg_path)
        && let Ok(root) = serde_json::from_str::<JsonValue>(&text)
        && let Some(plugins_obj) = root.get("plugins").and_then(JsonValue::as_object)
    {
        for (name, val) in plugins_obj {
            let enabled = val
                .get("enabled")
                .and_then(JsonValue::as_bool)
                .or_else(|| val.as_bool())
                .unwrap_or(true);
            cfg_states.insert(name.clone(), enabled);
        }
    }

    let mut all_ids: BTreeSet<String> = cfg_states.keys().cloned().collect();
    if plugins_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&plugins_dir)
    {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
                && !name.starts_with('.')
            {
                all_ids.insert(name.to_string());
            }
        }
    }

    let mut out = Vec::new();
    for id in all_ids {
        let dir_path = plugins_dir.join(&id);
        let install_path = dir_path.is_dir().then_some(dir_path.clone());
        let display_name = extract_manifest_name(&dir_path).unwrap_or_else(|| id.clone());
        let mut default_enabled = true;
        let mut version = None;
        let mut source = Some("local".to_string());

        let manifest_path = dir_path.join("plugin.json");
        if manifest_path.is_file()
            && let Ok(text) = fs::read_to_string(&manifest_path)
            && let Ok(manifest) = serde_json::from_str::<JsonValue>(&text)
        {
            if manifest.get("disabled").and_then(JsonValue::as_bool) == Some(true) {
                default_enabled = false;
            }
            if let Some(v) = manifest.get("version").and_then(JsonValue::as_str) {
                version = Some(v.to_string());
            }
            if let Some(repo) = manifest.get("repository").and_then(JsonValue::as_str) {
                source = Some(repo.to_string());
            }
        } else if install_path.is_none() {
            source = Some("config.json".to_string());
        }

        let enabled = cfg_states.get(&id).copied().unwrap_or(default_enabled);
        let description = install_path
            .as_deref()
            .and_then(extract_manifest_description);
        let bundled = install_path
            .as_deref()
            .map(inspect_bundled_contents)
            .unwrap_or_default();

        out.push(HarnessPlugin {
            harness: HarnessKind::Antigravity,
            id,
            display_name,
            description,
            enabled,
            kind: PluginKind::Bundle,
            source,
            version,
            install_path,
            config_path: cfg_path.clone(),
            bundled,
        });
    }
    out
}

pub fn set_agy_plugin_enabled(cfg_path: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let mut root: JsonMap<String, JsonValue> = if cfg_path.is_file() {
        let text = fs::read_to_string(cfg_path)
            .map_err(|e| format!("Failed to read {}: {e}", cfg_path.display()))?;
        if text.trim().is_empty() {
            JsonMap::new()
        } else {
            serde_json::from_str(&text)
                .map_err(|e| format!("Invalid Antigravity config.json: {e}"))?
        }
    } else {
        JsonMap::new()
    };

    let plugins_entry = root
        .entry("plugins".to_string())
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if !plugins_entry.is_object() {
        *plugins_entry = JsonValue::Object(JsonMap::new());
    }
    let plugins_map = plugins_entry.as_object_mut().unwrap();
    let plugin_obj = plugins_map
        .entry(id.to_string())
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if let Some(obj) = plugin_obj.as_object_mut() {
        obj.insert("enabled".to_string(), json!(enabled));
    } else {
        *plugin_obj = json!({ "enabled": enabled });
    }

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize Antigravity config.json: {e}"))?;
    write_atomic_with_backup(cfg_path, &serialized)
}

// ============================================================================
// 2. Claude Code (`~/.claude/plugins/installed_plugins.json` + `~/.claude/settings.json`)
// ============================================================================

#[derive(Debug, Default, Clone)]
struct ClaudeInstalledMeta {
    version: Option<String>,
    install_path: Option<PathBuf>,
}

pub fn discover_claude_plugins(paths: &StoragePaths, config: &MagaConfig) -> Vec<HarnessPlugin> {
    let settings_path = paths.effective_claude_settings_path(config);
    let installed_path = paths.effective_claude_installed_plugins_path(config);

    let mut installed_map: BTreeMap<String, ClaudeInstalledMeta> = BTreeMap::new();
    if installed_path.is_file()
        && let Ok(text) = fs::read_to_string(&installed_path)
        && let Ok(root) = serde_json::from_str::<JsonValue>(&text)
        && let Some(plugins_obj) = root.get("plugins").and_then(JsonValue::as_object)
    {
        for (id, val) in plugins_obj {
            let entry_obj = val
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(JsonValue::as_object)
                .or_else(|| val.as_object());
            let mut meta = ClaudeInstalledMeta::default();
            if let Some(obj) = entry_obj {
                meta.version = obj
                    .get("version")
                    .and_then(JsonValue::as_str)
                    .map(ToString::to_string);
                meta.install_path = obj
                    .get("installPath")
                    .and_then(JsonValue::as_str)
                    .map(PathBuf::from);
            }
            installed_map.insert(id.clone(), meta);
        }
    }

    let mut enabled_map: BTreeMap<String, bool> = BTreeMap::new();
    if settings_path.is_file()
        && let Ok(text) = fs::read_to_string(&settings_path)
        && let Ok(root) = serde_json::from_str::<JsonValue>(&text)
        && let Some(en_obj) = root.get("enabledPlugins").and_then(JsonValue::as_object)
    {
        for (id, val) in en_obj {
            if let Some(b) = val.as_bool() {
                enabled_map.insert(id.clone(), b);
            }
        }
    }

    let mut all_ids: BTreeSet<String> = installed_map.keys().cloned().collect();
    all_ids.extend(enabled_map.keys().cloned());

    let mut out = Vec::new();
    for id in all_ids {
        let (short_name, marketplace) = match id.split_once('@') {
            Some((name, mkt)) => (name.to_string(), Some(mkt.to_string())),
            None => (id.clone(), None),
        };
        let meta = installed_map.get(&id).cloned().unwrap_or_default();
        // In Claude Code, if a plugin is listed in enabledPlugins, that boolean wins;
        // otherwise if it's only in installed_plugins.json and omitted from enabledPlugins, it is disabled (false) when enabledPlugins exists, or enabled by default if enabledPlugins is empty.
        let enabled = enabled_map
            .get(&id)
            .copied()
            .unwrap_or(enabled_map.is_empty());

        let description = meta
            .install_path
            .as_deref()
            .and_then(extract_manifest_description);
        let bundled = meta
            .install_path
            .as_deref()
            .map(inspect_bundled_contents)
            .unwrap_or_default();

        out.push(HarnessPlugin {
            harness: HarnessKind::Claude,
            id,
            display_name: short_name,
            description,
            enabled,
            kind: PluginKind::Bundle,
            source: marketplace,
            version: meta.version,
            install_path: meta.install_path,
            config_path: settings_path.clone(),
            bundled,
        });
    }
    out
}

pub fn set_claude_plugin_enabled(
    settings_path: &Path,
    id: &str,
    enabled: bool,
) -> Result<(), String> {
    let mut root: JsonMap<String, JsonValue> = if settings_path.exists() {
        let text = fs::read_to_string(settings_path)
            .map_err(|e| format!("Failed to read {}: {e}", settings_path.display()))?;
        if text.trim().is_empty() {
            JsonMap::new()
        } else {
            serde_json::from_str(&text).map_err(|e| format!("Invalid Claude settings.json: {e}"))?
        }
    } else {
        JsonMap::new()
    };

    let en_entry = root
        .entry("enabledPlugins".to_string())
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    if !en_entry.is_object() {
        *en_entry = JsonValue::Object(JsonMap::new());
    }
    en_entry
        .as_object_mut()
        .unwrap()
        .insert(id.to_string(), json!(enabled));

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize Claude settings.json: {e}"))?;
    write_atomic_with_backup(settings_path, &serialized)
}

// ============================================================================
// 3. GitHub Copilot CLI (`~/.copilot/config.json` + `~/.copilot/installed-plugins/`)
// ============================================================================

pub fn discover_copilot_plugins(paths: &StoragePaths, config: &MagaConfig) -> Vec<HarnessPlugin> {
    let cfg_path = paths.effective_copilot_plugin_config_path(config);
    let installed_dir = paths.effective_copilot_installed_plugins_dir(config);

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    if cfg_path.is_file()
        && let Ok(text) = fs::read_to_string(&cfg_path)
        && let Ok(root) = serde_json::from_str::<JsonValue>(&text)
        && let Some(arr) = root.get("installedPlugins").and_then(JsonValue::as_array)
    {
        for item in arr {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(name) = obj.get("name").and_then(JsonValue::as_str) else {
                continue;
            };
            seen.insert(name.to_string());
            let enabled = obj
                .get("enabled")
                .and_then(JsonValue::as_bool)
                .unwrap_or(true);
            let source = obj
                .get("marketplace")
                .and_then(JsonValue::as_str)
                .map(ToString::to_string);
            let version = obj
                .get("source_sha")
                .and_then(JsonValue::as_str)
                .map(|sha| sha.chars().take(12).collect());
            let install_path = obj
                .get("cache_path")
                .and_then(JsonValue::as_str)
                .map(PathBuf::from);
            let description = install_path
                .as_deref()
                .and_then(extract_manifest_description);
            let bundled = install_path
                .as_deref()
                .map(inspect_bundled_contents)
                .unwrap_or_default();

            out.push(HarnessPlugin {
                harness: HarnessKind::Copilot,
                id: name.to_string(),
                display_name: name.to_string(),
                description,
                enabled,
                kind: PluginKind::Bundle,
                source,
                version,
                install_path,
                config_path: cfg_path.clone(),
                bundled,
            });
        }
    }

    if installed_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&installed_dir)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(name) = entry.file_name().to_str()
                && !name.starts_with('.')
                && !seen.contains(name)
            {
                let nested = p.join(name);
                let target_dir = if nested.is_dir() { nested } else { p };
                let description = extract_manifest_description(&target_dir);
                let bundled = inspect_bundled_contents(&target_dir);
                out.push(HarnessPlugin {
                    harness: HarnessKind::Copilot,
                    id: name.to_string(),
                    display_name: name.to_string(),
                    description,
                    enabled: true,
                    kind: PluginKind::Bundle,
                    source: Some("local".to_string()),
                    version: None,
                    install_path: Some(target_dir),
                    config_path: cfg_path.clone(),
                    bundled,
                });
            }
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn set_copilot_plugin_enabled(cfg_path: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let mut root: JsonMap<String, JsonValue> = if cfg_path.is_file() {
        let text = fs::read_to_string(cfg_path)
            .map_err(|e| format!("Failed to read {}: {e}", cfg_path.display()))?;
        if text.trim().is_empty() {
            JsonMap::new()
        } else {
            serde_json::from_str(&text).map_err(|e| format!("Invalid Copilot config.json: {e}"))?
        }
    } else {
        JsonMap::new()
    };

    let arr_entry = root
        .entry("installedPlugins".to_string())
        .or_insert_with(|| JsonValue::Array(Vec::new()));
    if !arr_entry.is_array() {
        *arr_entry = JsonValue::Array(Vec::new());
    }
    let arr = arr_entry.as_array_mut().unwrap();
    let mut found = false;
    for item in arr.iter_mut() {
        if let Some(obj) = item.as_object_mut()
            && obj.get("name").and_then(JsonValue::as_str) == Some(id)
        {
            obj.insert("enabled".to_string(), json!(enabled));
            found = true;
        }
    }
    if !found {
        arr.push(json!({
            "name": id,
            "enabled": enabled,
        }));
    }

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize Copilot config.json: {e}"))?;
    write_atomic_with_backup(cfg_path, &serialized)
}

// ============================================================================
// 4. Grok CLI (`~/.grok/config.toml` + `~/.grok/installed-plugins/`)
// ============================================================================

pub fn discover_grok_plugins(paths: &StoragePaths, config: &MagaConfig) -> Vec<HarnessPlugin> {
    let grok_path = paths.effective_grok_path(config);
    let installed_dir = paths.effective_grok_installed_plugins_dir(config);

    let mut disabled_set = BTreeSet::new();
    let mut path_ids = Vec::new();

    if grok_path.is_file()
        && let Ok(text) = fs::read_to_string(&grok_path)
        && let Ok(root) = toml::from_str::<TomlTable>(&text)
        && let Some(plugins_tbl) = root.get("plugins").and_then(TomlValue::as_table)
    {
        if let Some(disabled_arr) = plugins_tbl.get("disabled").and_then(TomlValue::as_array) {
            for v in disabled_arr {
                if let Some(s) = v.as_str() {
                    disabled_set.insert(s.to_string());
                }
            }
        }
        if let Some(paths_arr) = plugins_tbl.get("paths").and_then(TomlValue::as_array) {
            for v in paths_arr {
                if let Some(s) = v.as_str() {
                    path_ids.push(s.to_string());
                }
            }
        }
    }

    let mut all_ids: BTreeSet<String> = disabled_set.iter().cloned().collect();
    all_ids.extend(path_ids.iter().cloned());

    if installed_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&installed_dir)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(name) = entry.file_name().to_str()
                && !name.starts_with('.')
            {
                all_ids.insert(name.to_string());
            }
        }
    }

    let mut out = Vec::new();
    for id in all_ids {
        let candidate_dir = installed_dir.join(&id);
        let install_path = if candidate_dir.is_dir() {
            Some(candidate_dir)
        } else {
            let expanded = crate::domain::storage::expand_tilde(&id);
            expanded.is_dir().then_some(expanded)
        };
        let description = install_path
            .as_deref()
            .and_then(extract_manifest_description);
        let bundled = install_path
            .as_deref()
            .map(inspect_bundled_contents)
            .unwrap_or_default();
        let display_name = install_path
            .as_deref()
            .and_then(extract_manifest_name)
            .or_else(|| extract_grok_registry_display_name(&installed_dir, &id))
            .unwrap_or_else(|| plugin_path_basename(&id));
        let enabled = !disabled_set.contains(&id);

        out.push(HarnessPlugin {
            harness: HarnessKind::Grok,
            id,
            display_name,
            description,
            enabled,
            kind: PluginKind::Bundle,
            source: Some("grok".to_string()),
            version: None,
            install_path,
            config_path: grok_path.clone(),
            bundled,
        });
    }

    out
}

pub fn set_grok_plugin_enabled(grok_path: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let mut root: TomlTable = if grok_path.is_file() {
        let text = fs::read_to_string(grok_path)
            .map_err(|e| format!("Failed to read {}: {e}", grok_path.display()))?;
        if text.trim().is_empty() {
            TomlTable::new()
        } else {
            toml::from_str(&text).map_err(|e| format!("Invalid Grok config.toml: {e}"))?
        }
    } else {
        TomlTable::new()
    };

    let plugins_val = root
        .entry("plugins".to_string())
        .or_insert_with(|| TomlValue::Table(TomlTable::new()));
    if !plugins_val.is_table() {
        *plugins_val = TomlValue::Table(TomlTable::new());
    }
    let plugins_tbl = plugins_val.as_table_mut().unwrap();
    let disabled_val = plugins_tbl
        .entry("disabled".to_string())
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    if !disabled_val.is_array() {
        *disabled_val = TomlValue::Array(Vec::new());
    }
    let disabled_arr = disabled_val.as_array_mut().unwrap();

    if enabled {
        disabled_arr.retain(|v| v.as_str() != Some(id));
    } else if !disabled_arr.iter().any(|v| v.as_str() == Some(id)) {
        disabled_arr.push(TomlValue::String(id.to_string()));
    }

    let serialized = toml::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize Grok config.toml: {e}"))?;
    write_atomic_with_backup(grok_path, &serialized)
}

// ============================================================================
// 5. OpenCode (`~/.config/opencode/opencode.json` + `plugins/` + `plugins.toml`)
// ============================================================================

pub fn discover_opencode_plugins(
    paths: &StoragePaths,
    config: &MagaConfig,
    ledger: &PluginsLedger,
) -> Vec<HarnessPlugin> {
    let opencode_path = paths.effective_opencode_path(config);
    let plugins_dir = paths.effective_opencode_plugins_dir(config);

    let mut active_npm = BTreeSet::new();
    let mut disabled_patterns = BTreeSet::new();

    if opencode_path.is_file()
        && let Ok(text) = fs::read_to_string(&opencode_path)
        && let Ok(root) = serde_json::from_str::<JsonValue>(&text)
    {
        for key in ["plugin", "plugins"] {
            if let Some(arr) = root.get(key).and_then(JsonValue::as_array) {
                for item in arr {
                    let raw = if let Some(s) = item.as_str() {
                        Some(s.to_string())
                    } else {
                        item.get("package")
                            .and_then(JsonValue::as_str)
                            .map(ToString::to_string)
                    };
                    let Some(spec) = raw else {
                        continue;
                    };
                    if spec == "*" {
                        continue;
                    }
                    if let Some(stripped) = spec.strip_prefix('-') {
                        disabled_patterns.insert(stripped.to_string());
                    } else {
                        active_npm.insert(spec);
                    }
                }
            }
        }
    }

    let mut local_dirs = BTreeSet::new();
    if plugins_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&plugins_dir)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(name) = entry.file_name().to_str()
                && !name.starts_with('.')
            {
                local_dirs.insert(name.to_string());
            }
        }
    }

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    // 1. Active npm/package plugins in opencode.json
    for id in &active_npm {
        if local_dirs.contains(id) {
            continue;
        }
        seen.insert(id.clone());
        let enabled = !disabled_patterns.contains(id);
        out.push(HarnessPlugin {
            harness: HarnessKind::OpenCode,
            id: id.clone(),
            display_name: id.clone(),
            description: None,
            enabled,
            kind: PluginKind::Npm,
            source: Some("opencode.json".to_string()),
            version: extract_npm_version(id),
            install_path: None,
            config_path: opencode_path.clone(),
            bundled: BundledCounts::default(),
        });
    }

    // 2. Local directory plugins in ~/.config/opencode/plugins/*
    for dir_name in &local_dirs {
        seen.insert(dir_name.clone());
        let dir_path = plugins_dir.join(dir_name);
        let enabled = !disabled_patterns.contains(dir_name);
        let description = extract_manifest_description(&dir_path);
        let bundled = inspect_bundled_contents(&dir_path);
        out.push(HarnessPlugin {
            harness: HarnessKind::OpenCode,
            id: dir_name.clone(),
            display_name: dir_name.clone(),
            description,
            enabled,
            kind: PluginKind::Local,
            source: Some("plugins/".to_string()),
            version: None,
            install_path: Some(dir_path),
            config_path: opencode_path.clone(),
            bundled,
        });
    }

    // 3. Previously disabled OpenCode npm plugins remembered in `plugins.toml`
    for (id, entry) in &ledger.opencode {
        if seen.contains(id) {
            continue;
        }
        let kind = entry.kind.unwrap_or(PluginKind::Npm);
        if kind == PluginKind::Local && !plugins_dir.join(id).is_dir() {
            continue;
        }
        seen.insert(id.clone());
        out.push(HarnessPlugin {
            harness: HarnessKind::OpenCode,
            id: id.clone(),
            display_name: id.clone(),
            description: None,
            enabled: false,
            kind,
            source: entry
                .source
                .clone()
                .or_else(|| Some("plugins.toml (disabled)".to_string())),
            version: entry.version.clone().or_else(|| extract_npm_version(id)),
            install_path: None,
            config_path: opencode_path.clone(),
            bundled: BundledCounts::default(),
        });
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn extract_npm_version(spec: &str) -> Option<String> {
    let (_, ver) = spec.rsplit_once('@')?;
    if ver.is_empty() || ver.contains('/') {
        None
    } else {
        Some(ver.to_string())
    }
}

pub fn set_opencode_plugin_enabled(
    opencode_path: &Path,
    id: &str,
    kind: PluginKind,
    enabled: bool,
) -> Result<(), String> {
    let mut root: JsonMap<String, JsonValue> = if opencode_path.is_file() {
        let text = fs::read_to_string(opencode_path)
            .map_err(|e| format!("Failed to read {}: {e}", opencode_path.display()))?;
        if text.trim().is_empty() {
            JsonMap::new()
        } else {
            serde_json::from_str(&text)
                .map_err(|e| format!("Invalid OpenCode opencode.json: {e}"))?
        }
    } else {
        JsonMap::new()
    };

    let neg_id = format!("-{id}");

    match kind {
        PluginKind::Npm | PluginKind::Bundle => {
            if enabled {
                // Remove any "-id" disable entry from "plugin" / "plugins"
                for key in ["plugin", "plugins"] {
                    if let Some(arr) = root.get_mut(key).and_then(JsonValue::as_array_mut) {
                        arr.retain(|v| v.as_str() != Some(&neg_id));
                    }
                }
                let target_key = if root.contains_key("plugin") || !root.contains_key("plugins") {
                    "plugin"
                } else {
                    "plugins"
                };
                let arr_entry = root
                    .entry(target_key.to_string())
                    .or_insert_with(|| JsonValue::Array(Vec::new()));
                if !arr_entry.is_array() {
                    *arr_entry = JsonValue::Array(Vec::new());
                }
                let arr = arr_entry.as_array_mut().unwrap();
                let already_present = arr.iter().any(|v| {
                    v.as_str() == Some(id)
                        || v.get("package").and_then(JsonValue::as_str) == Some(id)
                });
                if !already_present {
                    arr.push(json!(id));
                }
            } else {
                // Remove from active "plugin" and "plugins" arrays (it will be retained in plugins.toml)
                for key in ["plugin", "plugins"] {
                    if let Some(arr) = root.get_mut(key).and_then(JsonValue::as_array_mut) {
                        arr.retain(|v| {
                            v.as_str() != Some(id)
                                && v.get("package").and_then(JsonValue::as_str) != Some(id)
                        });
                    }
                }
            }
        }
        PluginKind::Local => {
            let target_key = if root.contains_key("plugins") {
                "plugins"
            } else {
                "plugin"
            };
            let arr_entry = root
                .entry(target_key.to_string())
                .or_insert_with(|| JsonValue::Array(Vec::new()));
            if !arr_entry.is_array() {
                *arr_entry = JsonValue::Array(Vec::new());
            }
            let arr = arr_entry.as_array_mut().unwrap();
            if enabled {
                arr.retain(|v| v.as_str() != Some(&neg_id));
            } else if !arr.iter().any(|v| v.as_str() == Some(&neg_id)) {
                arr.push(json!(neg_id));
            }
        }
    }

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize OpenCode opencode.json: {e}"))?;
    write_atomic_with_backup(opencode_path, &serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_plugins_ledger_toml() {
        let mut ledger = PluginsLedger::default();
        ledger.antigravity.insert(
            "ponytail".to_string(),
            PluginLedgerEntry {
                enabled: true,
                kind: Some(PluginKind::Bundle),
                source: Some("local".to_string()),
                version: None,
            },
        );
        ledger.opencode.insert(
            "opencode-wakatime".to_string(),
            PluginLedgerEntry {
                enabled: false,
                kind: Some(PluginKind::Npm),
                source: Some("opencode.json".to_string()),
                version: None,
            },
        );

        let toml_str = ledger.to_toml_string().expect("serialize plugins.toml");
        let parsed = PluginsLedger::from_toml_str(&toml_str).expect("parse plugins.toml");
        assert_eq!(ledger, parsed);
    }

    #[test]
    fn discovers_and_toggles_agy_claude_and_copilot_plugins_with_bundled_counts() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let config = MagaConfig::default();
        let mut ledger = PluginsLedger::default();

        // 1. Antigravity setup: config.json + plugins/team-kit/{plugin.json, skills/s1, hooks.json, mcp_config.json}
        fs::write(
            paths.effective_agy_plugin_config_path(&config),
            r#"{"plugins": {"ponytail": {"enabled": true}}, "userSettings": {"themeMode": "DARK"}}"#,
        )
        .unwrap();
        let agy_plugin_dir = paths.effective_agy_plugins_dir(&config).join("team-kit");
        fs::create_dir_all(agy_plugin_dir.join("skills").join("review")).unwrap();
        fs::write(
            agy_plugin_dir.join("plugin.json"),
            r#"{"name": "Team Kit", "description": "Team engineering toolkit", "disabled": true, "version": "1.2.0"}"#,
        )
        .unwrap();
        fs::write(
            agy_plugin_dir.join("hooks.json"),
            r#"{"hooks": {"PreToolUse": []}}"#,
        )
        .unwrap();
        fs::write(
            agy_plugin_dir.join("mcp_config.json"),
            r#"{"mcpServers": {"db": {"command": "dbhub"}}}"#,
        )
        .unwrap();

        // 2. Claude Code setup: .claude/plugins/installed_plugins.json + .claude/settings.json
        let claude_plugins_dir = paths
            .effective_claude_installed_plugins_path(&config)
            .parent()
            .unwrap()
            .to_path_buf();
        let boost_cache_dir = claude_plugins_dir.join("cache").join("laravel-boost");
        fs::create_dir_all(boost_cache_dir.join("skills").join("eloquent")).unwrap();
        fs::write(
            paths.effective_claude_installed_plugins_path(&config),
            serde_json::to_string_pretty(&json!({
                "version": 2,
                "plugins": {
                    "laravel-boost@claude-plugins-official": [{
                        "scope": "user",
                        "installPath": boost_cache_dir.to_string_lossy(),
                        "version": "f8c5e7a"
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            paths.effective_claude_settings_path(&config),
            r#"{"model": "sonnet", "enabledPlugins": {"laravel-boost@claude-plugins-official": true}}"#,
        )
        .unwrap();

        // 3. Copilot setup: config.json (note: in tempdir, agy and copilot share config.json unless overridden; let's override copilot_json into a subdir)
        let copilot_dir = dir.path().join("copilot");
        fs::create_dir_all(&copilot_dir).unwrap();
        let mut paths = paths;
        paths.copilot_json = copilot_dir.join("mcp-config.json");
        fs::write(
            paths.effective_copilot_plugin_config_path(&config),
            r#"{"trustedFolders": ["/tmp"], "installedPlugins": [{"name": "ponytail", "marketplace": "ponytail", "enabled": true, "source_sha": "11c3de081cf4ef25"}]}"#,
        )
        .unwrap();

        let mut all = discover_all_harness_plugins(&paths, &config, &mut ledger);

        // Verify Antigravity discovery & toggle
        let agy_list = all.get_mut(&HarnessKind::Antigravity).unwrap();
        assert_eq!(agy_list.len(), 2);
        let team_kit = agy_list.iter_mut().find(|p| p.id == "team-kit").unwrap();
        assert!(!team_kit.enabled); // disabled by default in plugin.json
        assert_eq!(team_kit.display_name, "Team Kit");
        assert_eq!(
            team_kit.description.as_deref(),
            Some("Team engineering toolkit")
        );
        assert_eq!(team_kit.bundled.skills, 1);
        assert_eq!(team_kit.bundled.hooks, 1);
        assert_eq!(team_kit.bundled.mcps, 1);
        assert_eq!(team_kit.bundled.skill_names, vec!["review"]);
        assert_eq!(team_kit.bundled.hook_names, vec!["PreToolUse"]);
        assert_eq!(team_kit.bundled.mcp_names, vec!["db"]);
        assert_eq!(
            team_kit.bundled.summary_badge().as_deref(),
            Some("1 skill, 1 hook, 1 mcp")
        );

        toggle_harness_plugin(&paths, &config, &mut ledger, team_kit).unwrap();
        assert!(team_kit.enabled);
        let agy_raw: JsonValue = serde_json::from_str(
            &fs::read_to_string(paths.effective_agy_plugin_config_path(&config)).unwrap(),
        )
        .unwrap();
        assert_eq!(agy_raw["plugins"]["team-kit"]["enabled"], json!(true));
        assert_eq!(agy_raw["userSettings"]["themeMode"], json!("DARK"));

        // Verify Claude discovery & toggle
        let claude_list = all.get_mut(&HarnessKind::Claude).unwrap();
        assert_eq!(claude_list.len(), 1);
        let boost = &mut claude_list[0];
        assert_eq!(boost.display_name, "laravel-boost");
        assert_eq!(boost.source.as_deref(), Some("claude-plugins-official"));
        assert_eq!(boost.version.as_deref(), Some("f8c5e7a"));
        assert_eq!(boost.bundled.skills, 1);
        assert!(boost.enabled);

        toggle_harness_plugin(&paths, &config, &mut ledger, boost).unwrap();
        assert!(!boost.enabled);
        let claude_raw: JsonValue = serde_json::from_str(
            &fs::read_to_string(paths.effective_claude_settings_path(&config)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            claude_raw["enabledPlugins"]["laravel-boost@claude-plugins-official"],
            json!(false)
        );
        assert_eq!(claude_raw["model"], json!("sonnet"));

        // Verify Copilot discovery & toggle
        let cop_list = all.get_mut(&HarnessKind::Copilot).unwrap();
        assert_eq!(cop_list.len(), 1);
        let cop_ponytail = &mut cop_list[0];
        assert!(cop_ponytail.enabled);
        assert_eq!(cop_ponytail.version.as_deref(), Some("11c3de081cf4"));

        toggle_harness_plugin(&paths, &config, &mut ledger, cop_ponytail).unwrap();
        assert!(!cop_ponytail.enabled);
        let cop_raw: JsonValue = serde_json::from_str(
            &fs::read_to_string(paths.effective_copilot_plugin_config_path(&config)).unwrap(),
        )
        .unwrap();
        assert_eq!(cop_raw["installedPlugins"][0]["enabled"], json!(false));
        assert_eq!(cop_raw["trustedFolders"], json!(["/tmp"]));
    }

    #[test]
    fn discovers_and_toggles_grok_and_opencode_plugins_retaining_disabled_npm_in_plugins_toml() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let config = MagaConfig::default();
        let mut ledger = PluginsLedger::default();

        // 1. Grok setup: grok_config.toml + installed-plugins/xai-helper
        let grok_plugin_dir = paths
            .effective_grok_installed_plugins_dir(&config)
            .join("xai-helper");
        fs::create_dir_all(&grok_plugin_dir).unwrap();
        fs::write(
            &paths.grok_toml,
            r#"
[models]
default = "grok-4.5"

[plugins]
disabled = ["user/noisy-plugin"]
"#,
        )
        .unwrap();

        // 2. OpenCode setup: opencode.json with "plugin" array + plugins/workmux-status local dir
        let oc_local_dir = paths
            .effective_opencode_plugins_dir(&config)
            .join("workmux-status");
        fs::create_dir_all(&oc_local_dir).unwrap();
        fs::write(
            &paths.opencode_json,
            r#"{
  "$schema": "https://opencode.ai/config.json",
  "plugin": [
    "opencode-wakatime",
    "@tarquinen/opencode-dcp@latest"
  ]
}"#,
        )
        .unwrap();

        let mut all = discover_all_harness_plugins(&paths, &config, &mut ledger);

        // Grok verification
        let grok_list = all.get_mut(&HarnessKind::Grok).unwrap();
        assert_eq!(grok_list.len(), 2);
        let xai_helper = grok_list.iter_mut().find(|p| p.id == "xai-helper").unwrap();
        assert!(xai_helper.enabled);
        toggle_harness_plugin(&paths, &config, &mut ledger, xai_helper).unwrap();
        assert!(!xai_helper.enabled);
        let grok_raw: TomlTable =
            toml::from_str(&fs::read_to_string(&paths.grok_toml).unwrap()).unwrap();
        let disabled_arr = grok_raw["plugins"]["disabled"].as_array().unwrap();
        assert!(disabled_arr.contains(&TomlValue::String("xai-helper".to_string())));
        assert_eq!(grok_raw["models"]["default"].as_str(), Some("grok-4.5"));

        // OpenCode verification: disable "opencode-wakatime" -> removed from opencode.json, retained in plugins.toml
        let oc_list = all.get_mut(&HarnessKind::OpenCode).unwrap();
        assert_eq!(oc_list.len(), 3);
        let waka = oc_list
            .iter_mut()
            .find(|p| p.id == "opencode-wakatime")
            .unwrap();
        assert!(waka.enabled);
        assert_eq!(waka.kind, PluginKind::Npm);

        toggle_harness_plugin(&paths, &config, &mut ledger, waka).unwrap();
        assert!(!waka.enabled);

        let oc_raw: JsonValue =
            serde_json::from_str(&fs::read_to_string(&paths.opencode_json).unwrap()).unwrap();
        let active_arr = oc_raw["plugin"].as_array().unwrap();
        assert!(!active_arr.contains(&json!("opencode-wakatime")));
        assert!(active_arr.contains(&json!("@tarquinen/opencode-dcp@latest")));

        // Re-discover from disk using saved plugins.toml: "opencode-wakatime" is still discovered as disabled!
        let saved_ledger_str = fs::read_to_string(&paths.plugins_toml).unwrap();
        let mut reloaded_ledger = PluginsLedger::from_toml_str(&saved_ledger_str).unwrap();
        let mut rediscovered = discover_all_harness_plugins(&paths, &config, &mut reloaded_ledger);
        let oc_rediscovered = rediscovered.get_mut(&HarnessKind::OpenCode).unwrap();
        let waka_again = oc_rediscovered
            .iter_mut()
            .find(|p| p.id == "opencode-wakatime")
            .expect("retained disabled opencode plugin");
        assert!(!waka_again.enabled);

        // Toggle back ON -> restored to opencode.json "plugin" array!
        toggle_harness_plugin(&paths, &config, &mut reloaded_ledger, waka_again).unwrap();
        assert!(waka_again.enabled);
        let oc_restored: JsonValue =
            serde_json::from_str(&fs::read_to_string(&paths.opencode_json).unwrap()).unwrap();
        assert!(
            oc_restored["plugin"]
                .as_array()
                .unwrap()
                .contains(&json!("opencode-wakatime"))
        );

        // Remove "opencode-wakatime" completely via remove_harness_plugin
        remove_harness_plugin(
            &paths,
            &config,
            &mut reloaded_ledger,
            HarnessKind::OpenCode,
            "opencode-wakatime",
            None,
        )
        .unwrap();
        assert!(!reloaded_ledger.opencode.contains_key("opencode-wakatime"));
        let oc_after_remove: JsonValue =
            serde_json::from_str(&fs::read_to_string(&paths.opencode_json).unwrap()).unwrap();
        assert!(
            !oc_after_remove["plugin"]
                .as_array()
                .unwrap()
                .contains(&json!("opencode-wakatime"))
        );

        // Seed a ghost non-OpenCode entry into ledger and verify sync_from_discovered prunes it
        reloaded_ledger.antigravity.insert(
            "ghost-plugin".to_string(),
            PluginLedgerEntry {
                enabled: true,
                kind: Some(PluginKind::Bundle),
                source: None,
                version: None,
            },
        );
        let _ = discover_all_harness_plugins(&paths, &config, &mut reloaded_ledger);
        assert!(!reloaded_ledger.antigravity.contains_key("ghost-plugin"));
    }

    #[test]
    fn removes_directory_backed_plugins_completely_across_all_five_harnesses() {
        let dir = tempdir().expect("create tempdir");
        let mut paths = StoragePaths::in_dir(dir.path());
        let copilot_dir = dir.path().join("copilot");
        let opencode_dir = dir.path().join("opencode");
        fs::create_dir_all(&copilot_dir).unwrap();
        fs::create_dir_all(&opencode_dir).unwrap();
        paths.copilot_json = copilot_dir.join("mcp-config.json");
        paths.opencode_json = opencode_dir.join("opencode.json");
        let config = MagaConfig::default();
        let mut ledger = PluginsLedger::default();

        // 1. Antigravity directory-backed plugin
        let agy_plugin_dir = paths.effective_agy_plugins_dir(&config).join("team-kit");
        fs::create_dir_all(&agy_plugin_dir).unwrap();
        fs::write(
            paths.effective_agy_plugin_config_path(&config),
            r#"{"plugins": {"team-kit": {"enabled": true}}}"#,
        )
        .unwrap();

        // 2. Claude Code directory-backed plugin (`claudeline@claude-plugins-official`)
        let claude_plugins_dir = paths
            .effective_claude_installed_plugins_path(&config)
            .parent()
            .unwrap()
            .to_path_buf();
        let claudeline_cache_dir = claude_plugins_dir
            .join("cache")
            .join("claude-plugins-official")
            .join("claudeline")
            .join("1.0.0");
        fs::create_dir_all(&claudeline_cache_dir).unwrap();
        fs::write(
            paths.effective_claude_installed_plugins_path(&config),
            serde_json::to_string_pretty(&json!({
                "version": 2,
                "plugins": {
                    "claudeline@claude-plugins-official": [{
                        "scope": "user",
                        "installPath": claudeline_cache_dir.to_string_lossy(),
                        "version": "1.0.0"
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            paths.effective_claude_settings_path(&config),
            r#"{"enabledPlugins": {"claudeline@claude-plugins-official": true}}"#,
        )
        .unwrap();

        // 3. Copilot directory-backed plugin
        let cop_cache_dir = paths
            .effective_copilot_installed_plugins_dir(&config)
            .join("ponytail")
            .join("ponytail");
        fs::create_dir_all(&cop_cache_dir).unwrap();
        fs::write(
            paths.effective_copilot_plugin_config_path(&config),
            serde_json::to_string_pretty(&json!({
                "installedPlugins": [{
                    "name": "ponytail",
                    "enabled": true,
                    "cache_path": cop_cache_dir.to_string_lossy()
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        // 4. Grok directory-backed plugin
        let grok_plugin_dir = paths
            .effective_grok_installed_plugins_dir(&config)
            .join("xai-helper");
        fs::create_dir_all(&grok_plugin_dir).unwrap();
        fs::write(
            &paths.grok_toml,
            r#"
[plugins]
disabled = ["xai-helper"]
paths = ["xai-helper"]
"#,
        )
        .unwrap();

        // 5. OpenCode local directory plugin
        let oc_local_dir = paths
            .effective_opencode_plugins_dir(&config)
            .join("workmux-status");
        fs::create_dir_all(&oc_local_dir).unwrap();
        fs::write(&paths.opencode_json, r#"{"plugin": ["-workmux-status"]}"#).unwrap();

        let discovered = discover_all_harness_plugins(&paths, &config, &mut ledger);
        assert_eq!(discovered[&HarnessKind::Antigravity].len(), 1);
        assert_eq!(discovered[&HarnessKind::Claude].len(), 1);
        assert_eq!(discovered[&HarnessKind::Copilot].len(), 1);
        assert_eq!(discovered[&HarnessKind::Grok].len(), 1);
        assert_eq!(discovered[&HarnessKind::OpenCode].len(), 1);

        // Remove each plugin completely
        for (harness, list) in &discovered {
            for p in list {
                remove_harness_plugin(
                    &paths,
                    &config,
                    &mut ledger,
                    *harness,
                    &p.id,
                    p.install_path.as_deref(),
                )
                .unwrap();
            }
        }

        assert!(!agy_plugin_dir.exists());
        assert!(!claudeline_cache_dir.exists());
        assert!(!cop_cache_dir.exists());
        assert!(
            !paths
                .effective_copilot_installed_plugins_dir(&config)
                .join("ponytail")
                .exists()
        );
        assert!(!grok_plugin_dir.exists());
        assert!(!oc_local_dir.exists());

        // Re-discover and ensure none of the uninstalled plugins reappear
        let rediscovered = discover_all_harness_plugins(&paths, &config, &mut ledger);
        for kind in HarnessKind::ALL {
            assert!(
                rediscovered[&kind].is_empty(),
                "expected {kind:?} plugins to be empty after uninstall, found {:?}",
                rediscovered[&kind]
            );
        }
        assert!(ledger.is_empty());
    }

    #[test]
    fn extract_manifest_name_reads_plugin_json_name() {
        let dir = tempdir().expect("create tempdir");
        let plugin_dir = dir.path().join("skills-343a64e7");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.json"),
            r#"{"name": "ponytail", "description": "Lazy senior dev mode"}"#,
        )
        .unwrap();

        assert_eq!(
            extract_manifest_name(&plugin_dir).as_deref(),
            Some("ponytail")
        );
        assert_eq!(
            extract_manifest_description(&plugin_dir).as_deref(),
            Some("Lazy senior dev mode")
        );
        assert_eq!(extract_manifest_name(dir.path()), None);
    }

    #[test]
    fn discovers_grok_display_name_from_opaque_install_dir_manifest() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let config = MagaConfig::default();
        let mut ledger = PluginsLedger::default();

        let opaque_id = "skills-343a64e7";
        let grok_plugin_dir = paths
            .effective_grok_installed_plugins_dir(&config)
            .join(opaque_id);
        fs::create_dir_all(&grok_plugin_dir).unwrap();
        fs::write(
            grok_plugin_dir.join("plugin.json"),
            r#"{"name": "ponytail", "description": "Lazy senior dev mode"}"#,
        )
        .unwrap();
        fs::write(
            &paths.grok_toml,
            r#"
[models]
default = "grok-4.5"

[plugins]
disabled = []
"#,
        )
        .unwrap();

        let all = discover_all_harness_plugins(&paths, &config, &mut ledger);
        let grok_list = &all[&HarnessKind::Grok];
        assert_eq!(grok_list.len(), 1);
        let plugin = &grok_list[0];
        assert_eq!(plugin.id, opaque_id);
        assert_eq!(plugin.display_name, "ponytail");
        assert_eq!(plugin.description.as_deref(), Some("Lazy senior dev mode"));
        assert!(plugin.enabled);

        // Toggle still keys on the opaque install id.
        let mut plugin = plugin.clone();
        toggle_harness_plugin(&paths, &config, &mut ledger, &mut plugin).unwrap();
        assert!(!plugin.enabled);
        let grok_raw: TomlTable =
            toml::from_str(&fs::read_to_string(&paths.grok_toml).unwrap()).unwrap();
        let disabled_arr = grok_raw["plugins"]["disabled"].as_array().unwrap();
        assert!(disabled_arr.contains(&TomlValue::String(opaque_id.to_string())));
        assert_eq!(grok_raw["models"]["default"].as_str(), Some("grok-4.5"));
    }

    #[test]
    fn discovers_grok_display_name_from_registry_when_manifest_name_missing() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let config = MagaConfig::default();
        let mut ledger = PluginsLedger::default();

        let opaque_id = "skills-abcdef12";
        let installed_dir = paths.effective_grok_installed_plugins_dir(&config);
        let grok_plugin_dir = installed_dir.join(opaque_id);
        fs::create_dir_all(&grok_plugin_dir).unwrap();
        // Manifest present but without a usable name field.
        fs::write(
            grok_plugin_dir.join("plugin.json"),
            r#"{"description": "Fallback via registry"}"#,
        )
        .unwrap();
        fs::write(
            installed_dir.join("registry.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "repos": {
                    opaque_id: {
                        "marketplace": {
                            "source_display_name": "registry-named-plugin"
                        },
                        "plugins": {
                            "should-not-win": {}
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(&paths.grok_toml, "[plugins]\ndisabled = []\n").unwrap();

        let all = discover_all_harness_plugins(&paths, &config, &mut ledger);
        let plugin = all[&HarnessKind::Grok]
            .iter()
            .find(|p| p.id == opaque_id)
            .expect("opaque grok plugin");
        assert_eq!(plugin.display_name, "registry-named-plugin");
        assert_eq!(plugin.description.as_deref(), Some("Fallback via registry"));
        assert_eq!(
            plugin_path_basename(r"C:\Users\dev\.grok\plugins\my-plugin"),
            "my-plugin"
        );
        assert_eq!(plugin_path_basename("user/noisy-plugin"), "noisy-plugin");
    }
}
