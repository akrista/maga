use crate::domain::agy::AgyDocument;
use crate::domain::claude::ClaudeDocument;
use crate::domain::copilot::CopilotDocument;
use crate::domain::grok::GrokDocument;
use crate::domain::model::{
    DEFAULT_AGY_CONFIG_PATH, DEFAULT_CLAUDE_CONFIG_PATH, DEFAULT_COPILOT_CONFIG_PATH,
    DEFAULT_GROK_CONFIG_PATH, DEFAULT_OPENCODE_CONFIG_PATH, HarnessKind, MagaConfig, McpRegistry,
    McpServerConfig, ProfilesConfig,
};
use crate::domain::opencode::OpenCodeDocument;
use crate::domain::plugins::{HarnessPlugin, PluginsLedger, discover_all_harness_plugins};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Target file paths managed by `maga`.
#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub config_toml: PathBuf,
    pub mcp_toml: PathBuf,
    pub plugins_toml: PathBuf,
    pub profiles_toml: PathBuf,
    pub refs_toml: PathBuf,
    pub refs_cache_dir: PathBuf,
    pub agy_json: PathBuf,
    pub claude_json: PathBuf,
    pub copilot_json: PathBuf,
    pub grok_toml: PathBuf,
    pub opencode_json: PathBuf,
    pub agy_overridden: bool,
    pub claude_overridden: bool,
    pub copilot_overridden: bool,
    pub grok_overridden: bool,
    pub opencode_overridden: bool,
    pub isolated: bool,
}

impl StoragePaths {
    pub fn resolve_default() -> Result<Self, String> {
        let home = std::env::home_dir()
            .ok_or_else(|| "Could not determine $HOME directory.".to_string())?;
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let maga_dir = config_root.join("maga");

        let data_root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));
        let refs_cache_dir = data_root.join("maga").join("refs");

        Ok(Self {
            config_toml: maga_dir.join("config.toml"),
            mcp_toml: maga_dir.join("mcp.toml"),
            plugins_toml: maga_dir.join("plugins.toml"),
            profiles_toml: maga_dir.join("profiles.toml"),
            refs_toml: maga_dir.join("refs.toml"),
            refs_cache_dir,
            agy_json: home.join(".gemini").join("config").join("mcp_config.json"),
            claude_json: home.join(".claude.json"),
            copilot_json: home.join(".copilot").join("mcp-config.json"),
            grok_toml: home.join(".grok").join("config.toml"),
            opencode_json: config_root.join("opencode").join("opencode.json"),
            agy_overridden: false,
            claude_overridden: false,
            copilot_overridden: false,
            grok_overridden: false,
            opencode_overridden: false,
            isolated: false,
        })
    }

    /// Creates isolated test paths inside `dir`.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config_toml: dir.join("config.toml"),
            mcp_toml: dir.join("mcp.toml"),
            plugins_toml: dir.join("plugins.toml"),
            profiles_toml: dir.join("profiles.toml"),
            refs_toml: dir.join("refs.toml"),
            refs_cache_dir: dir.join("refs"),
            agy_json: dir.join("mcp_config.json"),
            claude_json: dir.join(".claude.json"),
            copilot_json: dir.join("mcp-config.json"),
            grok_toml: dir.join("grok_config.toml"),
            opencode_json: dir.join("opencode.json"),
            agy_overridden: false,
            claude_overridden: false,
            copilot_overridden: false,
            grok_overridden: false,
            opencode_overridden: false,
            isolated: true,
        }
    }

    pub fn effective_agy_plugin_config_path(&self, config: &MagaConfig) -> PathBuf {
        let agy_mcp = self.effective_agy_path(config);
        agy_mcp
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("config.json")
    }

    pub fn effective_agy_plugins_dir(&self, config: &MagaConfig) -> PathBuf {
        let agy_mcp = self.effective_agy_path(config);
        agy_mcp
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plugins")
    }

    pub fn effective_claude_dir(&self, config: &MagaConfig) -> PathBuf {
        let claude_mcp = self.effective_claude_path(config);
        claude_mcp
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".claude")
    }

    pub fn effective_claude_settings_path(&self, config: &MagaConfig) -> PathBuf {
        self.effective_claude_dir(config).join("settings.json")
    }

    pub fn effective_claude_installed_plugins_path(&self, config: &MagaConfig) -> PathBuf {
        self.effective_claude_dir(config)
            .join("plugins")
            .join("installed_plugins.json")
    }

    pub fn effective_copilot_plugin_config_path(&self, config: &MagaConfig) -> PathBuf {
        let copilot_mcp = self.effective_copilot_path(config);
        copilot_mcp
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("config.json")
    }

    pub fn effective_copilot_installed_plugins_dir(&self, config: &MagaConfig) -> PathBuf {
        let copilot_mcp = self.effective_copilot_path(config);
        copilot_mcp
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("installed-plugins")
    }

    pub fn effective_grok_installed_plugins_dir(&self, config: &MagaConfig) -> PathBuf {
        let grok_cfg = self.effective_grok_path(config);
        grok_cfg
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("installed-plugins")
    }

    pub fn effective_opencode_plugins_dir(&self, config: &MagaConfig) -> PathBuf {
        let opencode_cfg = self.effective_opencode_path(config);
        opencode_cfg
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plugins")
    }

    pub fn effective_harness_path(&self, kind: HarnessKind, config: &MagaConfig) -> PathBuf {
        match kind {
            HarnessKind::Antigravity => self.effective_agy_path(config),
            HarnessKind::Claude => self.effective_claude_path(config),
            HarnessKind::Copilot => self.effective_copilot_path(config),
            HarnessKind::Grok => self.effective_grok_path(config),
            HarnessKind::OpenCode => self.effective_opencode_path(config),
        }
    }

    pub fn effective_agy_path(&self, config: &MagaConfig) -> PathBuf {
        if self.agy_overridden {
            return self.agy_json.clone();
        }
        if let Some(ref custom) = config.tools.antigravity.config_path {
            let trimmed = custom.trim();
            if !trimmed.is_empty() && trimmed != DEFAULT_AGY_CONFIG_PATH {
                return expand_tilde(trimmed);
            }
        }
        self.agy_json.clone()
    }

    pub fn effective_claude_path(&self, config: &MagaConfig) -> PathBuf {
        if self.claude_overridden {
            return self.claude_json.clone();
        }
        if let Some(ref custom) = config.tools.claude.config_path {
            let trimmed = custom.trim();
            if !trimmed.is_empty() && trimmed != DEFAULT_CLAUDE_CONFIG_PATH {
                return expand_tilde(trimmed);
            }
        }
        self.claude_json.clone()
    }

    pub fn effective_copilot_path(&self, config: &MagaConfig) -> PathBuf {
        if self.copilot_overridden {
            return self.copilot_json.clone();
        }
        if let Some(ref custom) = config.tools.copilot.config_path {
            let trimmed = custom.trim();
            if !trimmed.is_empty() && trimmed != DEFAULT_COPILOT_CONFIG_PATH {
                return expand_tilde(trimmed);
            }
        }
        self.copilot_json.clone()
    }

    pub fn effective_grok_path(&self, config: &MagaConfig) -> PathBuf {
        if self.grok_overridden {
            return self.grok_toml.clone();
        }
        if let Some(ref custom) = config.tools.grok.config_path {
            let trimmed = custom.trim();
            if !trimmed.is_empty() && trimmed != DEFAULT_GROK_CONFIG_PATH {
                return expand_tilde(trimmed);
            }
        }
        self.grok_toml.clone()
    }

    pub fn effective_opencode_path(&self, config: &MagaConfig) -> PathBuf {
        if self.opencode_overridden {
            return self.opencode_json.clone();
        }
        if let Some(ref custom) = config.tools.opencode.config_path {
            let trimmed = custom.trim();
            if !trimmed.is_empty() && trimmed != DEFAULT_OPENCODE_CONFIG_PATH {
                return expand_tilde(trimmed);
            }
        }
        self.opencode_json.clone()
    }

    pub fn dotenv_search_paths(&self) -> Vec<PathBuf> {
        if self.isolated {
            return self
                .mcp_toml
                .parent()
                .map(|maga_dir| maga_dir.join(".env.local"))
                .into_iter()
                .collect();
        }

        let mut paths = vec![
            expand_tilde("~/.env.local"),
            expand_tilde("~/.config/maga/.env.local"),
        ];
        if let Some(maga_dir) = self.mcp_toml.parent() {
            let local = maga_dir.join(".env.local");
            if !paths.contains(&local) {
                paths.push(local);
            }
        }
        paths
    }

    pub fn load_env_context(&self) -> BTreeMap<String, String> {
        crate::domain::model::load_env_context(&self.dotenv_search_paths())
    }
}

pub fn expand_tilde(path_str: &str) -> PathBuf {
    if let Some(rest) = path_str
        .strip_prefix("~/")
        .or_else(|| path_str.strip_prefix("~\\"))
        && let Some(home) = std::env::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path_str)
}

/// Loaded state from disk for general `maga` config, canonical MCP registry, and all harnesses.
#[derive(Debug, Clone, Default)]
pub struct LoadedConfigs {
    pub config: MagaConfig,
    pub config_exists: bool,
    pub config_needs_save: bool,
    pub mcp: McpRegistry,
    pub mcp_exists: bool,
    pub mcp_had_spaced_keys: bool,
    pub plugins_ledger: PluginsLedger,
    pub plugins_ledger_exists: bool,
    pub plugins_ledger_needs_save: bool,
    pub profiles: ProfilesConfig,
    pub profiles_exists: bool,
    pub profiles_needs_save: bool,
    pub refs: crate::domain::refs::RefsRegistry,
    pub refs_exists: bool,
    pub harness_plugins: BTreeMap<HarnessKind, Vec<HarnessPlugin>>,
    pub agy: AgyDocument,
    pub claude: ClaudeDocument,
    pub copilot: CopilotDocument,
    pub grok: GrokDocument,
    pub opencode: OpenCodeDocument,
}

impl LoadedConfigs {
    pub fn harness_servers(&self, kind: HarnessKind) -> &BTreeMap<String, McpServerConfig> {
        match kind {
            HarnessKind::Antigravity => &self.agy.servers,
            HarnessKind::Claude => &self.claude.servers,
            HarnessKind::Copilot => &self.copilot.servers,
            HarnessKind::Grok => &self.grok.servers,
            HarnessKind::OpenCode => &self.opencode.servers,
        }
    }

    pub fn harness_has_raw_spaced_keys(&self, kind: HarnessKind) -> bool {
        match kind {
            HarnessKind::Antigravity => self.agy.has_raw_spaced_keys(),
            HarnessKind::Claude => self.claude.has_raw_spaced_keys(),
            HarnessKind::Copilot => self.copilot.has_raw_spaced_keys(),
            HarnessKind::Grok => self.grok.has_raw_spaced_keys(),
            HarnessKind::OpenCode => self.opencode.has_raw_spaced_keys(),
        }
    }

    pub fn enabled_harness_servers(&self) -> Vec<(HarnessKind, BTreeMap<String, McpServerConfig>)> {
        HarnessKind::ALL
            .into_iter()
            .filter(|k| self.config.tools.is_enabled(*k))
            .map(|k| (k, self.harness_servers(k).clone()))
            .collect()
    }

    pub fn load(paths: &StoragePaths) -> Result<Self, String> {
        let config_exists = paths.config_toml.exists();
        let mut config_needs_save = !config_exists;
        let mut config = if config_exists {
            let content = fs::read_to_string(&paths.config_toml)
                .map_err(|e| format!("Failed to read {}: {e}", paths.config_toml.display()))?;
            if !MagaConfig::has_all_default_keys(&content) {
                config_needs_save = true;
            }
            MagaConfig::from_toml_str(&content)?
        } else {
            MagaConfig::default()
        };

        let mcp_exists = paths.mcp_toml.exists();
        let mut mcp_had_spaced_keys = false;
        let mcp = if mcp_exists {
            let content = fs::read_to_string(&paths.mcp_toml)
                .map_err(|e| format!("Failed to read {}: {e}", paths.mcp_toml.display()))?;
            if let Ok(raw_tbl) = toml::from_str::<toml::Table>(&content)
                && let Some(mcps_tbl) = raw_tbl.get("mcps").and_then(|v| v.as_table())
                && mcps_tbl
                    .keys()
                    .any(|k| crate::domain::model::normalize_mcp_server_name(k) != *k)
            {
                mcp_had_spaced_keys = true;
            }
            McpRegistry::from_toml_str(&content)?
        } else if let Some(migrated) = config.take_legacy_mcps()? {
            config_needs_save = true;
            migrated
        } else {
            McpRegistry::default()
        };

        let agy_path = paths.effective_agy_path(&config);
        let agy = if config.tools.antigravity.enabled && agy_path.exists() {
            let content = fs::read_to_string(&agy_path)
                .map_err(|e| format!("Failed to read {}: {e}", agy_path.display()))?;
            AgyDocument::parse_json_str(&content)?
        } else {
            AgyDocument::default()
        };

        let claude_path = paths.effective_claude_path(&config);
        let claude = if config.tools.claude.enabled && claude_path.exists() {
            let content = fs::read_to_string(&claude_path)
                .map_err(|e| format!("Failed to read {}: {e}", claude_path.display()))?;
            ClaudeDocument::parse_json_str(&content)?
        } else {
            ClaudeDocument::default()
        };

        let copilot_path = paths.effective_copilot_path(&config);
        let copilot = if config.tools.copilot.enabled && copilot_path.exists() {
            let content = fs::read_to_string(&copilot_path)
                .map_err(|e| format!("Failed to read {}: {e}", copilot_path.display()))?;
            CopilotDocument::parse_json_str(&content)?
        } else {
            CopilotDocument::default()
        };

        let grok_path = paths.effective_grok_path(&config);
        let grok = if config.tools.grok.enabled && grok_path.exists() {
            let content = fs::read_to_string(&grok_path)
                .map_err(|e| format!("Failed to read {}: {e}", grok_path.display()))?;
            GrokDocument::parse_toml_str(&content)?
        } else {
            GrokDocument::default()
        };

        let opencode_path = paths.effective_opencode_path(&config);
        let opencode = if config.tools.opencode.enabled && opencode_path.exists() {
            let content = fs::read_to_string(&opencode_path)
                .map_err(|e| format!("Failed to read {}: {e}", opencode_path.display()))?;
            OpenCodeDocument::parse_json_str(&content)?
        } else {
            OpenCodeDocument::default()
        };

        let plugins_ledger_exists = paths.plugins_toml.exists();
        let mut plugins_ledger = if plugins_ledger_exists {
            let content = fs::read_to_string(&paths.plugins_toml)
                .map_err(|e| format!("Failed to read {}: {e}", paths.plugins_toml.display()))?;
            PluginsLedger::from_toml_str(&content)?
        } else {
            PluginsLedger::default()
        };
        let prev_ledger = plugins_ledger.clone();
        let harness_plugins = discover_all_harness_plugins(paths, &config, &mut plugins_ledger);
        let plugins_ledger_needs_save =
            plugins_ledger != prev_ledger || (!plugins_ledger_exists && !plugins_ledger.is_empty());

        let profiles_exists = paths.profiles_toml.exists();
        let profiles = if profiles_exists {
            let content = fs::read_to_string(&paths.profiles_toml)
                .map_err(|e| format!("Failed to read {}: {e}", paths.profiles_toml.display()))?;
            ProfilesConfig::from_toml_str(&content)?
        } else {
            ProfilesConfig::default()
        };

        let refs_exists = paths.refs_toml.exists();
        let refs = if refs_exists {
            let content = fs::read_to_string(&paths.refs_toml)
                .map_err(|e| format!("Failed to read {}: {e}", paths.refs_toml.display()))?;
            crate::domain::refs::RefsRegistry::from_toml(&content)
                .map_err(|e| format!("Failed to parse {}: {e}", paths.refs_toml.display()))?
        } else {
            crate::domain::refs::RefsRegistry::default()
        };

        Ok(Self {
            config,
            config_exists,
            config_needs_save,
            mcp,
            mcp_exists,
            mcp_had_spaced_keys,
            plugins_ledger,
            plugins_ledger_exists,
            plugins_ledger_needs_save,
            profiles,
            profiles_exists,
            profiles_needs_save: false,
            refs,
            refs_exists,
            harness_plugins,
            agy,
            claude,
            copilot,
            grok,
            opencode,
        })
    }

    /// Writes `profiles.toml` atomically to disk.
    pub fn save_profiles(&mut self, paths: &StoragePaths) -> Result<(), String> {
        let profiles_str = self.profiles.to_toml_string()?;
        write_atomic_with_backup(&paths.profiles_toml, &profiles_str)?;
        self.profiles_exists = true;
        self.profiles_needs_save = false;
        Ok(())
    }

    /// Writes `config.toml` atomically to disk.
    pub fn save_config(&mut self, paths: &StoragePaths) -> Result<(), String> {
        let config_str = self.config.to_toml_string()?;
        write_atomic_with_backup(&paths.config_toml, &config_str)?;
        self.config_exists = true;
        self.config_needs_save = false;
        Ok(())
    }

    /// Writes `refs.toml` atomically to disk.
    pub fn save_refs(&mut self, paths: &StoragePaths) -> Result<(), String> {
        let refs_str = self
            .refs
            .to_toml()
            .map_err(|e| format!("Failed to serialize refs registry: {e}"))?;
        write_atomic_with_backup(&paths.refs_toml, &refs_str)?;
        self.refs_exists = true;
        Ok(())
    }

    /// Writes `plugins.toml` atomically to disk.
    pub fn save_plugins_ledger(&mut self, paths: &StoragePaths) -> Result<(), String> {
        let ledger_str = self.plugins_ledger.to_toml_string()?;
        write_atomic_with_backup(&paths.plugins_toml, &ledger_str)?;
        self.plugins_ledger_exists = true;
        self.plugins_ledger_needs_save = false;
        Ok(())
    }

    /// Re-runs plugin discovery across enabled harnesses and persists `plugins.toml` if updated.
    pub fn refresh_plugins(&mut self, paths: &StoragePaths) -> Result<(), String> {
        let prev_ledger = self.plugins_ledger.clone();
        self.harness_plugins =
            discover_all_harness_plugins(paths, &self.config, &mut self.plugins_ledger);
        if self.plugins_ledger != prev_ledger
            || (!self.plugins_ledger_exists && !self.plugins_ledger.is_empty())
        {
            self.save_plugins_ledger(paths)?;
        }
        Ok(())
    }

    /// Synchronizes general `config.toml` (when created or migrated) and the canonical MCP server map
    /// to `mcp.toml` and all enabled harnesses atomically.
    pub fn save_all(
        &mut self,
        paths: &StoragePaths,
        canonical: &BTreeMap<String, McpServerConfig>,
    ) -> Result<(), String> {
        self.save_all_with_options(paths, canonical, cfg!(windows), None)
    }

    /// Synchronizes configurations with explicit platform wrapping and environment context options.
    pub fn save_all_with_options(
        &mut self,
        paths: &StoragePaths,
        canonical: &BTreeMap<String, McpServerConfig>,
        is_windows: bool,
        custom_env: Option<&BTreeMap<String, String>>,
    ) -> Result<(), String> {
        self.mcp.mcps = canonical.clone();

        let mcp_str = self.mcp.to_toml_string()?;

        if !self.config_exists || self.config_needs_save {
            self.save_config(paths)?;
        }
        if self.plugins_ledger_needs_save {
            self.save_plugins_ledger(paths)?;
        }
        if self.profiles_needs_save {
            self.save_profiles(paths)?;
        }

        write_atomic_with_backup(&paths.mcp_toml, &mcp_str)?;
        self.mcp_exists = true;
        self.mcp_had_spaced_keys = false;

        let default_env;
        let env_ref = match custom_env {
            Some(e) => e,
            None => {
                default_env = paths.load_env_context();
                &default_env
            }
        };

        let active_prof = self
            .config
            .active_profile
            .as_deref()
            .or(self.profiles.active.as_deref());

        let export_for_harness = |harness: HarnessKind| -> BTreeMap<String, McpServerConfig> {
            canonical
                .iter()
                .map(|(k, v)| {
                    let mut exported = v.for_harness_export(is_windows, env_ref);
                    let profile_active = self.profiles.is_server_active(active_prof, k);
                    let harness_enabled = v.enabled_for_harness(harness);
                    exported.enabled = profile_active && harness_enabled;
                    (k.clone(), exported)
                })
                .collect()
        };

        if self.config.tools.antigravity.enabled {
            let agy_servers = export_for_harness(HarnessKind::Antigravity);
            let agy_str = self.agy.to_json_string(&agy_servers)?;
            let agy_path = paths.effective_agy_path(&self.config);
            write_atomic_with_backup(&agy_path, &agy_str)?;
            self.agy = AgyDocument::parse_json_str(&agy_str)?;
        }

        if self.config.tools.claude.enabled {
            let claude_servers = export_for_harness(HarnessKind::Claude);
            let claude_str = self.claude.to_json_string(&claude_servers)?;
            let claude_path = paths.effective_claude_path(&self.config);
            write_atomic_with_backup(&claude_path, &claude_str)?;
            self.claude = ClaudeDocument::parse_json_str(&claude_str)?;
        }

        if self.config.tools.copilot.enabled {
            let copilot_servers = export_for_harness(HarnessKind::Copilot);
            let copilot_str = self.copilot.to_json_string(&copilot_servers)?;
            let copilot_path = paths.effective_copilot_path(&self.config);
            write_atomic_with_backup(&copilot_path, &copilot_str)?;
            self.copilot = CopilotDocument::parse_json_str(&copilot_str)?;
        }

        if self.config.tools.grok.enabled {
            let grok_servers = export_for_harness(HarnessKind::Grok);
            let grok_str = self.grok.to_toml_string(&grok_servers)?;
            let grok_path = paths.effective_grok_path(&self.config);
            write_atomic_with_backup(&grok_path, &grok_str)?;
            self.grok = GrokDocument::parse_toml_str(&grok_str)?;
        }

        if self.config.tools.opencode.enabled {
            let opencode_servers = export_for_harness(HarnessKind::OpenCode);
            let opencode_str = self.opencode.to_json_string(&opencode_servers)?;
            let opencode_path = paths.effective_opencode_path(&self.config);
            write_atomic_with_backup(&opencode_path, &opencode_str)?;
            self.opencode = OpenCodeDocument::parse_json_str(&opencode_str)?;
        }

        Ok(())
    }

    /// Performs a headless check across all enabled harnesses without modifying any files.
    pub fn check_headless(
        &self,
        paths: &StoragePaths,
    ) -> crate::domain::reconcile::ConvergenceStatus {
        let enabled_servers = self.enabled_harness_servers();
        // Check if any harness file is missing on disk
        let any_missing_file = HarnessKind::ALL.into_iter().any(|kind| {
            self.config.tools.is_enabled(kind)
                && !paths.effective_harness_path(kind, &self.config).exists()
        });

        let env_context = paths.load_env_context();
        let mut status = crate::domain::reconcile::evaluate_convergence_with_env(
            &self.mcp.mcps,
            &enabled_servers,
            Some(&env_context),
        );
        if let crate::domain::reconcile::ConvergenceStatus::Converged = status
            && any_missing_file
        {
            status = crate::domain::reconcile::ConvergenceStatus::Drift {
                drift_harnesses: Vec::new(),
                imported_count: 0,
            };
        }
        status
    }

    /// Performs headless sync across all enabled harnesses.
    /// If `force` is true, authoritatively pushes `mcp.toml` to all harnesses.
    /// If `force` is false, runs safe two-way reconciliation and returns `Err(conflicts)` if conflicts exist.
    /// Returns `Ok(true)` if files were written, or `Ok(false)` if already converged (idempotent skip).
    pub fn sync_headless(
        &mut self,
        paths: &StoragePaths,
        force: bool,
    ) -> Result<bool, HeadlessSyncError> {
        let enabled_servers = self.enabled_harness_servers();

        if force {
            let canonical = self.mcp.mcps.clone();
            self.save_all(paths, &canonical)
                .map_err(HeadlessSyncError::Io)?;
            return Ok(true);
        }

        let any_missing_file = HarnessKind::ALL.into_iter().any(|kind| {
            self.config.tools.is_enabled(kind)
                && !paths.effective_harness_path(kind, &self.config).exists()
        });

        let env_context = paths.load_env_context();
        let outcome = crate::domain::reconcile::reconcile_harnesses_with_env(
            &self.mcp.mcps,
            &enabled_servers,
            Some(&env_context),
        );
        if !outcome.conflicts.is_empty() {
            return Err(HeadlessSyncError::Conflicted(outcome.conflicts));
        }

        let is_converged = !any_missing_file
            && !self.mcp_had_spaced_keys
            && self.mcp.mcps == outcome.canonical
            && enabled_servers.iter().all(|(k, map)| {
                crate::domain::reconcile::harness_maps_match_with_env(
                    map,
                    &outcome.canonical,
                    Some(&env_context),
                ) && !self.harness_has_raw_spaced_keys(*k)
            });

        if is_converged && self.config_exists && !self.config_needs_save {
            return Ok(false);
        }

        self.save_all(paths, &outcome.canonical)
            .map_err(HeadlessSyncError::Io)?;
        Ok(true)
    }
}

/// Errors returned by `sync_headless`.
#[derive(Debug)]
pub enum HeadlessSyncError {
    Conflicted(Vec<crate::domain::reconcile::McpConflict>),
    Io(String),
}

/// Writes `content` to `target` atomically via a temporary file in the same directory,
/// creating a `<filename>.bak` backup first if `target` already exists.
/// If `target` is a symbolic link, resolves the underlying target path (`fs::canonicalize`)
/// so the symbolic link itself is never replaced with a regular file.
pub fn write_atomic_with_backup(target: &Path, content: &str) -> Result<(), String> {
    let write_target = match fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(target).or_else(|_| -> Result<PathBuf, String> {
                let link_dest = fs::read_link(target)
                    .map_err(|e| format!("Failed to read symlink {}: {e}", target.display()))?;
                if link_dest.is_absolute() {
                    Ok(link_dest)
                } else {
                    Ok(target
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(link_dest))
                }
            })?
        }
        _ => target.to_path_buf(),
    };

    let parent = write_target
        .parent()
        .ok_or_else(|| format!("Invalid target path {}", write_target.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create directory {}: {e}", parent.display()))?;

    if write_target.exists() {
        let mut bak_os = target.as_os_str().to_os_string();
        bak_os.push(".bak");
        let bak_path = PathBuf::from(bak_os);
        if let Some(bak_parent) = bak_path.parent() {
            let _ = fs::create_dir_all(bak_parent);
        }
        fs::copy(&write_target, &bak_path)
            .map_err(|e| format!("Failed to create backup {}: {e}", bak_path.display()))?;
    }

    let mut tmp = NamedTempFile::new_in(parent)
        .map_err(|e| format!("Failed to create temp file in {}: {e}", parent.display()))?;

    #[cfg(unix)]
    if write_target.exists()
        && let Ok(meta) = fs::metadata(&write_target)
    {
        let _ = fs::set_permissions(tmp.path(), meta.permissions());
    }

    tmp.write_all(content.as_bytes()).map_err(|e| {
        format!(
            "Failed to write temp file for {}: {e}",
            write_target.display()
        )
    })?;
    tmp.flush().map_err(|e| {
        format!(
            "Failed to flush temp file for {}: {e}",
            write_target.display()
        )
    })?;

    tmp.persist(&write_target).map_err(|e| {
        format!(
            "Failed to atomically persist {}: {e}",
            write_target.display()
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_refs_paths_and_persists_refs_registry() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        assert_eq!(paths.refs_toml, dir.path().join("refs.toml"));
        assert_eq!(paths.refs_cache_dir, dir.path().join("refs"));

        let mut configs = LoadedConfigs::load(&paths).expect("load configs");
        assert!(!configs.refs_exists);
        assert!(configs.refs.refs.is_empty());

        configs.refs.refs.insert(
            "pangolin".to_string(),
            crate::domain::refs::RefRepoEntry {
                url: "https://github.com/fosrl/docs".to_string(),
                branch: Some("main".to_string()),
                description: Some("Pangolin documentation".to_string()),
                path: None,
            },
        );

        configs.save_refs(&paths).expect("save refs");
        assert!(paths.refs_toml.exists());

        let reloaded = LoadedConfigs::load(&paths).expect("reload configs");
        assert!(reloaded.refs_exists);
        assert_eq!(reloaded.refs.refs.len(), 1);
        assert_eq!(
            reloaded.refs.refs.get("pangolin").unwrap().url,
            "https://github.com/fosrl/docs"
        );
    }

    #[test]
    fn atomic_write_creates_backup_and_replaces_file() {
        let dir = tempdir().expect("create tempdir");
        let file_path = dir.path().join("config.json");
        let bak_path = dir.path().join("config.json.bak");

        // First write (no backup yet)
        write_atomic_with_backup(&file_path, "{\"v\": 1}").expect("first write");
        assert!(file_path.exists());
        assert!(!bak_path.exists());
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "{\"v\": 1}");

        // Second write (creates .bak of v1, writes v2)
        write_atomic_with_backup(&file_path, "{\"v\": 2}").expect("second write");
        assert!(bak_path.exists());
        assert_eq!(fs::read_to_string(&bak_path).unwrap(), "{\"v\": 1}");
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "{\"v\": 2}");
    }

    #[test]
    fn resolves_plugin_paths_and_preserves_symlinks_on_atomic_write() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let cfg = MagaConfig::default();

        assert_eq!(paths.plugins_toml, dir.path().join("plugins.toml"));
        assert_eq!(
            paths.effective_agy_plugin_config_path(&cfg),
            dir.path().join("config.json")
        );
        assert_eq!(
            paths.effective_agy_plugins_dir(&cfg),
            dir.path().join("plugins")
        );
        assert_eq!(
            paths.effective_claude_settings_path(&cfg),
            dir.path().join(".claude").join("settings.json")
        );
        assert_eq!(
            paths.effective_claude_installed_plugins_path(&cfg),
            dir.path()
                .join(".claude")
                .join("plugins")
                .join("installed_plugins.json")
        );
        assert_eq!(
            paths.effective_copilot_plugin_config_path(&cfg),
            dir.path().join("config.json")
        );
        assert_eq!(
            paths.effective_grok_installed_plugins_dir(&cfg),
            dir.path().join("installed-plugins")
        );
        assert_eq!(
            paths.effective_opencode_plugins_dir(&cfg),
            dir.path().join("plugins")
        );

        #[cfg(unix)]
        {
            let real_dir = dir.path().join("dotfiles");
            fs::create_dir_all(&real_dir).expect("create dotfiles dir");
            let real_file = real_dir.join("settings.json");
            fs::write(&real_file, "{\"enabledPlugins\": {\"a\": true}}").expect("write real file");

            let claude_dir = dir.path().join(".claude");
            fs::create_dir_all(&claude_dir).expect("create .claude dir");
            let symlink_path = claude_dir.join("settings.json");
            std::os::unix::fs::symlink(&real_file, &symlink_path).expect("create symlink");

            write_atomic_with_backup(&symlink_path, "{\"enabledPlugins\": {\"a\": false}}")
                .expect("atomic write through symlink");

            let meta = fs::symlink_metadata(&symlink_path).expect("symlink metadata");
            assert!(meta.file_type().is_symlink());
            assert_eq!(
                fs::read_to_string(&real_file).unwrap(),
                "{\"enabledPlugins\": {\"a\": false}}"
            );
            assert_eq!(
                fs::read_to_string(&symlink_path).unwrap(),
                "{\"enabledPlugins\": {\"a\": false}}"
            );
            assert!(claude_dir.join("settings.json.bak").exists());
        }
    }

    #[test]
    fn migrates_legacy_mcps_from_config_toml_into_mcp_toml() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        let legacy_config = r#"
theme = "minimal"

[mcps.context7]
enabled = true
transport = "http"
url = "https://mcp.context7.com/mcp"
"#;
        fs::write(&paths.config_toml, legacy_config).expect("write legacy config.toml");

        let mut loaded = LoadedConfigs::load(&paths).expect("load configs with legacy migration");
        assert!(loaded.config_exists);
        assert!(loaded.config_needs_save);
        assert!(!loaded.mcp_exists);
        assert!(loaded.mcp.mcps.contains_key("context7"));
        assert!(!loaded.config.settings.contains_key("mcps"));

        let canonical = loaded.mcp.mcps.clone();
        loaded
            .save_all(&paths, &canonical)
            .expect("save migrated configs");

        assert!(paths.mcp_toml.exists());
        assert!(dir.path().join("config.toml.bak").exists());
        let saved_config =
            fs::read_to_string(&paths.config_toml).expect("read cleaned config.toml");
        assert!(!saved_config.contains("[mcps"));
        assert!(saved_config.contains("theme = \"minimal\""));

        let saved_mcp = fs::read_to_string(&paths.mcp_toml).expect("read new mcp.toml");
        assert!(saved_mcp.contains("[mcps.context7]"));
    }

    #[test]
    fn preserves_general_config_toml_across_mcp_syncs() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        // Partial config.toml missing [refresh] and [tools] is marked for auto-population
        fs::write(&paths.config_toml, "editor = \"nvim\"\n").expect("write config.toml");

        let mut loaded = LoadedConfigs::load(&paths).expect("load configs");
        assert!(loaded.config_needs_save);

        let mut canonical = BTreeMap::new();
        canonical.insert(
            "sqlite-mcp".to_string(),
            McpServerConfig::new_stdio("/usr/bin/sqlite-mcp", vec!["mcp".to_string()]),
        );
        loaded.save_all(&paths, &canonical).expect("save all");

        let populated = fs::read_to_string(&paths.config_toml).unwrap();
        assert!(populated.contains("editor = \"nvim\""));
        assert!(populated.contains("[refresh]"));
        assert!(populated.contains("[tools.antigravity]"));
        assert!(populated.contains("config_path = \"~/.gemini/config/mcp_config.json\""));
        assert!(populated.contains("[tools.claude]"));
        assert!(populated.contains("config_path = \"~/.claude.json\""));
        assert!(populated.contains("[tools.copilot]"));
        assert!(populated.contains("config_path = \"~/.copilot/mcp-config.json\""));
        assert!(populated.contains("[tools.grok]"));
        assert!(populated.contains("config_path = \"~/.grok/config.toml\""));
        assert!(populated.contains("[tools.opencode]"));
        assert!(populated.contains("config_path = \"~/.config/opencode/opencode.json\""));
        assert!(paths.mcp_toml.exists());

        // Remove the .bak created during initial auto-population and verify subsequent MCP syncs
        // do not rewrite an already-populated config.toml
        let bak_path = dir.path().join("config.toml.bak");
        let _ = fs::remove_file(&bak_path);

        let mut reloaded = LoadedConfigs::load(&paths).expect("reload populated configs");
        assert!(!reloaded.config_needs_save);
        reloaded
            .save_all(&paths, &canonical)
            .expect("save all on populated config");
        assert!(!bak_path.exists());
        assert_eq!(fs::read_to_string(&paths.config_toml).unwrap(), populated);
    }

    #[test]
    fn respects_tools_config_path_cli_precedence_and_disabled_harnesses() {
        let dir = tempdir().expect("create tempdir");
        let custom_agy = dir.path().join("custom_agy.json");
        let cli_agy = dir.path().join("cli_agy.json");
        let custom_copilot = dir.path().join("custom_copilot.json");
        let cli_copilot = dir.path().join("cli_copilot.json");
        let custom_grok = dir.path().join("custom_grok.toml");
        let custom_opencode = dir.path().join("custom_opencode.json");
        let default_claude = dir.path().join(".claude.json");

        let mut paths = StoragePaths::in_dir(dir.path());
        paths.agy_json = dir.path().join("default_agy.json");
        paths.claude_json = default_claude.clone();

        let config_content = format!(
            r#"
[tools.antigravity]
enabled = true
config_path = '{}'

[tools.claude]
enabled = false

[tools.copilot]
enabled = true
config_path = '{}'

[tools.grok]
enabled = true
config_path = '{}'

[tools.opencode]
enabled = true
config_path = '{}'
"#,
            custom_agy.display(),
            custom_copilot.display(),
            custom_grok.display(),
            custom_opencode.display(),
        );
        fs::write(&paths.config_toml, config_content).expect("write config.toml");
        fs::write(
            &default_claude,
            r#"{"mcpServers": {"ignored": {"type": "http", "url": "https://ignored.example.com/mcp"}}}"#,
        )
        .expect("write claude config");

        let mut loaded = LoadedConfigs::load(&paths).expect("load configs");
        // Claude is disabled in [tools.claude], so its servers are not loaded
        assert!(loaded.claude.servers.is_empty());
        assert_eq!(paths.effective_copilot_path(&loaded.config), custom_copilot);
        assert_eq!(paths.effective_grok_path(&loaded.config), custom_grok);
        assert_eq!(
            paths.effective_opencode_path(&loaded.config),
            custom_opencode
        );

        let mut canonical = BTreeMap::new();
        canonical.insert(
            "context7".to_string(),
            McpServerConfig::new_http("https://mcp.context7.com/mcp"),
        );
        loaded.save_all(&paths, &canonical).expect("save all");

        // Antigravity, Copilot, Grok, and OpenCode wrote to their custom paths; Claude was not modified
        assert!(custom_agy.exists());
        assert!(custom_copilot.exists());
        assert!(custom_grok.exists());
        assert!(custom_opencode.exists());
        assert!(!paths.agy_json.exists());
        assert!(!dir.path().join(".claude.json.bak").exists());

        // Reload and verify all 4 enabled harnesses round-tripped "context7"
        let reloaded = LoadedConfigs::load(&paths).expect("reload 4 harnesses");
        assert!(reloaded.agy.servers.contains_key("context7"));
        assert!(reloaded.copilot.servers.contains_key("context7"));
        assert!(reloaded.grok.servers.contains_key("context7"));
        assert!(reloaded.opencode.servers.contains_key("context7"));

        // When CLI flag overrides agy_json or copilot_json, CLI path wins over tools.<harness>.config_path
        paths.agy_json = cli_agy.clone();
        paths.agy_overridden = true;
        paths.copilot_json = cli_copilot.clone();
        paths.copilot_overridden = true;
        assert_eq!(paths.effective_copilot_path(&loaded.config), cli_copilot);
        loaded
            .save_all(&paths, &canonical)
            .expect("save all with CLI override");
        assert!(cli_agy.exists());
        assert!(cli_copilot.exists());
        assert!(custom_grok.with_extension("toml.bak").exists());
    }

    #[test]
    fn detects_and_rewrites_spaced_server_names_in_mcp_toml_and_harnesses() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        fs::write(
            &paths.mcp_toml,
            r#"
[mcps."Astro Docs"]
enabled = true
transport = "http"
url = "https://mcp.docs.astro.build/mcp"
"#,
        )
        .expect("write spaced mcp.toml");

        fs::write(
            &paths.agy_json,
            r#"{"mcpServers": {"Astro Docs": {"type": "http", "url": "https://mcp.docs.astro.build/mcp"}}}"#,
        )
        .expect("write spaced agy_json");

        let mut loaded = LoadedConfigs::load(&paths).expect("load spaced configs");
        assert!(loaded.mcp_had_spaced_keys);
        assert!(loaded.harness_has_raw_spaced_keys(HarnessKind::Antigravity));
        assert!(loaded.mcp.mcps.contains_key("Astro-Docs"));
        assert!(loaded.agy.servers.contains_key("Astro-Docs"));

        let canonical = loaded.mcp.mcps.clone();
        loaded
            .save_all(&paths, &canonical)
            .expect("save normalized configs");

        assert!(!loaded.mcp_had_spaced_keys);
        assert!(!loaded.harness_has_raw_spaced_keys(HarnessKind::Antigravity));
        let mcp_text = fs::read_to_string(&paths.mcp_toml).unwrap();
        assert!(!mcp_text.contains("Astro Docs"));
        assert!(mcp_text.contains("[mcps.Astro-Docs]"));

        let agy_text = fs::read_to_string(&paths.agy_json).unwrap();
        assert!(!agy_text.contains("Astro Docs"));
        assert!(agy_text.contains("\"Astro-Docs\""));
    }

    #[test]
    fn persists_description_in_mcp_toml_without_writing_to_harnesses() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        let mut loaded = LoadedConfigs::default();
        let mut canonical = BTreeMap::new();
        let mut sqlite = McpServerConfig::new_stdio("/usr/bin/sqlite-mcp", vec!["mcp".to_string()]);
        sqlite.description = Some("Local SQLite database MCP".to_string());
        canonical.insert("sqlite-mcp".to_string(), sqlite);

        loaded.save_all(&paths, &canonical).expect("save all");

        let mcp_text = fs::read_to_string(&paths.mcp_toml).unwrap();
        assert!(mcp_text.contains("description = \"Local SQLite database MCP\""));

        // External harness files do not receive unknown `description` keys
        for harness_file in [
            &paths.agy_json,
            &paths.claude_json,
            &paths.copilot_json,
            &paths.grok_toml,
            &paths.opencode_json,
        ] {
            let text = fs::read_to_string(harness_file).unwrap();
            assert!(!text.contains("description"));
        }

        let reloaded = LoadedConfigs::load(&paths).expect("reload");
        assert_eq!(
            reloaded.mcp.mcps["sqlite-mcp"].description.as_deref(),
            Some("Local SQLite database MCP")
        );
    }

    #[test]
    fn expands_tilde_with_forward_and_backslash_prefixes() {
        let home = std::env::home_dir().expect("home dir");
        assert_eq!(
            expand_tilde("~/.gemini/config/mcp_config.json"),
            home.join(".gemini/config/mcp_config.json")
        );
        assert_eq!(
            expand_tilde(r"~\.gemini\config\mcp_config.json"),
            home.join(r".gemini\config\mcp_config.json")
        );
    }

    #[test]
    fn headless_check_and_sync_idempotency_and_force() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        // Setup canonical mcp.toml with "sqlite"
        let mut loaded = LoadedConfigs::default();
        let server = McpServerConfig::new_stdio("/usr/bin/sqlite-mcp", vec!["mcp".to_string()]);
        loaded
            .mcp
            .mcps
            .insert("sqlite-mcp".to_string(), server.clone());
        loaded
            .save_all(&paths, &loaded.mcp.mcps.clone())
            .expect("save initial");

        // 1. Initial check after save_all -> Converged
        let status = loaded.check_headless(&paths);
        assert_eq!(
            status,
            crate::domain::reconcile::ConvergenceStatus::Converged
        );

        // 2. Headless sync when converged -> returns Ok(false) without rewriting files
        let bak_path = dir.path().join("mcp.toml.bak");
        let _ = fs::remove_file(&bak_path);
        let synced = loaded.sync_headless(&paths, false).expect("sync converged");
        assert!(!synced);
        assert!(!bak_path.exists());

        // 3. Harness is edited on disk (Claude adds a new untracked server) -> Drift
        let new_server = McpServerConfig::new_stdio("npx", vec!["filesystem".to_string()]);
        let claude_doc = crate::domain::claude::ClaudeDocument::default();
        let mut claude_map = BTreeMap::new();
        claude_map.insert("sqlite-mcp".to_string(), server.clone());
        claude_map.insert("filesystem".to_string(), new_server);
        let claude_str = claude_doc.to_json_string(&claude_map).unwrap();
        fs::write(&paths.claude_json, &claude_str).unwrap();

        let reloaded = LoadedConfigs::load(&paths).expect("reload");
        let status = reloaded.check_headless(&paths);
        assert!(matches!(
            status,
            crate::domain::reconcile::ConvergenceStatus::Drift { .. }
        ));

        // 4. Safe sync when there's definition drift on sqlite-mcp between mcp.toml and Claude -> conflict
        let mut conflicting = server.clone();
        conflicting.args.push("--conflict".to_string());
        claude_map.insert("sqlite-mcp".to_string(), conflicting);
        let claude_str2 = claude_doc.to_json_string(&claude_map).unwrap();
        fs::write(&paths.claude_json, &claude_str2).unwrap();

        let mut reloaded_conflicted = LoadedConfigs::load(&paths).expect("reload conflicted");
        let res = reloaded_conflicted.sync_headless(&paths, false);
        assert!(matches!(res, Err(HeadlessSyncError::Conflicted(_))));

        // 5. Force sync -> succeeds and pushes canonical mcp.toml
        let force_res = reloaded_conflicted
            .sync_headless(&paths, true)
            .expect("force sync");
        assert!(force_res);
        let check_after_force = reloaded_conflicted.check_headless(&paths);
        assert_eq!(
            check_after_force,
            crate::domain::reconcile::ConvergenceStatus::Converged
        );
    }

    #[test]
    fn cross_os_windows_and_debian_export_with_env_interpolation_and_idempotent_sync() {
        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        // Write .env.local in maga directory
        let env_file = dir.path().join(".env.local");
        fs::write(&env_file, "DB_PASS=UltraSecret123!\nDB_HOST=10.0.0.5\n").unwrap();

        let mut loaded = LoadedConfigs::default();
        let server = McpServerConfig::new_stdio(
            "dbhub",
            vec![
                "--transport".to_string(),
                "stdio".to_string(),
                "--dsn".to_string(),
                "sqlserver://sa:${DB_PASS}@${DB_HOST}:1433/App".to_string(),
            ],
        );
        loaded.mcp.mcps.insert("dbhub".to_string(), server);

        // 1. Export in Windows mode (is_windows = true)
        loaded
            .save_all_with_options(&paths, &loaded.mcp.mcps.clone(), true, None)
            .expect("save windows export");

        // Canonical mcp.toml MUST retain template and bare command
        let mcp_toml_content = fs::read_to_string(&paths.mcp_toml).expect("read mcp.toml");
        assert!(mcp_toml_content.contains("command = \"dbhub\""));
        assert!(mcp_toml_content.contains("${DB_PASS}"));
        assert!(!mcp_toml_content.contains("cmd"));
        assert!(!mcp_toml_content.contains("UltraSecret123!"));

        // Claude on disk MUST have cmd /c shim and interpolated secrets
        let claude_content = fs::read_to_string(&paths.claude_json).expect("read claude.json");
        assert!(claude_content.contains("\"command\": \"cmd\""));
        assert!(claude_content.contains("\"/c\""));
        assert!(claude_content.contains("sqlserver://sa:UltraSecret123!@10.0.0.5:1433/App"));

        // 2. Reloading from disk should unwrap shims and converge without false drift
        let mut reloaded = LoadedConfigs::load(&paths).expect("reload");
        assert_eq!(reloaded.mcp.mcps["dbhub"].command.as_deref(), Some("dbhub"));
        assert_eq!(
            reloaded.claude.servers["dbhub"].command.as_deref(),
            Some("dbhub")
        );

        let status = reloaded.check_headless(&paths);
        assert_eq!(
            status,
            crate::domain::reconcile::ConvergenceStatus::Converged
        );

        // 3. Headless safe sync is idempotent
        let synced = reloaded
            .sync_headless(&paths, false)
            .expect("sync headless");
        assert!(!synced, "Converged state should not trigger file rewrites");

        // 4. Export in Debian / Unix mode (is_windows = false)
        reloaded
            .save_all_with_options(&paths, &reloaded.mcp.mcps.clone(), false, None)
            .expect("save debian export");

        let claude_debian = fs::read_to_string(&paths.claude_json).expect("read claude.json");
        assert!(claude_debian.contains("\"command\": \"dbhub\""));
        assert!(!claude_debian.contains("\"/c\""));
        assert!(claude_debian.contains("UltraSecret123!"));

        let reloaded_debian = LoadedConfigs::load(&paths).expect("reload debian");
        let status_debian = reloaded_debian.check_headless(&paths);
        assert_eq!(
            status_debian,
            crate::domain::reconcile::ConvergenceStatus::Converged
        );
    }
}
