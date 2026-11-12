use crate::domain::model::{
    HarnessKind, MagaConfig, MascotMode, McpServerConfig, ProjectPolicy, SingleServerSnippet,
    ThemeName, Transport, normalize_mcp_description, validate_mcp_server_name,
};
use crate::domain::plugins::{
    HarnessPlugin, discover_all_harness_plugins, remove_harness_plugin, toggle_harness_plugin,
};
use crate::domain::probe::{ProbeEvent, ServerProbeState, spawn_probe_worker};
use crate::domain::reconcile::{
    ConflictResolution, McpConflict, ReconcileOutcome, harness_maps_match_with_env,
    reconcile_harnesses_with_env, reconcile_live_reload_harnesses_with_env,
};
use crate::domain::storage::{LoadedConfigs, StoragePaths};
use crate::fx::{FxEngine, FxTrigger};
use crate::keybind::{Action, map_workspace_key};
use crate::mascot::{MascotAnimState, MascotReaction};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// Active workspace view tab (`MCPs` vs. `Plugins` vs. `Guidelines` vs. `Refs`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceSection {
    #[default]
    Mcps,
    Plugins,
    Guidelines,
    Refs,
}

impl WorkspaceSection {
    pub fn toggle(self) -> Self {
        match self {
            Self::Mcps => Self::Plugins,
            Self::Plugins => Self::Guidelines,
            Self::Guidelines => Self::Refs,
            Self::Refs => Self::Mcps,
        }
    }
}

/// Virtual row in the `PLUGINS` Grouped Accordion list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum PluginRow {
    HarnessHeader {
        harness: HarnessKind,
        enabled_count: usize,
        total_count: usize,
        collapsed: bool,
    },
    Plugin {
        harness: HarnessKind,
        index: usize,
        plugin: HarnessPlugin,
    },
}

/// Active modal / screen mode in `maga`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveMode {
    Normal,
    Filtering,
    Form(FormState),
    Options(OptionsState),
    ConflictModal {
        index: usize,
    },
    ConfirmDelete {
        name: String,
    },
    ConfirmDeletePlugin {
        harness: HarnessKind,
        id: String,
        display_name: String,
        install_path: Option<PathBuf>,
    },
    ProfilesModal {
        selected: usize,
        items: Vec<String>,
    },
    MatrixModal {
        server_name: String,
        selected: usize,
    },
    NewProfileModal {
        input: String,
    },
    Help,
}

/// Focused field in the interactive Options modal (`o` keybinding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsField {
    Theme,
    MascotMode,
    Background,
    HeroMascot,
    HideUi,
    Animations,
    AutoRefresh,
    RefreshInterval,
    Antigravity,
    Claude,
    Copilot,
    Grok,
    OpenCode,
}

impl OptionsField {
    pub const ALL: [Self; 13] = [
        Self::Theme,
        Self::MascotMode,
        Self::Background,
        Self::HeroMascot,
        Self::HideUi,
        Self::Animations,
        Self::AutoRefresh,
        Self::RefreshInterval,
        Self::Antigravity,
        Self::Claude,
        Self::Copilot,
        Self::Grok,
        Self::OpenCode,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        if idx == 0 {
            Self::ALL[Self::ALL.len() - 1]
        } else {
            Self::ALL[idx - 1]
        }
    }
}

/// Editable state of the in-TUI Options modal (`o` keybinding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsState {
    pub theme: ThemeName,
    pub mascot_mode: MascotMode,
    pub background: bool,
    pub hero_mascot: bool,
    pub hide_ui: bool,
    pub animations: bool,
    pub refresh_enabled: bool,
    pub refresh_interval_secs: u64,
    pub antigravity_enabled: bool,
    pub claude_enabled: bool,
    pub copilot_enabled: bool,
    pub grok_enabled: bool,
    pub opencode_enabled: bool,
    pub focused: OptionsField,
}

impl OptionsState {
    pub fn from_config(cfg: &MagaConfig) -> Self {
        Self {
            theme: cfg.ui.theme,
            mascot_mode: cfg.ui.mascot_mode,
            background: cfg.ui.background,
            hero_mascot: cfg.ui.hero_mascot,
            hide_ui: cfg.ui.hide_ui,
            animations: cfg.ui.animations,
            refresh_enabled: cfg.refresh.enabled,
            refresh_interval_secs: cfg.refresh.interval_secs.clamp(1, 300),
            antigravity_enabled: cfg.tools.antigravity.enabled,
            claude_enabled: cfg.tools.claude.enabled,
            copilot_enabled: cfg.tools.copilot.enabled,
            grok_enabled: cfg.tools.grok.enabled,
            opencode_enabled: cfg.tools.opencode.enabled,
            focused: OptionsField::Theme,
        }
    }

    pub fn step(&mut self, forward: bool) {
        match self.focused {
            OptionsField::Theme => {
                self.theme = if forward {
                    self.theme.next()
                } else {
                    self.theme.prev()
                };
            }
            OptionsField::MascotMode => {
                self.mascot_mode = self.mascot_mode.toggle();
            }
            OptionsField::Background => {
                self.background = !self.background;
            }
            OptionsField::HeroMascot => {
                self.hero_mascot = !self.hero_mascot;
            }
            OptionsField::HideUi => {
                self.hide_ui = !self.hide_ui;
            }
            OptionsField::Animations => {
                self.animations = !self.animations;
            }
            OptionsField::AutoRefresh => {
                self.refresh_enabled = !self.refresh_enabled;
            }
            OptionsField::RefreshInterval => {
                if forward {
                    self.refresh_interval_secs = (self.refresh_interval_secs + 1).min(300);
                } else {
                    self.refresh_interval_secs =
                        self.refresh_interval_secs.saturating_sub(1).max(1);
                }
            }
            OptionsField::Antigravity => {
                self.antigravity_enabled = !self.antigravity_enabled;
            }
            OptionsField::Claude => {
                self.claude_enabled = !self.claude_enabled;
            }
            OptionsField::Copilot => {
                self.copilot_enabled = !self.copilot_enabled;
            }
            OptionsField::Grok => {
                self.grok_enabled = !self.grok_enabled;
            }
            OptionsField::OpenCode => {
                self.opencode_enabled = !self.opencode_enabled;
            }
        }
    }

    /// Applies the edited options to `MagaConfig` and returns `true` if any harness toggles changed.
    pub fn apply_to(&self, cfg: &mut MagaConfig) -> bool {
        let tools_changed = cfg.tools.antigravity.enabled != self.antigravity_enabled
            || cfg.tools.claude.enabled != self.claude_enabled
            || cfg.tools.copilot.enabled != self.copilot_enabled
            || cfg.tools.grok.enabled != self.grok_enabled
            || cfg.tools.opencode.enabled != self.opencode_enabled;

        cfg.ui.theme = self.theme;
        cfg.ui.mascot_mode = self.mascot_mode;
        cfg.ui.background = self.background;
        cfg.ui.hero_mascot = self.hero_mascot;
        cfg.ui.hide_ui = self.hide_ui;
        cfg.ui.animations = self.animations;
        cfg.refresh.enabled = self.refresh_enabled;
        cfg.refresh.interval_secs = self.refresh_interval_secs.clamp(1, 300);
        cfg.tools.antigravity.enabled = self.antigravity_enabled;
        cfg.tools.claude.enabled = self.claude_enabled;
        cfg.tools.copilot.enabled = self.copilot_enabled;
        cfg.tools.grok.enabled = self.grok_enabled;
        cfg.tools.opencode.enabled = self.opencode_enabled;
        tools_changed
    }
}

/// Focused field in the guided Add/Edit modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Name,
    Description,
    Transport,
    Endpoint,
    Args,
    Pairs,
}

impl FormField {
    pub fn next(self, transport: Transport) -> Self {
        match (self, transport) {
            (Self::Name, _) => Self::Description,
            (Self::Description, _) => Self::Transport,
            (Self::Transport, _) => Self::Endpoint,
            (Self::Endpoint, Transport::Stdio) => Self::Args,
            (Self::Endpoint, Transport::Http) => Self::Pairs,
            (Self::Args, _) => Self::Pairs,
            (Self::Pairs, _) => Self::Name,
        }
    }

    pub fn prev(self, transport: Transport) -> Self {
        match (self, transport) {
            (Self::Name, _) => Self::Pairs,
            (Self::Description, _) => Self::Name,
            (Self::Transport, _) => Self::Description,
            (Self::Endpoint, _) => Self::Transport,
            (Self::Args, _) => Self::Endpoint,
            (Self::Pairs, Transport::Stdio) => Self::Args,
            (Self::Pairs, Transport::Http) => Self::Endpoint,
        }
    }
}

/// State of the guided Add/Edit MCP modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormState {
    pub original_name: Option<String>,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub transport: Transport,
    pub endpoint: String,
    pub args_input: String,
    pub pairs_input: String,
    pub focused: FormField,
    pub error: Option<String>,
}

impl FormState {
    pub fn new_add() -> Self {
        Self {
            original_name: None,
            name: String::new(),
            description: String::new(),
            enabled: true,
            transport: Transport::Stdio,
            endpoint: String::new(),
            args_input: String::new(),
            pairs_input: String::new(),
            focused: FormField::Name,
            error: None,
        }
    }

    pub fn from_existing(name: &str, cfg: &McpServerConfig) -> Self {
        let endpoint = match cfg.transport {
            Transport::Stdio => cfg.command.clone().unwrap_or_default(),
            Transport::Http => cfg.url.clone().unwrap_or_default(),
        };
        let args_input = cfg.args.join(" ");
        let map = match cfg.transport {
            Transport::Stdio => &cfg.env,
            Transport::Http => &cfg.headers,
        };
        let pairs_input = map
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");

        Self {
            original_name: Some(name.to_string()),
            name: name.to_string(),
            description: cfg.description.clone().unwrap_or_default(),
            enabled: cfg.enabled,
            transport: cfg.transport,
            endpoint,
            args_input,
            pairs_input,
            focused: FormField::Endpoint,
            error: None,
        }
    }

    pub fn build_config(&self) -> Result<(String, McpServerConfig), String> {
        let trimmed_name = self.name.trim().to_string();
        if trimmed_name.is_empty() {
            return Err("Server `name` is required.".to_string());
        }
        validate_mcp_server_name(&trimmed_name)?;
        let trimmed_endpoint = self.endpoint.trim().to_string();
        let pairs = parse_key_value_pairs(&self.pairs_input)?;
        let description = normalize_mcp_description(Some(self.description.clone()));

        let cfg = match self.transport {
            Transport::Stdio => {
                let args = self
                    .args_input
                    .split_whitespace()
                    .map(ToString::to_string)
                    .collect();
                let mut s = McpServerConfig::new_stdio(trimmed_endpoint, args);
                s.enabled = self.enabled;
                s.description = description;
                s.env = pairs;
                s
            }
            Transport::Http => {
                let mut s = McpServerConfig::new_http(trimmed_endpoint);
                s.enabled = self.enabled;
                s.description = description;
                s.headers = pairs;
                s
            }
        };

        cfg.validate()?;
        Ok((trimmed_name, cfg))
    }
}

fn parse_key_value_pairs(input: &str) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    if input.trim().is_empty() {
        return Ok(map);
    }
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=').ok_or_else(|| {
            format!("Invalid KEY=VALUE pair: '{part}' (separate multiple with commas)")
        })?;
        let key = k.trim();
        if key.is_empty() {
            return Err("Environment/Header key cannot be empty.".to_string());
        }
        map.insert(key.to_string(), v.trim().to_string());
    }
    Ok(map)
}

fn server_matches_filter_token(
    token: &str,
    name: &str,
    cfg: &McpServerConfig,
    conflicted: bool,
) -> bool {
    let desc_lower = cfg
        .description
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if let Some((key, val)) = token.split_once(':') {
        match key {
            "is" | "status" => {
                return val.is_empty() || matches_status_filter(val, cfg.enabled, conflicted, true);
            }
            "type" | "transport" => {
                return val.is_empty() || cfg.transport.as_str().contains(val);
            }
            "cmd" | "url" | "endpoint" => {
                return val.is_empty() || cfg.endpoint_summary().to_ascii_lowercase().contains(val);
            }
            "name" => {
                return val.is_empty() || name.to_ascii_lowercase().contains(val);
            }
            "desc" | "description" => {
                return val.is_empty() || desc_lower.contains(val);
            }
            _ => {}
        }
    }

    name.to_ascii_lowercase().contains(token)
        || (!desc_lower.is_empty() && desc_lower.contains(token))
        || cfg.transport.as_str().contains(token)
        || cfg.endpoint_summary().to_ascii_lowercase().contains(token)
        || matches_status_filter(token, cfg.enabled, conflicted, false)
}

fn matches_status_filter(
    val: &str,
    enabled: bool,
    conflicted: bool,
    explicit_qualifier: bool,
) -> bool {
    if val.is_empty() {
        return false;
    }
    let allow_prefix = explicit_qualifier || val.len() >= 3;
    let enabled_match = enabled
        && (val == "on"
            || (allow_prefix && ("enabled".starts_with(val) || "active".starts_with(val))));
    let disabled_match = !enabled
        && (val == "off"
            || (allow_prefix && ("disabled".starts_with(val) || "inactive".starts_with(val))));
    let conflict_match = conflicted
        && allow_prefix
        && ("conflict".starts_with(val) || "conflicted".starts_with(val));

    enabled_match || disabled_match || conflict_match
}

fn plugin_matches_filter_token(token: &str, plugin: &HarnessPlugin) -> bool {
    let id_lower = plugin.id.to_ascii_lowercase();
    let display_lower = plugin.display_name.to_ascii_lowercase();
    let harness_lower = plugin.harness.display_name().to_ascii_lowercase();
    let harness_short_lower = plugin.harness.short_name().to_ascii_lowercase();
    let kind_str = plugin.kind.as_str();
    let source_lower = plugin.source.as_deref().unwrap_or("").to_ascii_lowercase();
    let desc_lower = plugin
        .description
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();

    if let Some((key, val)) = token.split_once(':') {
        match key {
            "is" | "status" => {
                return val.is_empty() || matches_status_filter(val, plugin.enabled, false, true);
            }
            "harness" | "tool" => {
                return val.is_empty()
                    || harness_lower.contains(val)
                    || harness_short_lower.contains(val);
            }
            "type" | "kind" | "source" => {
                return val.is_empty() || kind_str.contains(val) || source_lower.contains(val);
            }
            "name" | "id" => {
                return val.is_empty() || id_lower.contains(val) || display_lower.contains(val);
            }
            "desc" | "description" => {
                return val.is_empty() || desc_lower.contains(val);
            }
            "skill" | "skills" => {
                return val.is_empty()
                    || plugin
                        .bundled
                        .skill_names
                        .iter()
                        .any(|s| s.to_ascii_lowercase().contains(val));
            }
            "hook" | "hooks" => {
                return val.is_empty()
                    || plugin
                        .bundled
                        .hook_names
                        .iter()
                        .any(|h| h.to_ascii_lowercase().contains(val));
            }
            "mcp" | "mcps" => {
                return val.is_empty()
                    || plugin
                        .bundled
                        .mcp_names
                        .iter()
                        .any(|m| m.to_ascii_lowercase().contains(val));
            }
            _ => {}
        }
    }

    id_lower.contains(token)
        || display_lower.contains(token)
        || harness_lower.contains(token)
        || harness_short_lower.contains(token)
        || kind_str.contains(token)
        || (!source_lower.is_empty() && source_lower.contains(token))
        || (!desc_lower.is_empty() && desc_lower.contains(token))
        || plugin
            .bundled
            .skill_names
            .iter()
            .any(|s| s.to_ascii_lowercase().contains(token))
        || plugin
            .bundled
            .hook_names
            .iter()
            .any(|h| h.to_ascii_lowercase().contains(token))
        || plugin
            .bundled
            .mcp_names
            .iter()
            .any(|m| m.to_ascii_lowercase().contains(token))
        || matches_status_filter(token, plugin.enabled, false, false)
}

/// Request emitted by `App` when the user invokes `$EDITOR` (`E`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorRequest {
    pub original_name: Option<String>,
    pub conflict_index: Option<usize>,
    pub initial_toml: String,
    pub direct_file_path: Option<PathBuf>,
}

/// Main application state.
pub struct App {
    pub should_quit: bool,
    pub width: u16,
    pub height: u16,
    pub paths: Option<StoragePaths>,
    pub loaded: LoadedConfigs,
    pub outcome: ReconcileOutcome,
    pub selected: usize,
    pub scroll_offset: usize,
    pub filter_query: String,
    pub section: WorkspaceSection,
    pub plugin_selected: usize,
    pub plugin_scroll_offset: usize,
    pub plugin_filter_query: String,
    pub collapsed_plugin_groups: BTreeSet<HarnessKind>,
    pub guidelines_statuses: Vec<crate::domain::guidelines::GuidelineStatus>,
    pub guidelines_selected: usize,
    pub refs_selected: usize,
    pub mode: ActiveMode,
    pub status_message: Option<(String, bool)>, // (message, is_error)
    pub pending_editor: Option<EditorRequest>,
    pub show_hero_mascot: bool,
    pub anim_tick: usize,
    pub anim_elapsed: Duration,
    pub gaze_target: (f32, f32),
    pub gaze_current: (f32, f32),
    pub last_input_elapsed: Duration,
    pub mascot_reaction: MascotReaction,
    pub reaction_remaining: Duration,
    pub reaction_total: Duration,
    pub fx: RefCell<FxEngine>,
    pub pending_fx: RefCell<Vec<FxTrigger>>,
    pub last_frame_delta: Duration,
    pub probe_states: BTreeMap<String, ServerProbeState>,
    pub probe_tx: mpsc::Sender<ProbeEvent>,
    pub probe_rx: mpsc::Receiver<ProbeEvent>,
    pub details_focused: bool,
    pub details_scroll: usize,
    pub help_scroll: usize,
    pub available_update: Arc<Mutex<Option<String>>>,
}

impl App {
    pub fn new(
        paths: Option<StoragePaths>,
        mut loaded: LoadedConfigs,
        width: u16,
        height: u16,
    ) -> Self {
        let env_ctx = paths.as_ref().map_or_else(
            crate::domain::model::load_default_env,
            StoragePaths::load_env_context,
        );
        let harnesses = loaded.enabled_harness_servers();
        let outcome = reconcile_harnesses_with_env(&loaded.mcp.mcps, &harnesses, Some(&env_ctx));
        let mut initial_fx = vec![FxTrigger::Startup];
        let initial_mode = if !outcome.conflicts.is_empty() {
            initial_fx.push(FxTrigger::ConflictAlert);
            ActiveMode::ConflictModal { index: 0 }
        } else {
            ActiveMode::Normal
        };

        let mut status_message = None;
        if let Some(ref p) = paths {
            if outcome.conflicts.is_empty() {
                if let Err(e) = loaded.save_all(p, &outcome.canonical) {
                    status_message = Some((format!("✗ Sync failed: {e}"), true));
                } else if outcome.imported_count > 0 {
                    status_message = Some((
                        format!(
                            "✓ Imported {} untracked MCP(s) and synced all configs",
                            outcome.imported_count
                        ),
                        false,
                    ));
                }
            } else {
                if (!loaded.config_exists || loaded.config_needs_save)
                    && let Err(e) = loaded.save_config(p)
                {
                    status_message = Some((format!("✗ Failed to save config.toml: {e}"), true));
                }
                if loaded.plugins_ledger_needs_save
                    && let Err(e) = loaded.save_plugins_ledger(p)
                {
                    status_message = Some((format!("✗ Failed to save plugins.toml: {e}"), true));
                }
            }
        }

        let (initial_reaction, initial_reaction_dur) = if !outcome.conflicts.is_empty() {
            (MascotReaction::Startled, Duration::from_millis(900))
        } else {
            (MascotReaction::Idle, Duration::ZERO)
        };

        let (probe_tx, probe_rx) = mpsc::channel();

        let available_update = Arc::new(Mutex::new(None));
        let update_clone = Arc::clone(&available_update);
        std::thread::spawn(move || {
            if let Ok(info) = crate::domain::update::check_latest_release(crate::domain::update::DEFAULT_REPO) {
                if info.is_newer {
                    if let Ok(mut lock) = update_clone.lock() {
                        *lock = Some(info.latest_version);
                    }
                }
            }
        });

        let mut app = Self {
            should_quit: false,
            width,
            height,
            paths,
            loaded,
            outcome,
            selected: 0,
            scroll_offset: 0,
            filter_query: String::new(),
            section: WorkspaceSection::Mcps,
            plugin_selected: 0,
            plugin_scroll_offset: 0,
            plugin_filter_query: String::new(),
            collapsed_plugin_groups: BTreeSet::new(),
            guidelines_statuses: crate::domain::guidelines::check_guidelines_status().unwrap_or_default(),
            guidelines_selected: 0,
            refs_selected: 0,
            mode: initial_mode,
            status_message,
            pending_editor: None,
            show_hero_mascot: false,
            anim_tick: 0,
            anim_elapsed: Duration::ZERO,
            gaze_target: (0.0, 0.0),
            gaze_current: (0.0, 0.0),
            last_input_elapsed: Duration::ZERO,
            mascot_reaction: initial_reaction,
            reaction_remaining: initial_reaction_dur,
            reaction_total: initial_reaction_dur,
            fx: RefCell::new(FxEngine::new()),
            pending_fx: RefCell::new(initial_fx),
            last_frame_delta: Duration::ZERO,
            probe_states: BTreeMap::new(),
            probe_tx,
            probe_rx,
            details_focused: false,
            details_scroll: 0,
            help_scroll: 0,
            available_update,
        };
        app.recompute_focus_gaze_target();
        app.gaze_current = app.gaze_target;
        app
    }

    pub fn available_update(&self) -> Option<String> {
        self.available_update.lock().ok().and_then(|g| g.clone())
    }

    pub fn emit_fx(&self, trigger: FxTrigger) {
        if self.loaded.config.ui.animations {
            self.pending_fx.borrow_mut().push(trigger);
        }
    }

    pub fn take_pending_fx(&self) -> Vec<FxTrigger> {
        std::mem::take(&mut *self.pending_fx.borrow_mut())
    }

    pub fn set_frame_delta(&mut self, delta: Duration) {
        self.last_frame_delta = delta;
        if self.loaded.config.ui.animations {
            self.anim_elapsed = self.anim_elapsed.saturating_add(delta);
            self.last_input_elapsed = self.last_input_elapsed.saturating_add(delta);

            if !self.reaction_remaining.is_zero() {
                self.reaction_remaining = self.reaction_remaining.saturating_sub(delta);
                if self.reaction_remaining.is_zero() {
                    self.mascot_reaction = if !self.outcome.conflicts.is_empty()
                        || self
                            .status_message
                            .as_ref()
                            .is_some_and(|(_, is_err)| *is_err)
                    {
                        MascotReaction::Startled
                    } else if matches!(self.mode, ActiveMode::Filtering)
                        || !self.filter_query.is_empty()
                        || !self.plugin_filter_query.is_empty()
                    {
                        MascotReaction::Scrying
                    } else {
                        MascotReaction::Idle
                    };
                    self.reaction_total = Duration::ZERO;
                }
            }

            if self.last_input_elapsed >= Duration::from_millis(2500)
                || self.loaded.config.ui.hide_ui
            {
                let t = self.anim_elapsed.as_secs_f32();
                let idle_x = (t * 0.85).sin() * 0.48 + (t * 1.9).cos() * 0.14;
                let idle_y = (t * 0.62).cos() * 0.36 - 0.06;
                self.gaze_target = (idle_x.clamp(-0.65, 0.65), idle_y.clamp(-0.55, 0.45));
            } else {
                self.recompute_focus_gaze_target();
            }

            let alpha = (delta.as_secs_f32() * 10.0).clamp(0.05, 1.0);
            self.gaze_current.0 += (self.gaze_target.0 - self.gaze_current.0) * alpha;
            self.gaze_current.1 += (self.gaze_target.1 - self.gaze_current.1) * alpha;
        } else {
            self.anim_elapsed = Duration::ZERO;
            self.last_input_elapsed = Duration::ZERO;
            self.gaze_target = (0.0, 0.0);
            self.gaze_current = (0.0, 0.0);
            self.mascot_reaction = MascotReaction::Idle;
            self.reaction_remaining = Duration::ZERO;
            self.reaction_total = Duration::ZERO;
        }
    }

    /// Triggers a timed mascot reaction across Hero Showcase, Background, and Chibi Companion.
    pub fn trigger_mascot_reaction(&mut self, reaction: MascotReaction, duration: Duration) {
        if self.loaded.config.ui.animations {
            self.mascot_reaction = reaction;
            self.reaction_remaining = duration;
            self.reaction_total = duration;
        }
    }

    /// Recomputes `gaze_target` in `[-1.0, 1.0]` based on active keyboard focus (`mode`, `selected`, `scroll_offset`).
    pub fn recompute_focus_gaze_target(&mut self) {
        if self.loaded.config.ui.hide_ui {
            let t = self.anim_elapsed.as_secs_f32();
            self.gaze_target = ((t * 0.85).sin() * 0.48, (t * 0.62).cos() * 0.36);
            return;
        }
        match &self.mode {
            ActiveMode::Filtering => {
                // Look up-left toward the `/filter` query bar in the SERVERS header
                self.gaze_target = (-0.75, -0.75);
            }
            ActiveMode::Form(form) => {
                let fy = match form.focused {
                    FormField::Name => -0.50,
                    FormField::Description => -0.30,
                    FormField::Transport => -0.10,
                    FormField::Endpoint => 0.10,
                    FormField::Args => 0.30,
                    FormField::Pairs => 0.50,
                };
                self.gaze_target = (0.0, fy);
            }
            ActiveMode::Options(opt) => {
                let idx = OptionsField::ALL
                    .iter()
                    .position(|f| *f == opt.focused)
                    .unwrap_or(0);
                let ratio = idx as f32 / (OptionsField::ALL.len().saturating_sub(1).max(1) as f32);
                self.gaze_target = (0.0, (ratio * 1.1 - 0.55).clamp(-0.6, 0.6));
            }
            ActiveMode::ConflictModal { .. }
            | ActiveMode::ConfirmDelete { .. }
            | ActiveMode::ConfirmDeletePlugin { .. }
            | ActiveMode::ProfilesModal { .. }
            | ActiveMode::MatrixModal { .. }
            | ActiveMode::NewProfileModal { .. }
            | ActiveMode::Help => {
                self.gaze_target = (0.0, 0.0);
            }
            ActiveMode::Normal => {
                if self.details_focused {
                    let norm_y =
                        ((self.details_scroll.min(20) as f32 / 20.0) * 1.0 - 0.5).clamp(-0.6, 0.6);
                    self.gaze_target = (0.35, norm_y);
                } else {
                    let (visible_len, sel, off) = match self.section {
                        WorkspaceSection::Mcps => (
                            self.visible_servers().len(),
                            self.selected,
                            self.scroll_offset,
                        ),
                        WorkspaceSection::Plugins => (
                            self.visible_plugin_rows().len(),
                            self.plugin_selected,
                            self.plugin_scroll_offset,
                        ),
                        WorkspaceSection::Guidelines => (
                            self.guidelines_statuses.len(),
                            self.guidelines_selected,
                            0,
                        ),
                        WorkspaceSection::Refs => (
                            self.loaded.refs.refs.len(),
                            self.refs_selected,
                            0,
                        ),
                    };
                    if visible_len == 0 {
                        self.gaze_target = (-0.30, 0.0);
                    } else {
                        let vh = self.list_viewport_height().max(1);
                        let visible_rows = visible_len.saturating_sub(off).min(vh).max(1);
                        let rel_row = sel.saturating_sub(off);
                        let norm_y = if visible_rows <= 1 {
                            -0.25
                        } else {
                            (rel_row as f32 / (visible_rows - 1) as f32) * 1.4 - 0.7
                        };
                        self.gaze_target = (-0.65, norm_y.clamp(-0.75, 0.75));
                    }
                }
            }
        }
    }

    /// Returns the snapshot of `MascotAnimState` for the current frame.
    pub fn mascot_anim_state(&self) -> MascotAnimState {
        if !self.loaded.config.ui.animations {
            return MascotAnimState::default();
        }
        let active_reaction = if !self.reaction_remaining.is_zero() {
            self.mascot_reaction
        } else if !self.outcome.conflicts.is_empty()
            || self
                .status_message
                .as_ref()
                .is_some_and(|(_, is_err)| *is_err)
        {
            MascotReaction::Startled
        } else if matches!(self.mode, ActiveMode::Filtering)
            || !self.filter_query.is_empty()
            || !self.plugin_filter_query.is_empty()
            || self.has_active_probe()
        {
            MascotReaction::Scrying
        } else {
            MascotReaction::Idle
        };

        let reaction_progress =
            if !self.reaction_total.is_zero() && !self.reaction_remaining.is_zero() {
                let rem = self.reaction_remaining.as_secs_f32();
                let tot = self.reaction_total.as_secs_f32().max(0.001);
                (1.0 - (rem / tot)).clamp(0.0, 1.0)
            } else {
                0.0
            };

        MascotAnimState {
            gaze_x: self.gaze_current.0,
            gaze_y: self.gaze_current.1,
            last_input_elapsed: self.last_input_elapsed,
            reaction: active_reaction,
            reaction_progress,
        }
    }

    /// Returns `true` if any animated visual tier (Hero Showcase, Background Watermark, Background-Only, or Rich Companion) is currently visible.
    pub fn has_visible_animated_surface(&self) -> bool {
        if !self.loaded.config.ui.animations {
            return false;
        }
        let (mascot_mode, background, hero_mascot, hide_ui) = match &self.mode {
            ActiveMode::Options(opt) => (
                opt.mascot_mode,
                opt.background,
                opt.hero_mascot,
                opt.hide_ui,
            ),
            _ => (
                self.loaded.config.ui.mascot_mode,
                self.loaded.config.ui.background,
                self.loaded.config.ui.hero_mascot,
                self.loaded.config.ui.hide_ui,
            ),
        };
        let hero_visible =
            hero_mascot && (self.show_hero_mascot || self.outcome.canonical.is_empty());
        mascot_mode == MascotMode::Rich
            || background
            || hide_ui
            || hero_visible
            || self.has_active_probe()
    }

    pub fn has_active_probe(&self) -> bool {
        self.probe_states
            .values()
            .any(|s| matches!(s, ServerProbeState::Probing))
    }

    pub fn probe_state(&self, name: &str) -> Option<&ServerProbeState> {
        self.probe_states.get(name)
    }

    fn current_env_context(&self) -> BTreeMap<String, String> {
        self.paths.as_ref().map_or_else(
            crate::domain::model::load_default_env,
            StoragePaths::load_env_context,
        )
    }

    /// Launches a background MCP `initialize` + `tools/list` probe for the currently selected server (`p`).
    pub fn probe_selected(&mut self) {
        let Some((name, cfg, _)) = self.selected_server() else {
            self.status_message = Some(("○ No MCP server selected to probe".to_string(), false));
            return;
        };
        if matches!(
            self.probe_states.get(&name),
            Some(ServerProbeState::Probing)
        ) {
            self.status_message = Some((format!("↻ Already probing '{name}'…"), false));
            return;
        }
        let env_ctx = self.current_env_context();
        let probe_cfg = cfg.interpolate(&env_ctx);
        self.probe_states
            .insert(name.clone(), ServerProbeState::Probing);
        spawn_probe_worker(name.clone(), probe_cfg, self.probe_tx.clone());
        self.status_message = Some((
            format!("↻ Probing '{name}' (initialize + tools/list)…"),
            false,
        ));
        self.trigger_mascot_reaction(MascotReaction::Scrying, Duration::from_millis(900));
        self.emit_fx(FxTrigger::SelectionChanged);
    }

    /// Launches parallel background MCP health probes for all enabled canonical MCP servers (`P`).
    pub fn probe_all_enabled(&mut self) {
        let targets: Vec<(String, McpServerConfig)> = self
            .outcome
            .canonical
            .iter()
            .filter(|(_, cfg)| cfg.enabled)
            .map(|(n, cfg)| (n.clone(), cfg.clone()))
            .collect();

        if targets.is_empty() {
            self.status_message = Some(("○ No enabled MCP servers to probe".to_string(), false));
            return;
        }

        let env_ctx = self.current_env_context();
        let mut launched = 0usize;
        for (name, cfg) in targets {
            if matches!(
                self.probe_states.get(&name),
                Some(ServerProbeState::Probing)
            ) {
                continue;
            }
            let probe_cfg = cfg.interpolate(&env_ctx);
            self.probe_states
                .insert(name.clone(), ServerProbeState::Probing);
            spawn_probe_worker(name, probe_cfg, self.probe_tx.clone());
            launched += 1;
        }

        if launched > 0 {
            self.status_message = Some((
                format!("↻ Probing {launched} enabled MCP server(s) in background…"),
                false,
            ));
            self.trigger_mascot_reaction(MascotReaction::Scrying, Duration::from_millis(950));
            self.emit_fx(FxTrigger::Synced);
        } else {
            self.status_message = Some((
                "↻ All enabled MCP servers are already probing…".to_string(),
                false,
            ));
        }
    }

    /// Drains completed `ProbeEvent` messages from background probe worker threads.
    pub fn poll_probe_results(&mut self) -> bool {
        let mut any = false;
        while let Ok(ev) = self.probe_rx.try_recv() {
            any = true;
            match &ev.state {
                ServerProbeState::Online(outcome) => {
                    let tool_count = outcome.tools.len();
                    let tools_label = if tool_count == 1 {
                        "1 tool".to_string()
                    } else {
                        format!("{tool_count} tools")
                    };
                    self.status_message = Some((
                        format!(
                            "✓ '{}' online ({}ms · {tools_label})",
                            ev.name, outcome.latency_ms
                        ),
                        false,
                    ));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::Synced);
                }
                ServerProbeState::Failed { error, .. } => {
                    self.status_message =
                        Some((format!("✗ '{}' probe failed: {error}", ev.name), true));
                    self.trigger_mascot_reaction(
                        MascotReaction::Startled,
                        Duration::from_millis(850),
                    );
                    self.emit_fx(FxTrigger::ConflictAlert);
                }
                ServerProbeState::Probing => {}
            }
            self.probe_states.insert(ev.name, ev.state);
        }
        any
    }

    pub fn has_active_fx_transition(&self) -> bool {
        self.loaded.config.ui.animations
            && (self.fx.borrow().has_active_transition() || !self.pending_fx.borrow().is_empty())
    }

    pub fn animations_enabled(&self) -> bool {
        self.loaded.config.ui.animations
    }

    /// Resets `anim_elapsed` to zero so the Hero Mascot Showcase replays its radial arcane reveal.
    pub fn reset_mascot_reveal(&mut self) {
        self.anim_elapsed = Duration::ZERO;
    }

    pub fn advance_animation(&mut self, mono: bool) {
        if self.loaded.config.ui.animations && !mono {
            self.anim_tick = self.anim_tick.wrapping_add(1);
        } else {
            self.anim_tick = 0;
            self.anim_elapsed = Duration::ZERO;
            self.fx.borrow_mut().clear();
            self.pending_fx.borrow_mut().clear();
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.sync_scroll_offset();
        self.sync_plugin_scroll_offset();
    }

    /// Computes the inner viewport height of the `SERVERS` list panel (minimum 1 row).
    pub fn list_viewport_height(&self) -> usize {
        // Total height minus HEADER_HEIGHT (2), STATUS_BAR_HEIGHT (1), top/bottom borders (2), and vertical padding (2).
        self.height.saturating_sub(7).max(1) as usize
    }

    /// Ensures `scroll_offset` keeps `selected` within the visible `[scroll_offset, scroll_offset + viewport)` window.
    pub fn sync_scroll_offset(&mut self) {
        let len = self.visible_servers().len();
        if len == 0 {
            self.scroll_offset = 0;
            return;
        }
        let vh = self.list_viewport_height();
        let selected = self.selected.min(len - 1);
        if selected < self.scroll_offset {
            self.scroll_offset = selected;
        } else if selected >= self.scroll_offset + vh {
            self.scroll_offset = selected + 1 - vh;
        }
        let max_offset = len.saturating_sub(vh);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    /// Ensures `plugin_scroll_offset` keeps `plugin_selected` within the visible viewport window.
    pub fn sync_plugin_scroll_offset(&mut self) {
        let len = self.visible_plugin_rows().len();
        if len == 0 {
            self.plugin_scroll_offset = 0;
            return;
        }
        let vh = self.list_viewport_height();
        let selected = self.plugin_selected.min(len - 1);
        if selected < self.plugin_scroll_offset {
            self.plugin_scroll_offset = selected;
        } else if selected >= self.plugin_scroll_offset + vh {
            self.plugin_scroll_offset = selected + 1 - vh;
        }
        let max_offset = len.saturating_sub(vh);
        if self.plugin_scroll_offset > max_offset {
            self.plugin_scroll_offset = max_offset;
        }
    }

    /// Returns `(enabled_count, total_count)` across all enabled harnesses' discovered plugins.
    pub fn total_plugins_counts(&self) -> (usize, usize) {
        let mut enabled = 0usize;
        let mut total = 0usize;
        for kind in HarnessKind::ALL {
            if !self.loaded.config.tools.is_enabled(kind) {
                continue;
            }
            if let Some(plugins) = self.loaded.harness_plugins.get(&kind) {
                total += plugins.len();
                enabled += plugins.iter().filter(|p| p.enabled).count();
            }
        }
        (enabled, total)
    }

    /// Returns the virtual Grouped Accordion rows (`HarnessHeader` + `Plugin`) for the `Plugins` workspace view.
    pub fn visible_plugin_rows(&self) -> Vec<PluginRow> {
        let tokens: Vec<String> = self
            .plugin_filter_query
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        let is_filtering = !tokens.is_empty();

        let mut rows = Vec::new();
        for kind in HarnessKind::ALL {
            if !self.loaded.config.tools.is_enabled(kind) {
                continue;
            }
            let all_plugins = self
                .loaded
                .harness_plugins
                .get(&kind)
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            let matching: Vec<(usize, HarnessPlugin)> = all_plugins
                .iter()
                .enumerate()
                .filter(|(_, p)| tokens.iter().all(|tok| plugin_matches_filter_token(tok, p)))
                .map(|(idx, p)| (idx, p.clone()))
                .collect();

            if is_filtering && matching.is_empty() {
                continue;
            }

            let enabled_count = matching.iter().filter(|(_, p)| p.enabled).count();
            let total_count = matching.len();
            let collapsed = self.collapsed_plugin_groups.contains(&kind);

            rows.push(PluginRow::HarnessHeader {
                harness: kind,
                enabled_count,
                total_count,
                collapsed,
            });

            if !collapsed {
                for (index, plugin) in matching {
                    rows.push(PluginRow::Plugin {
                        harness: kind,
                        index,
                        plugin,
                    });
                }
            }
        }
        rows
    }

    pub fn selected_plugin_row(&self) -> Option<PluginRow> {
        let rows = self.visible_plugin_rows();
        if rows.is_empty() {
            None
        } else {
            let idx = self.plugin_selected.min(rows.len() - 1);
            rows.into_iter().nth(idx)
        }
    }

    pub fn clamp_plugin_selection(&mut self) {
        let len = self.visible_plugin_rows().len();
        if len == 0 {
            self.plugin_selected = 0;
        } else if self.plugin_selected >= len {
            self.plugin_selected = len - 1;
        }
        self.sync_plugin_scroll_offset();
    }

    /// Toggles the currently selected plugin (or folds/unfolds the selected harness group header) in `Plugins` view.
    pub fn toggle_selected_plugin_or_group(&mut self) {
        let Some(row) = self.selected_plugin_row() else {
            return;
        };
        match row {
            PluginRow::HarnessHeader { harness, .. } => {
                if self.collapsed_plugin_groups.contains(&harness) {
                    self.collapsed_plugin_groups.remove(&harness);
                } else {
                    self.collapsed_plugin_groups.insert(harness);
                }
                self.clamp_plugin_selection();
                self.emit_fx(FxTrigger::SelectionChanged);
            }
            PluginRow::Plugin {
                harness,
                index,
                plugin,
            } => {
                if let Some(ref paths) = self.paths {
                    let mut updated = plugin.clone();
                    match toggle_harness_plugin(
                        paths,
                        &self.loaded.config,
                        &mut self.loaded.plugins_ledger,
                        &mut updated,
                    ) {
                        Ok(()) => {
                            if let Some(list) = self.loaded.harness_plugins.get_mut(&harness)
                                && let Some(slot) = list.get_mut(index)
                            {
                                *slot = updated.clone();
                            }
                            self.loaded.plugins_ledger_exists = true;
                            self.loaded.plugins_ledger_needs_save = false;
                            let state_str = if updated.enabled {
                                "enabled"
                            } else {
                                "disabled"
                            };
                            self.status_message = Some((
                                format!(
                                    "✓ '{}' {state_str} in {}",
                                    updated.display_name,
                                    harness.display_name()
                                ),
                                false,
                            ));
                            self.trigger_mascot_reaction(
                                MascotReaction::Casting,
                                Duration::from_millis(750),
                            );
                            self.emit_fx(FxTrigger::ServerToggled);
                        }
                        Err(e) => {
                            self.status_message =
                                Some((format!("✗ Plugin toggle error: {e}"), true));
                        }
                    }
                } else if let Some(list) = self.loaded.harness_plugins.get_mut(&harness)
                    && let Some(slot) = list.get_mut(index)
                {
                    slot.enabled = !slot.enabled;
                    let state_str = if slot.enabled { "enabled" } else { "disabled" };
                    let name = slot.display_name.clone();
                    self.loaded.plugins_ledger.record_plugin(slot);
                    self.status_message = Some((
                        format!("✓ '{name}' {state_str} in {}", harness.display_name()),
                        false,
                    ));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::ServerToggled);
                }
            }
        }
    }

    /// Returns the filtered list of `(name, config, has_conflict)` sorted by name.
    pub fn visible_servers(&self) -> Vec<(String, McpServerConfig, bool)> {
        let tokens: Vec<String> = self
            .filter_query
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        self.outcome
            .canonical
            .iter()
            .filter_map(|(name, cfg)| {
                let conflicted = self.is_conflicted(name);
                if tokens
                    .iter()
                    .all(|token| server_matches_filter_token(token, name, cfg, conflicted))
                {
                    Some((name.clone(), cfg.clone(), conflicted))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn is_conflicted(&self, name: &str) -> bool {
        self.outcome.conflicts.iter().any(|c| c.matches_name(name))
    }

    pub fn enabled_harnesses_label(&self) -> String {
        self.loaded.config.tools.enabled_count_label()
    }

    pub fn selected_server(&self) -> Option<(String, McpServerConfig, bool)> {
        let list = self.visible_servers();
        if list.is_empty() {
            None
        } else {
            let idx = self.selected.min(list.len() - 1);
            list.into_iter().nth(idx)
        }
    }

    pub fn clamp_selection(&mut self) {
        let len = self.visible_servers().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
        self.sync_scroll_offset();
    }

    pub fn sync_to_disk(&mut self, success_msg: impl Into<String>) {
        let msg = success_msg.into();
        if let Some(ref paths) = self.paths {
            match self.loaded.save_all(paths, &self.outcome.canonical) {
                Ok(()) => {
                    self.status_message = Some((format!("✓ {msg}"), false));
                }
                Err(e) => {
                    self.status_message = Some((format!("✗ Sync error: {e}"), true));
                }
            }
        } else {
            self.status_message = Some((format!("✓ {msg}"), false));
        }
    }

    pub fn auto_refresh_enabled(&self) -> bool {
        self.loaded.config.refresh.enabled && self.loaded.config.refresh.interval_secs > 0
    }

    pub fn auto_refresh_interval(&self) -> Duration {
        Duration::from_secs(self.loaded.config.refresh.interval_secs)
    }

    /// Toggles periodic auto-refresh on/off and persists the updated `[refresh]` table to `config.toml`.
    pub fn toggle_auto_refresh(&mut self) {
        self.loaded.config.refresh.enabled = !self.loaded.config.refresh.enabled;
        if self.loaded.config.refresh.enabled && self.loaded.config.refresh.interval_secs == 0 {
            self.loaded.config.refresh.interval_secs = 5;
        }
        let enabled = self.loaded.config.refresh.enabled;
        let secs = self.loaded.config.refresh.interval_secs;

        if let Some(ref paths) = self.paths
            && let Err(e) = self.loaded.save_config(paths)
        {
            self.status_message = Some((format!("✗ Failed to save config.toml: {e}"), true));
            return;
        }

        if enabled {
            self.status_message = Some((format!("✓ Auto-refresh enabled (every {secs}s)"), false));
        } else {
            self.status_message = Some(("○ Auto-refresh paused".to_string(), false));
        }
    }

    /// Clones or updates the currently selected reference documentation repository.
    pub fn clone_or_update_selected_ref(&mut self, _force_update: bool) {
        let Some((alias, entry)) = self
            .loaded
            .refs
            .refs
            .iter()
            .nth(self.refs_selected)
            .map(|(a, e)| (a.clone(), e.clone()))
        else {
            self.status_message = Some(("No reference repository selected".to_string(), true));
            return;
        };

        let cache_dir = self
            .paths
            .as_ref()
            .map(|p| p.refs_cache_dir.clone())
            .unwrap_or_else(|| std::path::PathBuf::from("refs"));
        let target_path = self.loaded.refs.effective_path(&alias, &entry, &cache_dir);

        match crate::domain::refs::clone_or_update_ref(&alias, &entry, &target_path) {
            Ok(msg) => {
                self.status_message = Some((format!("✓ {msg}"), false));
                self.trigger_mascot_reaction(
                    MascotReaction::Casting,
                    Duration::from_millis(750),
                );
                self.emit_fx(FxTrigger::Synced);

                // Auto update guidelines lookup index
                let formatted = crate::domain::refs::format_reference_index(
                    &self.loaded.refs,
                    &cache_dir,
                );
                let _ = crate::domain::guidelines::update_canonical_refs(&formatted);
            }
            Err(e) => {
                self.status_message = Some((format!("✗ Ref operation failed: {e}"), true));
                self.trigger_mascot_reaction(
                    MascotReaction::Startled,
                    Duration::from_millis(800),
                );
            }
        }
    }

    /// Reloads `config.toml`, `mcp.toml`, and enabled harness configs from disk and reconciles
    /// external edits against the current canonical registry.
    pub fn refresh_from_disk(&mut self, silent_if_unchanged: bool) {
        let Some(paths) = self.paths.clone() else {
            if !silent_if_unchanged {
                self.status_message = Some((
                    "✓ Refreshed in-memory state (no disk paths)".to_string(),
                    false,
                ));
            }
            return;
        };

        let prev_selected_name = self.selected_server().map(|(n, _, _)| n);
        let prev_selected_plugin: Option<(HarnessKind, Option<String>)> =
            self.selected_plugin_row().map(|row| match row {
                PluginRow::HarnessHeader { harness, .. } => (harness, None),
                PluginRow::Plugin {
                    harness, plugin, ..
                } => (harness, Some(plugin.id)),
            });
        let prev_canonical = self.outcome.canonical.clone();
        let prev_conflicts = self.outcome.conflicts.clone();
        let prev_plugins = self.loaded.harness_plugins.clone();

        let mut fresh = match LoadedConfigs::load(&paths) {
            Ok(loaded) => loaded,
            Err(e) => {
                self.status_message = Some((format!("✗ Refresh failed: {e}"), true));
                return;
            }
        };

        let mut live_harnesses = Vec::new();
        for kind in HarnessKind::ALL {
            if fresh.config.tools.is_enabled(kind) {
                let h_path = paths.effective_harness_path(kind, &fresh.config);
                let servers = if h_path.exists() {
                    fresh.harness_servers(kind).clone()
                } else {
                    prev_canonical.clone()
                };
                live_harnesses.push((kind, servers));
            }
        }

        let env_ctx = paths.load_env_context();
        let new_outcome = reconcile_live_reload_harnesses_with_env(
            &prev_canonical,
            &fresh.mcp.mcps,
            &live_harnesses,
            Some(&env_ctx),
        );

        let any_harness_out_of_sync = HarnessKind::ALL.into_iter().any(|kind| {
            fresh.config.tools.is_enabled(kind)
                && (!harness_maps_match_with_env(
                    fresh.harness_servers(kind),
                    &new_outcome.canonical,
                    Some(&env_ctx),
                ) || fresh.harness_has_raw_spaced_keys(kind)
                    || !paths.effective_harness_path(kind, &fresh.config).exists())
        });

        let disk_needs_sync = new_outcome.conflicts.is_empty()
            && (fresh.mcp.mcps != new_outcome.canonical
                || fresh.mcp_had_spaced_keys
                || any_harness_out_of_sync);

        let changed = new_outcome.canonical != prev_canonical
            || new_outcome.conflicts != prev_conflicts
            || fresh.harness_plugins != prev_plugins
            || disk_needs_sync;

        if disk_needs_sync {
            if let Err(e) = fresh.save_all(&paths, &new_outcome.canonical) {
                self.status_message = Some((format!("✗ Refresh sync error: {e}"), true));
                return;
            }
        } else {
            if (!fresh.config_exists || fresh.config_needs_save)
                && let Err(e) = fresh.save_config(&paths)
            {
                self.status_message = Some((format!("✗ Refresh config save error: {e}"), true));
                return;
            }
            if fresh.plugins_ledger_needs_save
                && let Err(e) = fresh.save_plugins_ledger(&paths)
            {
                self.status_message = Some((format!("✗ Refresh plugins save error: {e}"), true));
                return;
            }
        }

        self.loaded = fresh;
        self.outcome = new_outcome;

        // Restore selection to the previously highlighted server name if still present
        if let Some(ref target_name) = prev_selected_name
            && let Some(pos) = self
                .visible_servers()
                .iter()
                .position(|(n, _, _)| n == target_name)
        {
            self.selected = pos;
        }
        self.clamp_selection();

        // Restore selection to the previously highlighted plugin or harness header if still present
        if let Some((target_harness, target_plugin_id)) = prev_selected_plugin {
            let rows = self.visible_plugin_rows();
            if let Some(ref pid) = target_plugin_id {
                if let Some(pos) = rows.iter().position(|r| {
                    matches!(
                        r,
                        PluginRow::Plugin { harness, plugin, .. }
                            if *harness == target_harness && &plugin.id == pid
                    )
                }) {
                    self.plugin_selected = pos;
                } else if let Some(pos) = rows.iter().position(|r| {
                    matches!(
                        r,
                        PluginRow::HarnessHeader { harness, .. } if *harness == target_harness
                    )
                }) {
                    self.plugin_selected = pos;
                }
            } else if let Some(pos) = rows.iter().position(|r| {
                matches!(
                    r,
                    PluginRow::HarnessHeader { harness, .. } if *harness == target_harness
                )
            }) {
                self.plugin_selected = pos;
            }
        }
        self.clamp_plugin_selection();

        if !self.outcome.conflicts.is_empty() && matches!(self.mode, ActiveMode::Normal) {
            let count = self.outcome.conflicts.len();
            self.status_message = Some((
                format!("! Refresh detected {count} conflict(s) (press 'c' to resolve)"),
                false,
            ));
            self.trigger_mascot_reaction(MascotReaction::Startled, Duration::from_millis(850));
            self.emit_fx(FxTrigger::ConflictAlert);
        } else if changed {
            self.status_message = Some((
                "✓ Refreshed from disk and synced external changes".to_string(),
                false,
            ));
            self.trigger_mascot_reaction(MascotReaction::Refreshing, Duration::from_millis(650));
            self.emit_fx(FxTrigger::Synced);
        } else if !silent_if_unchanged {
            self.status_message = Some((
                "✓ Refreshed from disk (all configs in sync)".to_string(),
                false,
            ));
            self.trigger_mascot_reaction(MascotReaction::Refreshing, Duration::from_millis(600));
            self.emit_fx(FxTrigger::Synced);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        self.last_input_elapsed = Duration::ZERO;

        match self.mode.clone() {
            ActiveMode::Normal => {
                if let Some(action) = map_workspace_key(key) {
                    self.apply_workspace_action(action);
                }
            }
            ActiveMode::Filtering => self.handle_filter_key(key),
            ActiveMode::Form(form) => self.handle_form_key(key, form),
            ActiveMode::Options(opt) => self.handle_options_key(key, opt),
            ActiveMode::ConflictModal { index } => self.handle_conflict_key(key, index),
            ActiveMode::ConfirmDelete { name } => self.handle_delete_confirm_key(key, &name),
            ActiveMode::ConfirmDeletePlugin {
                harness,
                id,
                display_name,
                install_path,
            } => self.handle_delete_plugin_confirm_key(
                key,
                harness,
                &id,
                &display_name,
                install_path.as_deref(),
            ),
            ActiveMode::ProfilesModal { selected, items } => {
                self.handle_profiles_modal_key(key, selected, items);
            }
            ActiveMode::MatrixModal { server_name, selected } => {
                self.handle_matrix_modal_key(key, server_name, selected);
            }
            ActiveMode::NewProfileModal { input } => {
                self.handle_new_profile_key(key, input);
            }
            ActiveMode::Help => match (key.modifiers, key.code) {
                (_, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter) => {
                    self.mode = ActiveMode::Normal;
                }
                (_, KeyCode::Down | KeyCode::Char('j')) => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                }
                (_, KeyCode::Up | KeyCode::Char('k')) => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                (_, KeyCode::PageDown) | (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                    self.help_scroll = self.help_scroll.saturating_add(8);
                }
                (_, KeyCode::PageUp) | (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                    self.help_scroll = self.help_scroll.saturating_sub(8);
                }
                (KeyModifiers::NONE, KeyCode::Char('g')) | (_, KeyCode::Home) => {
                    self.help_scroll = 0;
                }
                (KeyModifiers::SHIFT, KeyCode::Char('G'))
                | (KeyModifiers::NONE, KeyCode::Char('G'))
                | (_, KeyCode::End) => {
                    self.help_scroll = usize::MAX / 2;
                }
                _ => {}
            },
        }
        self.recompute_focus_gaze_target();
    }

    pub fn apply_workspace_action(&mut self, action: Action) {
        let len = match self.section {
            WorkspaceSection::Mcps => self.visible_servers().len(),
            WorkspaceSection::Plugins => self.visible_plugin_rows().len(),
            WorkspaceSection::Guidelines => self.guidelines_statuses.len(),
            WorkspaceSection::Refs => self.loaded.refs.refs.len(),
        };
        match action {
            Action::Quit => self.should_quit = true,
            Action::ToggleWorkspaceSection => {
                self.section = self.section.toggle();
                self.details_scroll = 0;
                self.emit_fx(FxTrigger::SelectionChanged);
            }
            Action::SelectMcpsSection => {
                if self.section != WorkspaceSection::Mcps {
                    self.section = WorkspaceSection::Mcps;
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::SelectPluginsSection => {
                if self.section != WorkspaceSection::Plugins {
                    self.section = WorkspaceSection::Plugins;
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::SelectGuidelinesSection => {
                if self.section != WorkspaceSection::Guidelines {
                    self.section = WorkspaceSection::Guidelines;
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::SelectRefsSection => {
                if self.section != WorkspaceSection::Refs {
                    self.section = WorkspaceSection::Refs;
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::Down => {
                if self.details_focused {
                    self.details_scroll = self.details_scroll.saturating_add(1);
                } else if len > 0 {
                    match self.section {
                        WorkspaceSection::Mcps => {
                            self.selected = (self.selected + 1) % len;
                            self.sync_scroll_offset();
                        }
                        WorkspaceSection::Plugins => {
                            self.plugin_selected = (self.plugin_selected + 1) % len;
                            self.sync_plugin_scroll_offset();
                        }
                        WorkspaceSection::Guidelines => {
                            self.guidelines_selected = (self.guidelines_selected + 1) % len;
                        }
                        WorkspaceSection::Refs => {
                            self.refs_selected = (self.refs_selected + 1) % len;
                        }
                    }
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::Up => {
                if self.details_focused {
                    self.details_scroll = self.details_scroll.saturating_sub(1);
                } else if len > 0 {
                    match self.section {
                        WorkspaceSection::Mcps => {
                            self.selected = if self.selected == 0 {
                                len - 1
                            } else {
                                self.selected - 1
                            };
                            self.sync_scroll_offset();
                        }
                        WorkspaceSection::Plugins => {
                            self.plugin_selected = if self.plugin_selected == 0 {
                                len - 1
                            } else {
                                self.plugin_selected - 1
                            };
                            self.sync_plugin_scroll_offset();
                        }
                        WorkspaceSection::Guidelines => {
                            self.guidelines_selected = if self.guidelines_selected == 0 {
                                len - 1
                            } else {
                                self.guidelines_selected - 1
                            };
                        }
                        WorkspaceSection::Refs => {
                            self.refs_selected = if self.refs_selected == 0 {
                                len - 1
                            } else {
                                self.refs_selected - 1
                            };
                        }
                    }
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::PageDown => {
                if self.details_focused {
                    self.details_scroll = self.details_scroll.saturating_add(10);
                } else if len > 0 {
                    match self.section {
                        WorkspaceSection::Mcps => {
                            self.selected = (self.selected + 10).min(len - 1);
                            self.sync_scroll_offset();
                        }
                        WorkspaceSection::Plugins => {
                            self.plugin_selected = (self.plugin_selected + 10).min(len - 1);
                            self.sync_plugin_scroll_offset();
                        }
                        WorkspaceSection::Guidelines => {
                            self.guidelines_selected = (self.guidelines_selected + 10).min(len - 1);
                        }
                        WorkspaceSection::Refs => {
                            self.refs_selected = (self.refs_selected + 10).min(len - 1);
                        }
                    }
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::PageUp => {
                if self.details_focused {
                    self.details_scroll = self.details_scroll.saturating_sub(10);
                } else if len > 0 {
                    match self.section {
                        WorkspaceSection::Mcps => {
                            self.selected = self.selected.saturating_sub(10);
                            self.sync_scroll_offset();
                        }
                        WorkspaceSection::Plugins => {
                            self.plugin_selected = self.plugin_selected.saturating_sub(10);
                            self.sync_plugin_scroll_offset();
                        }
                        WorkspaceSection::Guidelines => {
                            self.guidelines_selected = self.guidelines_selected.saturating_sub(10);
                        }
                        WorkspaceSection::Refs => {
                            self.refs_selected = self.refs_selected.saturating_sub(10);
                        }
                    }
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::First => {
                if self.details_focused {
                    self.details_scroll = 0;
                } else if len > 0 {
                    match self.section {
                        WorkspaceSection::Mcps => {
                            self.selected = 0;
                            self.sync_scroll_offset();
                        }
                        WorkspaceSection::Plugins => {
                            self.plugin_selected = 0;
                            self.sync_plugin_scroll_offset();
                        }
                        WorkspaceSection::Guidelines => {
                            self.guidelines_selected = 0;
                        }
                        WorkspaceSection::Refs => {
                            self.refs_selected = 0;
                        }
                    }
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::Last => {
                if self.details_focused {
                    self.details_scroll = usize::MAX / 2;
                } else if len > 0 {
                    match self.section {
                        WorkspaceSection::Mcps => {
                            self.selected = len - 1;
                            self.sync_scroll_offset();
                        }
                        WorkspaceSection::Plugins => {
                            self.plugin_selected = len - 1;
                            self.sync_plugin_scroll_offset();
                        }
                        WorkspaceSection::Guidelines => {
                            self.guidelines_selected = len - 1;
                        }
                        WorkspaceSection::Refs => {
                            self.refs_selected = len - 1;
                        }
                    }
                    self.details_scroll = 0;
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::ToggleSelected => match self.section {
                WorkspaceSection::Mcps => {
                    if let Some((name, _, _)) = self.selected_server()
                        && let Some(cfg) = self.outcome.canonical.get_mut(&name)
                    {
                        cfg.enabled = !cfg.enabled;
                        let new_state = if cfg.enabled { "enabled" } else { "disabled" };
                        let targets = self.enabled_harnesses_label();
                        self.sync_to_disk(format!("'{name}' {new_state} across {targets}"));
                        self.trigger_mascot_reaction(
                            MascotReaction::Casting,
                            Duration::from_millis(750),
                        );
                        self.emit_fx(FxTrigger::ServerToggled);
                    }
                }
                WorkspaceSection::Plugins => {
                    self.toggle_selected_plugin_or_group();
                }
                WorkspaceSection::Guidelines => {
                    if let Ok(canonical) = crate::domain::guidelines::canonical_guidelines_path() {
                        self.pending_editor = Some(EditorRequest {
                            original_name: None,
                            conflict_index: None,
                            initial_toml: String::new(),
                            direct_file_path: Some(canonical),
                        });
                    }
                }
                WorkspaceSection::Refs => {
                    self.clone_or_update_selected_ref(false);
                }
            },
            Action::ConfirmOrFold => {
                if self.section == WorkspaceSection::Guidelines {
                    if let Ok(canonical) = crate::domain::guidelines::canonical_guidelines_path() {
                        self.pending_editor = Some(EditorRequest {
                            original_name: None,
                            conflict_index: None,
                            initial_toml: String::new(),
                            direct_file_path: Some(canonical),
                        });
                    }
                } else if self.section == WorkspaceSection::Refs {
                    self.clone_or_update_selected_ref(false);
                } else if self.section == WorkspaceSection::Plugins
                    && let Some(PluginRow::HarnessHeader { harness, .. }) =
                        self.selected_plugin_row()
                {
                    if self.collapsed_plugin_groups.contains(&harness) {
                        self.collapsed_plugin_groups.remove(&harness);
                    } else {
                        self.collapsed_plugin_groups.insert(harness);
                    }
                    self.clamp_plugin_selection();
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::FoldLeft
                if self.section == WorkspaceSection::Plugins && !self.details_focused =>
            {
                match self.selected_plugin_row() {
                    Some(PluginRow::HarnessHeader { harness, .. }) => {
                        if self.collapsed_plugin_groups.insert(harness) {
                            self.clamp_plugin_selection();
                            self.emit_fx(FxTrigger::SelectionChanged);
                        }
                    }
                    Some(PluginRow::Plugin { harness, .. }) => {
                        self.collapsed_plugin_groups.insert(harness);
                        if let Some(pos) = self.visible_plugin_rows().iter().position(|r| {
                            matches!(
                                r,
                                PluginRow::HarnessHeader { harness: h, .. } if *h == harness
                            )
                        }) {
                            self.plugin_selected = pos;
                        }
                        self.details_scroll = 0;
                        self.clamp_plugin_selection();
                        self.emit_fx(FxTrigger::SelectionChanged);
                    }
                    None => {}
                }
            }
            Action::FoldRight
                if self.section == WorkspaceSection::Plugins && !self.details_focused =>
            {
                if let Some(PluginRow::HarnessHeader { harness, .. }) = self.selected_plugin_row()
                    && self.collapsed_plugin_groups.remove(&harness)
                {
                    self.clamp_plugin_selection();
                    self.emit_fx(FxTrigger::SelectionChanged);
                }
            }
            Action::StartFilter => {
                self.mode = ActiveMode::Filtering;
                self.trigger_mascot_reaction(MascotReaction::Scrying, Duration::from_millis(800));
            }
            Action::UpdateSelectedRef if self.section == WorkspaceSection::Refs => {
                self.clone_or_update_selected_ref(true);
            }
            Action::UpdateSelectedRef => {}
            Action::EditForm | Action::OpenEditor
                if self.section == WorkspaceSection::Refs =>
            {
                if let Some(ref paths) = self.paths {
                    self.pending_editor = Some(EditorRequest {
                        original_name: None,
                        conflict_index: None,
                        initial_toml: String::new(),
                        direct_file_path: Some(paths.refs_toml.clone()),
                    });
                }
            }
            Action::AddForm if self.section == WorkspaceSection::Mcps => {
                self.mode = ActiveMode::Form(FormState::new_add());
                self.emit_fx(FxTrigger::ModalOpened);
            }
            Action::EditForm | Action::OpenEditor
                if self.section == WorkspaceSection::Guidelines =>
            {
                if let Ok(canonical) = crate::domain::guidelines::canonical_guidelines_path() {
                    self.pending_editor = Some(EditorRequest {
                        original_name: None,
                        conflict_index: None,
                        initial_toml: String::new(),
                        direct_file_path: Some(canonical),
                    });
                }
            }
            Action::EditForm if self.section == WorkspaceSection::Mcps => {
                if let Some((name, cfg, _)) = self.selected_server() {
                    self.mode = ActiveMode::Form(FormState::from_existing(&name, &cfg));
                    self.emit_fx(FxTrigger::ModalOpened);
                }
            }
            Action::OpenEditor if self.section == WorkspaceSection::Mcps => {
                if let Some((name, cfg, _)) = self.selected_server() {
                    let snippet = SingleServerSnippet {
                        name: name.clone(),
                        config: cfg,
                    };
                    if let Ok(initial_toml) = snippet.to_editor_toml() {
                        self.pending_editor = Some(EditorRequest {
                            original_name: Some(name),
                            conflict_index: None,
                            initial_toml,
                            direct_file_path: None,
                        });
                    }
                } else {
                    let snippet = SingleServerSnippet {
                        name: "new-mcp".to_string(),
                        config: McpServerConfig::new_stdio("npx", vec!["-y".to_string()]),
                    };
                    if let Ok(initial_toml) = snippet.to_editor_toml() {
                        self.pending_editor = Some(EditorRequest {
                            original_name: None,
                            conflict_index: None,
                            initial_toml,
                            direct_file_path: None,
                        });
                    }
                }
            }
            Action::OpenEditor if self.section == WorkspaceSection::Plugins => {
                let target_path = match self.selected_plugin_row() {
                    Some(PluginRow::Plugin { plugin, .. }) => {
                        if let Some(ref dir) = plugin.install_path {
                            if dir.is_file() {
                                dir.clone()
                            } else {
                                let candidates = [
                                    dir.join("plugin.json"),
                                    dir.join(".claude-plugin").join("plugin.json"),
                                    dir.join("package.json"),
                                    dir.join("index.ts"),
                                    dir.join("index.js"),
                                    dir.join("mod.ts"),
                                ];
                                candidates
                                    .into_iter()
                                    .find(|p| p.is_file())
                                    .unwrap_or_else(|| plugin.config_path.clone())
                            }
                        } else {
                            plugin.config_path.clone()
                        }
                    }
                    Some(PluginRow::HarnessHeader { harness, .. }) => {
                        let Some(ref paths) = self.paths else {
                            return;
                        };
                        match harness {
                            HarnessKind::Antigravity => {
                                paths.effective_agy_plugin_config_path(&self.loaded.config)
                            }
                            HarnessKind::Claude => {
                                paths.effective_claude_settings_path(&self.loaded.config)
                            }
                            HarnessKind::Copilot => {
                                paths.effective_copilot_plugin_config_path(&self.loaded.config)
                            }
                            HarnessKind::Grok => {
                                paths.effective_harness_path(HarnessKind::Grok, &self.loaded.config)
                            }
                            HarnessKind::OpenCode => paths
                                .effective_harness_path(HarnessKind::OpenCode, &self.loaded.config),
                        }
                    }
                    None => return,
                };
                self.pending_editor = Some(EditorRequest {
                    original_name: None,
                    conflict_index: None,
                    initial_toml: String::new(),
                    direct_file_path: Some(target_path),
                });
            }
            Action::DeleteSelected if self.section == WorkspaceSection::Mcps => {
                if let Some((name, _, _)) = self.selected_server() {
                    self.mode = ActiveMode::ConfirmDelete { name };
                    self.trigger_mascot_reaction(
                        MascotReaction::Startled,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::ModalOpened);
                }
            }
            Action::DeleteSelected if self.section == WorkspaceSection::Plugins => {
                if let Some(PluginRow::Plugin {
                    harness, plugin, ..
                }) = self.selected_plugin_row()
                {
                    self.mode = ActiveMode::ConfirmDeletePlugin {
                        harness,
                        id: plugin.id,
                        display_name: plugin.display_name,
                        install_path: plugin.install_path,
                    };
                    self.trigger_mascot_reaction(
                        MascotReaction::Startled,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::ModalOpened);
                }
            }
            Action::OpenConflicts => {
                if self.section == WorkspaceSection::Refs {
                    self.clone_or_update_selected_ref(false);
                } else if !self.outcome.conflicts.is_empty() {
                    self.mode = ActiveMode::ConflictModal { index: 0 };
                    self.trigger_mascot_reaction(
                        MascotReaction::Startled,
                        Duration::from_millis(850),
                    );
                    self.emit_fx(FxTrigger::ModalOpened);
                } else {
                    self.status_message = Some(("✓ No pending conflicts".to_string(), false));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(550),
                    );
                    self.emit_fx(FxTrigger::Synced);
                }
            }
            Action::OpenOptions => {
                self.mode = ActiveMode::Options(OptionsState::from_config(&self.loaded.config));
                self.emit_fx(FxTrigger::ModalOpened);
            }
            Action::OpenProfiles => {
                let mut items = vec!["Default (All Servers)".to_string()];
                items.extend(self.loaded.profiles.profiles.keys().cloned());
                let current_active = self.loaded.config.active_profile.as_deref().unwrap_or("");
                let selected = items.iter().position(|i| i == current_active).unwrap_or(0);
                self.mode = ActiveMode::ProfilesModal { selected, items };
                self.emit_fx(FxTrigger::ModalOpened);
            }
            Action::OpenMatrix => {
                if let Some((name, _, _)) = self.selected_server() {
                    self.mode = ActiveMode::MatrixModal {
                        server_name: name,
                        selected: 0,
                    };
                    self.emit_fx(FxTrigger::ModalOpened);
                } else {
                    self.status_message = Some(("Select an MCP server to edit matrix overrides".to_string(), true));
                }
            }
            Action::ToggleHeroMascot => {
                if self.loaded.config.ui.hero_mascot {
                    self.show_hero_mascot = !self.show_hero_mascot;
                    if self.show_hero_mascot {
                        self.reset_mascot_reveal();
                        self.trigger_mascot_reaction(
                            MascotReaction::Casting,
                            Duration::from_millis(800),
                        );
                        self.status_message = Some((
                            "✦ Summoned Maga Hero Showcase (press 'm' for Inspector)".to_string(),
                            false,
                        ));
                    } else {
                        let inspector = match self.section {
                            WorkspaceSection::Mcps => "Server Inspector",
                            WorkspaceSection::Plugins => "Plugin Inspector",
                            WorkspaceSection::Guidelines => "Guidelines Inspector",
                            WorkspaceSection::Refs => "Reference Inspector",
                        };
                        self.status_message = Some((format!("✓ Returned to {inspector}"), false));
                    }
                    self.emit_fx(FxTrigger::SelectionChanged);
                } else {
                    self.status_message = Some((
                        "○ Hero Mascot is disabled in Options ('o')".to_string(),
                        false,
                    ));
                }
            }
            Action::ToggleHideUi => {
                self.loaded.config.ui.hide_ui = !self.loaded.config.ui.hide_ui;
                if self.loaded.config.ui.hide_ui {
                    self.reset_mascot_reveal();
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(800),
                    );
                    self.status_message = Some((
                        "✦ Background-Only mode (press 'b' or Esc to show UI)".to_string(),
                        false,
                    ));
                } else {
                    self.status_message = Some(("✓ Restored workspace UI".to_string(), false));
                }
                self.emit_fx(FxTrigger::Startup);
                if let Some(ref paths) = self.paths {
                    let _ = self.loaded.save_config(paths);
                }
            }
            Action::RefreshFromDisk => {
                self.refresh_from_disk(false);
            }
            Action::ToggleAutoRefresh => {
                self.toggle_auto_refresh();
                self.trigger_mascot_reaction(
                    MascotReaction::Refreshing,
                    Duration::from_millis(600),
                );
                self.emit_fx(FxTrigger::Synced);
            }
            Action::SyncNow => {
                if self.section == WorkspaceSection::Guidelines || self.section == WorkspaceSection::Refs {
                    let mut warning_suffix = String::new();
                    if !self.loaded.refs.refs.is_empty() {
                        let cache_dir = self
                            .paths
                            .as_ref()
                            .map(|p| p.refs_cache_dir.clone())
                            .unwrap_or_else(|| std::path::PathBuf::from("refs"));
                        let formatted_index = crate::domain::refs::format_reference_index(
                            &self.loaded.refs,
                            &cache_dir,
                        );
                        if let Ok(metrics) =
                            crate::domain::guidelines::update_canonical_refs(&formatted_index)
                        {
                            if metrics.exceeds_recommended_budget {
                                warning_suffix = format!(
                                    " (⚠ Sizing warning: {} lines, {} chars)",
                                    metrics.line_count, metrics.char_count
                                );
                            }
                        }
                    } else if let Ok(Some(metrics)) =
                        crate::domain::guidelines::canonical_guidelines_metrics()
                    {
                        if metrics.exceeds_recommended_budget {
                            warning_suffix = format!(
                                " (⚠ Sizing warning: {} lines, {} chars)",
                                metrics.line_count, metrics.char_count
                            );
                        }
                    }

                    match crate::domain::guidelines::sync_guidelines() {
                        Ok(results) => {
                            let count = results.iter().filter(|r| r.success).count();
                            self.guidelines_statuses =
                                crate::domain::guidelines::check_guidelines_status().unwrap_or_default();
                            self.status_message = Some((
                                format!(
                                    "✓ Synchronized guidelines across {count}/{} harnesses{warning_suffix}",
                                    results.len()
                                ),
                                false,
                            ));
                        }
                        Err(e) => {
                            self.status_message =
                                Some((format!("✗ Guidelines sync failed: {e}"), true));
                        }
                    }
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::Synced);
                } else {
                    let targets = self.enabled_harnesses_label();
                    self.sync_to_disk(format!("Synchronized canonical registry to {targets}"));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::Synced);
                }
            }
            Action::ProbeSelected => {
                self.probe_selected();
            }
            Action::ProbeAll => {
                self.probe_all_enabled();
            }
            Action::ToggleDetailsFocus => {
                self.show_hero_mascot = false;
                self.details_focused = !self.details_focused;
                let list_label = match self.section {
                    WorkspaceSection::Mcps => "SERVERS",
                    WorkspaceSection::Plugins => "PLUGINS",
                    WorkspaceSection::Guidelines => "GUIDELINES",
                    WorkspaceSection::Refs => "REFERENCES",
                };
                if self.details_focused {
                    self.status_message = Some((
                        format!(
                            "✓ Focused DETAILS inspector (j/k/PgUp/PgDn to scroll, t/Esc for {list_label})"
                        ),
                        false,
                    ));
                } else {
                    self.status_message =
                        Some((format!("✓ Returned focus to {list_label} list"), false));
                }
                self.emit_fx(FxTrigger::SelectionChanged);
            }
            Action::ToggleHelp => {
                self.help_scroll = 0;
                self.mode = ActiveMode::Help;
                self.emit_fx(FxTrigger::ModalOpened);
            }
            Action::CancelModal if self.loaded.config.ui.hide_ui => {
                self.loaded.config.ui.hide_ui = false;
                self.status_message = Some(("✓ Restored workspace UI".to_string(), false));
                self.emit_fx(FxTrigger::Startup);
                if let Some(ref paths) = self.paths {
                    let _ = self.loaded.save_config(paths);
                }
            }
            Action::CancelModal if self.details_focused => {
                self.details_focused = false;
                let list_label = match self.section {
                    WorkspaceSection::Mcps => "SERVERS",
                    WorkspaceSection::Plugins => "PLUGINS",
                    WorkspaceSection::Guidelines => "GUIDELINES",
                    WorkspaceSection::Refs => "REFERENCES",
                };
                self.status_message =
                    Some((format!("✓ Returned focus to {list_label} list"), false));
                self.emit_fx(FxTrigger::SelectionChanged);
            }
            Action::CancelModal
                if self.section == WorkspaceSection::Mcps && !self.filter_query.is_empty() =>
            {
                self.filter_query.clear();
                self.clamp_selection();
            }
            Action::CancelModal
                if self.section == WorkspaceSection::Plugins
                    && !self.plugin_filter_query.is_empty() =>
            {
                self.plugin_filter_query.clear();
                self.clamp_plugin_selection();
            }
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.mode = ActiveMode::Normal;
            }
            KeyCode::Esc => {
                match self.section {
                    WorkspaceSection::Mcps => {
                        self.filter_query.clear();
                        self.clamp_selection();
                    }
                    WorkspaceSection::Plugins => {
                        self.plugin_filter_query.clear();
                        self.clamp_plugin_selection();
                    }
                    WorkspaceSection::Guidelines | WorkspaceSection::Refs => {}
                }
                self.mode = ActiveMode::Normal;
            }
            KeyCode::Down => {
                self.apply_workspace_action(Action::Down);
            }
            KeyCode::Up => {
                self.apply_workspace_action(Action::Up);
            }
            KeyCode::PageDown => {
                self.apply_workspace_action(Action::PageDown);
            }
            KeyCode::PageUp => {
                self.apply_workspace_action(Action::PageUp);
            }
            KeyCode::Backspace => match self.section {
                WorkspaceSection::Mcps => {
                    self.filter_query.pop();
                    self.selected = 0;
                    self.scroll_offset = 0;
                }
                WorkspaceSection::Plugins => {
                    self.plugin_filter_query.pop();
                    self.plugin_selected = 0;
                    self.plugin_scroll_offset = 0;
                }
                WorkspaceSection::Guidelines | WorkspaceSection::Refs => {}
            },
            KeyCode::Char(c) => match self.section {
                WorkspaceSection::Mcps => {
                    self.filter_query.push(c);
                    self.selected = 0;
                    self.scroll_offset = 0;
                }
                WorkspaceSection::Plugins => {
                    self.plugin_filter_query.push(c);
                    self.plugin_selected = 0;
                    self.plugin_scroll_offset = 0;
                }
                WorkspaceSection::Guidelines | WorkspaceSection::Refs => {}
            },
            _ => {}
        }
    }

    fn handle_form_key(&mut self, key: KeyEvent, mut form: FormState) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                self.mode = ActiveMode::Normal;
            }
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::NONE, KeyCode::Down) => {
                form.focused = form.focused.next(form.transport);
                self.mode = ActiveMode::Form(form);
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) | (KeyModifiers::NONE, KeyCode::Up) => {
                form.focused = form.focused.prev(form.transport);
                self.mode = ActiveMode::Form(form);
            }
            (_, KeyCode::Enter) => match form.build_config() {
                Ok((new_name, cfg)) => {
                    if let Some(ref old_name) = form.original_name
                        && old_name != &new_name
                    {
                        self.outcome.canonical.remove(old_name);
                        self.probe_states.remove(old_name);
                    }
                    self.probe_states.remove(&new_name);
                    self.outcome.canonical.insert(new_name.clone(), cfg);
                    self.mode = ActiveMode::Normal;
                    self.clamp_selection();
                    let targets = self.enabled_harnesses_label();
                    self.sync_to_disk(format!("Saved '{new_name}' and synced to {targets}"));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::ServerSaved);
                }
                Err(err) => {
                    form.error = Some(err);
                    self.trigger_mascot_reaction(
                        MascotReaction::Startled,
                        Duration::from_millis(650),
                    );
                    self.mode = ActiveMode::Form(form);
                }
            },
            _ => {
                match form.focused {
                    FormField::Transport => {
                        if matches!(
                            key.code,
                            KeyCode::Char(' ')
                                | KeyCode::Left
                                | KeyCode::Right
                                | KeyCode::Char('t')
                        ) {
                            form.transport = match form.transport {
                                Transport::Stdio => Transport::Http,
                                Transport::Http => Transport::Stdio,
                            };
                        }
                    }
                    FormField::Name => edit_name_buffer(&mut form.name, key),
                    FormField::Description => edit_text_buffer(&mut form.description, key),
                    FormField::Endpoint => edit_text_buffer(&mut form.endpoint, key),
                    FormField::Args => edit_text_buffer(&mut form.args_input, key),
                    FormField::Pairs => edit_text_buffer(&mut form.pairs_input, key),
                }
                self.mode = ActiveMode::Form(form);
            }
        }
    }

    fn handle_conflict_key(&mut self, key: KeyEvent, index: usize) {
        if index >= self.outcome.conflicts.len() {
            self.mode = ActiveMode::Normal;
            return;
        }
        match key.code {
            KeyCode::Char(ch @ '1'..='9') => {
                let variant_idx = (ch as usize) - ('1' as usize);
                if let Some(variant) = self.outcome.conflicts[index].variants.get(variant_idx) {
                    let label = variant.sources_label();
                    if self
                        .outcome
                        .resolve(index, ConflictResolution::KeepVariant(variant_idx))
                        .is_ok()
                    {
                        self.advance_conflict_modal(
                            index,
                            &format!("Resolved conflict with {label} config"),
                        );
                    }
                }
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                let conflict: &McpConflict = &self.outcome.conflicts[index];
                let base_cfg = conflict
                    .variants
                    .first()
                    .map(|v| v.config.clone())
                    .or_else(|| conflict.maga.clone())
                    .unwrap_or_else(|| McpServerConfig::new_http("https://example.com/mcp"));
                let snippet = SingleServerSnippet {
                    name: conflict.name.clone(),
                    config: base_cfg,
                };
                if let Ok(initial_toml) = snippet.to_editor_toml() {
                    self.pending_editor = Some(EditorRequest {
                        original_name: Some(conflict.name.clone()),
                        conflict_index: Some(index),
                        initial_toml,
                        direct_file_path: None,
                    });
                }
            }
            KeyCode::Char('s') | KeyCode::Esc => {
                if index + 1 < self.outcome.conflicts.len() {
                    self.mode = ActiveMode::ConflictModal { index: index + 1 };
                } else {
                    self.mode = ActiveMode::Normal;
                }
            }
            _ => {}
        }
    }

    fn advance_conflict_modal(&mut self, index: usize, msg: &str) {
        if self.outcome.conflicts.is_empty() {
            self.mode = ActiveMode::Normal;
            self.sync_to_disk(msg);
            self.trigger_mascot_reaction(MascotReaction::Casting, Duration::from_millis(750));
            self.emit_fx(FxTrigger::Synced);
        } else {
            let next_idx = index.min(self.outcome.conflicts.len() - 1);
            self.mode = ActiveMode::ConflictModal { index: next_idx };
            self.sync_to_disk(msg);
            self.trigger_mascot_reaction(MascotReaction::Startled, Duration::from_millis(700));
            self.emit_fx(FxTrigger::ModalOpened);
        }
    }

    fn handle_delete_confirm_key(&mut self, key: KeyEvent, name: &str) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                self.outcome.canonical.remove(name);
                self.probe_states.remove(name);
                self.outcome.conflicts.retain(|c| !c.matches_name(name));
                self.mode = ActiveMode::Normal;
                self.clamp_selection();
                let targets = self.enabled_harnesses_label();
                self.sync_to_disk(format!("Deleted '{name}' from registry and {targets}"));
                self.trigger_mascot_reaction(MascotReaction::Casting, Duration::from_millis(700));
                self.emit_fx(FxTrigger::Synced);
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = ActiveMode::Normal;
            }
            _ => {}
        }
    }

    fn handle_profiles_modal_key(&mut self, key: KeyEvent, mut selected: usize, items: Vec<String>) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q')) => {
                self.mode = ActiveMode::Normal;
            }
            (_, KeyCode::Char('a')) => {
                self.mode = ActiveMode::NewProfileModal { input: String::new() };
            }
            (_, KeyCode::Char('e') | KeyCode::Char('E')) => {
                if let Some(ref paths) = self.paths {
                    if !paths.profiles_toml.exists() {
                        let _ = self.loaded.save_profiles(paths);
                    }
                    self.pending_editor = Some(EditorRequest {
                        original_name: None,
                        conflict_index: None,
                        initial_toml: String::new(),
                        direct_file_path: Some(paths.profiles_toml.clone()),
                    });
                    self.mode = ActiveMode::Normal;
                }
            }
            (_, KeyCode::Char('d')) => {
                if let Some(choice) = items.get(selected) {
                    if !choice.starts_with("Default") {
                        let to_remove = choice.clone();
                        self.loaded.profiles.profiles.remove(&to_remove);
                        if self.loaded.config.active_profile.as_deref() == Some(&to_remove) {
                            self.loaded.config.active_profile = None;
                        }
                        if let Some(ref paths) = self.paths {
                            let _ = self.loaded.save_profiles(paths);
                            let _ = self.loaded.save_config(paths);
                        }
                        self.status_message = Some((format!("✓ Deleted profile '{to_remove}'"), false));
                        let mut new_items = vec!["Default (All Servers)".to_string()];
                        new_items.extend(self.loaded.profiles.profiles.keys().cloned());
                        self.mode = ActiveMode::ProfilesModal {
                            selected: selected.min(new_items.len().saturating_sub(1)),
                            items: new_items,
                        };
                    }
                }
            }
            (_, KeyCode::Down | KeyCode::Char('j')) => {
                if !items.is_empty() {
                    selected = (selected + 1) % items.len();
                    self.mode = ActiveMode::ProfilesModal { selected, items };
                }
            }
            (_, KeyCode::Up | KeyCode::Char('k')) => {
                if !items.is_empty() {
                    selected = if selected == 0 { items.len() - 1 } else { selected - 1 };
                    self.mode = ActiveMode::ProfilesModal { selected, items };
                }
            }
            (_, KeyCode::Enter | KeyCode::Char(' ')) => {
                if let Some(choice) = items.get(selected) {
                    if choice.starts_with("Default") {
                        self.loaded.config.active_profile = None;
                        self.sync_to_disk("Active profile: Default (All servers)".to_string());
                    } else {
                        self.loaded.config.active_profile = Some(choice.clone());
                        self.sync_to_disk(format!("Active profile: {choice}"));
                    }
                    if let Some(ref paths) = self.paths {
                        let _ = self.loaded.save_config(paths);
                    }
                }
                self.mode = ActiveMode::Normal;
            }
            _ => {}
        }
    }

    fn handle_new_profile_key(&mut self, key: KeyEvent, mut input: String) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                let mut items = vec!["Default (All Servers)".to_string()];
                items.extend(self.loaded.profiles.profiles.keys().cloned());
                self.mode = ActiveMode::ProfilesModal { selected: 0, items };
            }
            (_, KeyCode::Enter) => {
                let name = input.trim().to_string();
                if name.is_empty() {
                    let mut items = vec!["Default (All Servers)".to_string()];
                    items.extend(self.loaded.profiles.profiles.keys().cloned());
                    self.mode = ActiveMode::ProfilesModal { selected: 0, items };
                    return;
                }
                let active_servers: Vec<String> = self
                    .outcome
                    .canonical
                    .iter()
                    .filter(|(_, s)| s.enabled)
                    .map(|(k, _)| k.clone())
                    .collect();
                self.loaded.profiles.profiles.insert(
                    name.clone(),
                    crate::domain::model::ProfileDefinition {
                        description: Some(format!("Custom profile {name}")),
                        include: active_servers,
                        exclude: Vec::new(),
                    },
                );
                self.loaded.config.active_profile = Some(name.clone());
                if let Some(ref paths) = self.paths {
                    let _ = self.loaded.save_profiles(paths);
                    let _ = self.loaded.save_config(paths);
                    let canonical = self.outcome.canonical.clone();
                    let _ = self.loaded.save_all(paths, &canonical);
                }
                self.status_message =
                    Some((format!("✓ Created and activated profile '{name}'"), false));
                self.mode = ActiveMode::Normal;
            }
            (_, KeyCode::Backspace) => {
                input.pop();
                self.mode = ActiveMode::NewProfileModal { input };
            }
            (_, KeyCode::Char(c)) => {
                if !c.is_control() && c != ' ' {
                    input.push(c);
                    self.mode = ActiveMode::NewProfileModal { input };
                }
            }
            _ => {}
        }
    }

    fn handle_matrix_modal_key(&mut self, key: KeyEvent, server_name: String, mut selected: usize) {
        let projects = self.loaded.claude.project_paths();
        let total_rows = 1 + HarnessKind::ALL.len() + projects.len();
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) => {
                self.sync_to_disk(format!("Updated matrix overrides for '{server_name}'"));
                self.mode = ActiveMode::Normal;
            }
            (_, KeyCode::Down | KeyCode::Char('j')) => {
                selected = (selected + 1) % total_rows;
                self.mode = ActiveMode::MatrixModal { server_name, selected };
            }
            (_, KeyCode::Up | KeyCode::Char('k')) => {
                selected = if selected == 0 { total_rows - 1 } else { selected - 1 };
                self.mode = ActiveMode::MatrixModal { server_name, selected };
            }
            (_, KeyCode::Char(' ')) => {
                let harness_end = 1 + HarnessKind::ALL.len();
                if selected == 0 {
                    if let Some(server) = self.outcome.canonical.get_mut(&server_name) {
                        server.project_policy = match server.project_policy {
                            ProjectPolicy::Mirror => ProjectPolicy::Preserve,
                            ProjectPolicy::Preserve => ProjectPolicy::Mirror,
                        };
                    }
                } else if selected < harness_end {
                    let harness = HarnessKind::ALL[selected - 1];
                    if let Some(server) = self.outcome.canonical.get_mut(&server_name) {
                        match server.harnesses.get(&harness) {
                            None => {
                                server.harnesses.insert(harness, true);
                            }
                            Some(true) => {
                                server.harnesses.insert(harness, false);
                            }
                            Some(false) => {
                                server.harnesses.remove(&harness);
                            }
                        }
                    }
                } else {
                    let proj_idx = selected - harness_end;
                    if let Some(proj_path) = projects.get(proj_idx).cloned() {
                        if let Some(server) = self.outcome.canonical.get_mut(&server_name) {
                            server.project_policy = ProjectPolicy::Preserve;
                        }
                        self.loaded.claude.toggle_project_server(&proj_path, &server_name);
                        if let Some(ref paths) = self.paths {
                            let p = paths.effective_claude_path(&self.loaded.config);
                            if let Ok(content) = self.loaded.claude.to_json_string(&self.outcome.canonical) {
                                let _ = crate::domain::storage::write_atomic_with_backup(&p, &content);
                            }
                        }
                    }
                }
                self.mode = ActiveMode::MatrixModal { server_name, selected };
            }
            _ => {}
        }
    }

    fn handle_delete_plugin_confirm_key(
        &mut self,
        key: KeyEvent,
        harness: HarnessKind,
        id: &str,
        display_name: &str,
        install_path: Option<&std::path::Path>,
    ) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Some(ref paths) = self.paths {
                    match remove_harness_plugin(
                        paths,
                        &self.loaded.config,
                        &mut self.loaded.plugins_ledger,
                        harness,
                        id,
                        install_path,
                    ) {
                        Ok(()) => {
                            if let Some(list) = self.loaded.harness_plugins.get_mut(&harness) {
                                list.retain(|p| p.id != id);
                            }
                            self.loaded.plugins_ledger_exists = true;
                            self.loaded.plugins_ledger_needs_save = false;
                            self.mode = ActiveMode::Normal;
                            self.clamp_plugin_selection();
                            self.status_message = Some((
                                format!(
                                    "✓ Removed '{display_name}' from {}",
                                    harness.display_name()
                                ),
                                false,
                            ));
                            self.trigger_mascot_reaction(
                                MascotReaction::Casting,
                                Duration::from_millis(700),
                            );
                            self.emit_fx(FxTrigger::Synced);
                        }
                        Err(e) => {
                            self.mode = ActiveMode::Normal;
                            self.status_message =
                                Some((format!("✗ Plugin removal error: {e}"), true));
                        }
                    }
                } else {
                    if let Some(list) = self.loaded.harness_plugins.get_mut(&harness) {
                        list.retain(|p| p.id != id);
                    }
                    self.loaded.plugins_ledger.map_for_mut(harness).remove(id);
                    self.mode = ActiveMode::Normal;
                    self.clamp_plugin_selection();
                    self.status_message = Some((
                        format!("✓ Removed '{display_name}' from {}", harness.display_name()),
                        false,
                    ));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(700),
                    );
                    self.emit_fx(FxTrigger::Synced);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = ActiveMode::Normal;
            }
            _ => {}
        }
    }

    fn handle_options_key(&mut self, key: KeyEvent, mut opt: OptionsState) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if opt.theme != self.loaded.config.ui.theme {
                    self.emit_fx(FxTrigger::ThemeChanged);
                }
                self.mode = ActiveMode::Normal;
                return;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                opt.focused = opt.focused.next();
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                opt.focused = opt.focused.prev();
            }
            KeyCode::Char(' ')
            | KeyCode::Right
            | KeyCode::Char('l')
            | KeyCode::Char('+')
            | KeyCode::Char('=') => {
                let prev_theme = opt.theme;
                opt.step(true);
                if opt.theme != prev_theme && opt.animations {
                    self.pending_fx.borrow_mut().push(FxTrigger::ThemeChanged);
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') => {
                let prev_theme = opt.theme;
                opt.step(false);
                if opt.theme != prev_theme && opt.animations {
                    self.pending_fx.borrow_mut().push(FxTrigger::ThemeChanged);
                }
            }
            KeyCode::Enter => {
                let prev_hide_ui = self.loaded.config.ui.hide_ui;
                let tools_changed = opt.apply_to(&mut self.loaded.config);
                if !self.loaded.config.ui.hero_mascot {
                    self.show_hero_mascot = false;
                }
                if self.loaded.config.ui.hide_ui && !prev_hide_ui {
                    self.reset_mascot_reveal();
                }
                if !self.loaded.config.ui.animations {
                    self.anim_tick = 0;
                    self.anim_elapsed = Duration::ZERO;
                    self.last_input_elapsed = Duration::ZERO;
                    self.gaze_target = (0.0, 0.0);
                    self.gaze_current = (0.0, 0.0);
                    self.mascot_reaction = MascotReaction::Idle;
                    self.reaction_remaining = Duration::ZERO;
                    self.reaction_total = Duration::ZERO;
                    self.fx.borrow_mut().clear();
                    self.pending_fx.borrow_mut().clear();
                } else {
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::Synced);
                }
                if tools_changed {
                    let env_ctx = self.current_env_context();
                    let harnesses = self.loaded.enabled_harness_servers();
                    self.outcome = reconcile_harnesses_with_env(
                        &self.outcome.canonical,
                        &harnesses,
                        Some(&env_ctx),
                    );
                    if let Some(ref paths) = self.paths {
                        self.loaded.harness_plugins = discover_all_harness_plugins(
                            paths,
                            &self.loaded.config,
                            &mut self.loaded.plugins_ledger,
                        );
                    } else {
                        self.loaded
                            .harness_plugins
                            .retain(|kind, _| self.loaded.config.tools.is_enabled(*kind));
                    }
                    self.clamp_selection();
                    self.clamp_plugin_selection();
                }
                self.mode = ActiveMode::Normal;
                if let Some(ref paths) = self.paths {
                    let res = if tools_changed && self.outcome.conflicts.is_empty() {
                        self.loaded.save_all(paths, &self.outcome.canonical)
                    } else {
                        self.loaded.save_config(paths)
                    };
                    match res {
                        Ok(()) => {
                            self.status_message =
                                Some(("✓ Saved options to config.toml".to_string(), false));
                        }
                        Err(e) => {
                            self.status_message =
                                Some((format!("✗ Failed to save config.toml: {e}"), true));
                        }
                    }
                } else {
                    self.status_message = Some(("✓ Applied options".to_string(), false));
                }
                return;
            }
            _ => {}
        }
        self.mode = ActiveMode::Options(opt);
    }

    /// Processes edited TOML content (or direct file edit) returned from `$EDITOR`.
    pub fn apply_editor_result(&mut self, req: EditorRequest, edited_content: &str) {
        if req.direct_file_path.is_some() {
            self.refresh_from_disk(false);
            return;
        }
        if edited_content.trim().is_empty() {
            self.status_message =
                Some(("! Editor buffer empty; changes aborted".to_string(), false));
            return;
        }
        match SingleServerSnippet::from_editor_toml(edited_content) {
            Ok(snippet) => {
                if let Some(conflict_idx) = req.conflict_index {
                    let _ = self.outcome.resolve(
                        conflict_idx,
                        ConflictResolution::Custom {
                            final_name: snippet.name.clone(),
                            config: snippet.config,
                        },
                    );
                    self.advance_conflict_modal(
                        conflict_idx,
                        &format!("Merged '{}' via $EDITOR", snippet.name),
                    );
                } else {
                    if let Some(ref orig) = req.original_name
                        && orig != &snippet.name
                    {
                        self.outcome.canonical.remove(orig);
                        self.probe_states.remove(orig);
                    }
                    self.probe_states.remove(&snippet.name);
                    self.outcome
                        .canonical
                        .insert(snippet.name.clone(), snippet.config);
                    self.mode = ActiveMode::Normal;
                    self.clamp_selection();
                    self.sync_to_disk(format!("Updated '{}' via $EDITOR", snippet.name));
                    self.trigger_mascot_reaction(
                        MascotReaction::Casting,
                        Duration::from_millis(750),
                    );
                    self.emit_fx(FxTrigger::ServerSaved);
                }
            }
            Err(err) => {
                self.trigger_mascot_reaction(MascotReaction::Startled, Duration::from_millis(750));
                self.status_message = Some((format!("✗ $EDITOR validation error: {err}"), true));
            }
        }
    }
}

fn edit_name_buffer(buf: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Backspace => {
            buf.pop();
        }
        KeyCode::Char(' ') => {
            if !buf.is_empty() && !buf.ends_with('-') {
                buf.push('-');
            }
        }
        KeyCode::Char(c) if !c.is_whitespace() => {
            buf.push(c);
        }
        _ => {}
    }
}

fn edit_text_buffer(buf: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Backspace => {
            buf.pop();
        }
        KeyCode::Char(c) => {
            buf.push(c);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_app() -> App {
        let mut loaded = LoadedConfigs::default();
        loaded.mcp.mcps.insert(
            "alpha".to_string(),
            McpServerConfig::new_http("https://alpha.example.com/mcp"),
        );
        loaded.mcp.mcps.insert(
            "beta".to_string(),
            McpServerConfig::new_stdio("/usr/bin/beta", vec!["--serve".to_string()]),
        );
        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();

        App::new(None, loaded, 80, 24)
    }

    #[test]
    fn scripted_key_navigation_toggle_filter_and_quit() {
        let mut app = sample_app();
        assert_eq!(app.selected, 0);
        assert!(app.outcome.canonical["alpha"].enabled);

        // Down -> selects "beta"
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.selected, 1);

        // Space -> toggles "beta" off
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.outcome.canonical["beta"].enabled);

        // '/' -> filter for "alpha"
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(app.mode, ActiveMode::Filtering);
        for ch in "alpha".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, ActiveMode::Normal);
        assert_eq!(app.visible_servers().len(), 1);
        assert_eq!(app.visible_servers()[0].0, "alpha");

        // Esc clears filter
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.visible_servers().len(), 2);

        // 'q' quits
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn guided_form_creates_and_validates_mcp() {
        let mut app = sample_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(matches!(app.mode, ActiveMode::Form(_)));

        // Submit empty form -> triggers validation error without crashing
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        if let ActiveMode::Form(ref f) = app.mode {
            assert!(f.error.is_some());
        } else {
            panic!("Expected form to remain open on validation error");
        }

        // Type name "gamma", Tab to Description, type "Gamma test server", Tab to Transport, Tab to Endpoint, type "/bin/gamma", Enter
        for ch in "gamma".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); // Description
        for ch in "Gamma test server".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); // Transport
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); // Endpoint
        for ch in "/bin/gamma".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.mode, ActiveMode::Normal);
        assert!(app.outcome.canonical.contains_key("gamma"));
        assert_eq!(
            app.outcome.canonical["gamma"].command.as_deref(),
            Some("/bin/gamma")
        );
        assert_eq!(
            app.outcome.canonical["gamma"].description.as_deref(),
            Some("Gamma test server")
        );

        // Pressing Space in the Name field converts spaces into a single hyphen (and ignores leading spaces)
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for ch in "  Astro  Docs".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        if let ActiveMode::Form(ref f) = app.mode {
            assert_eq!(f.name, "Astro-Docs");
        } else {
            panic!("Expected form to be open");
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        // Direct FormState::build_config rejects any name containing spaces
        let mut spaced_form = FormState::new_add();
        spaced_form.name = "Astro Docs".to_string();
        spaced_form.endpoint = "npx".to_string();
        let err = spaced_form.build_config().unwrap_err();
        assert!(err.contains("cannot contain spaces"));
    }

    #[test]
    fn scrolls_viewport_and_handles_page_first_last_and_filter_arrows() {
        let mut loaded = LoadedConfigs::default();
        for i in 1..=30 {
            loaded.mcp.mcps.insert(
                format!("server-{i:02}"),
                McpServerConfig::new_http(format!("https://s{i}.example.com/mcp")),
            );
        }
        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();

        // At height 24, viewport height is 24 - 7 = 17 rows
        let mut app = App::new(None, loaded, 80, 24);
        assert_eq!(app.list_viewport_height(), 17);
        assert_eq!(app.selected, 0);
        assert_eq!(app.scroll_offset, 0);

        // Navigate down 21 times (index 21 -> 22nd server)
        for _ in 0..21 {
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        }
        assert_eq!(app.selected, 21);
        // scroll_offset must have advanced so index 21 is inside [scroll_offset, scroll_offset + 17)
        assert_eq!(app.scroll_offset, 5);

        // 'g' jumps to first item and resets scroll_offset to 0
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.selected, 0);
        assert_eq!(app.scroll_offset, 0);

        // 'G' jumps to last item (index 29) and scrolls to bottom
        app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.selected, 29);
        assert_eq!(app.scroll_offset, 13);

        // PageUp moves back 10 items (29 -> 19)
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.selected, 19);
        assert_eq!(app.scroll_offset, 13);

        // PageUp again (19 -> 9), which is above offset 13, so scroll_offset adjusts to 9
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.selected, 9);
        assert_eq!(app.scroll_offset, 9);

        // PageDown moves forward 10 items (9 -> 19)
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.selected, 19);

        // Enter filter mode and use Down/Up arrows while filtering
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.selected, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn refresh_from_disk_reloads_external_changes_and_toggle_auto_refresh_persists() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        let mut initial = LoadedConfigs::default();
        initial.mcp.mcps.insert(
            "alpha".to_string(),
            McpServerConfig::new_http("https://alpha.example.com/mcp"),
        );
        initial.agy.servers = initial.mcp.mcps.clone();
        initial.claude.servers = initial.mcp.mcps.clone();

        let mut app = App::new(Some(paths.clone()), initial, 80, 24);
        assert!(app.auto_refresh_enabled());

        // Toggle auto-refresh off and verify it persists to config.toml
        app.toggle_auto_refresh();
        assert!(!app.auto_refresh_enabled());
        let saved_cfg = fs::read_to_string(&paths.config_toml).expect("read config.toml");
        assert!(saved_cfg.contains("enabled = false"));

        // External harness (Antigravity) adds "ext-server" on disk while maga is running
        fs::write(
            &paths.agy_json,
            r#"{
  "mcpServers": {
    "alpha": { "type": "http", "url": "https://alpha.example.com/mcp" },
    "ext-server": { "type": "http", "url": "https://ext.example.com/mcp" }
  }
}"#,
        )
        .expect("write external agy update");

        app.refresh_from_disk(false);
        assert!(app.outcome.canonical.contains_key("ext-server"));
        // And it automatically propagated to Claude Code's config on disk
        let reloaded = LoadedConfigs::load(&paths).expect("reload after refresh");
        assert!(reloaded.claude.servers.contains_key("ext-server"));
    }

    #[test]
    fn startup_auto_populates_empty_config_toml_even_when_conflicts_exist() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        // Write an empty config.toml and conflicting harness definitions
        fs::write(&paths.config_toml, "").expect("write empty config.toml");
        fs::write(
            &paths.agy_json,
            r#"{"mcpServers": {"conflict-srv": {"type": "http", "url": "https://a.example.com/mcp"}}}"#,
        )
        .expect("write agy");
        fs::write(
            &paths.claude_json,
            r#"{"mcpServers": {"conflict-srv": {"type": "http", "url": "https://c.example.com/mcp"}}}"#,
        )
        .expect("write claude");

        let loaded = LoadedConfigs::load(&paths).expect("load configs");
        assert!(loaded.config_needs_save);

        let app = App::new(Some(paths.clone()), loaded, 80, 24);
        assert_eq!(app.outcome.conflicts.len(), 1);
        // mcp.toml was not written because of unresolved MCP conflicts
        assert!(!paths.mcp_toml.exists());
        // config.toml WAS auto-populated with all default maga settings
        let populated = fs::read_to_string(&paths.config_toml).expect("read populated config.toml");
        assert!(populated.contains("[refresh]"));
        assert!(populated.contains("[tools.antigravity]"));
        assert!(populated.contains("config_path = \"~/.gemini/config/mcp_config.json\""));
        assert!(populated.contains("[tools.claude]"));
        assert!(populated.contains("config_path = \"~/.claude.json\""));
    }

    #[test]
    fn advance_animation_respects_animations_toggle_and_monochrome_mode() {
        let mut app = App::new(None, LoadedConfigs::default(), 80, 24);
        assert_eq!(app.anim_tick, 0);

        app.advance_animation(false);
        assert_eq!(app.anim_tick, 1);

        // Monochrome mode freezes frame to 0
        app.advance_animation(true);
        assert_eq!(app.anim_tick, 0);

        // Disabling ui.animations freezes frame to 0
        app.advance_animation(false);
        assert_eq!(app.anim_tick, 1);
        app.loaded.config.ui.animations = false;
        app.advance_animation(false);
        assert_eq!(app.anim_tick, 0);
    }

    #[test]
    fn options_modal_edits_saves_and_cancels_config_toml() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let mut app = App::new(Some(paths.clone()), LoadedConfigs::default(), 80, 24);

        // Open Options modal with 'o'
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert!(matches!(app.mode, ActiveMode::Options(_)));

        // Cycle theme (Blush -> Elite), move down to MascotMode and toggle (Lite -> Rich)
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        // Move down to Background and toggle on (false -> true), then HeroMascot and toggle off (true -> false)
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        // Move down to HideUi and toggle on, then Animations and toggle off
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        // Move down twice to RefreshInterval and increment (5s -> 6s)
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));

        // Save with Enter
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, ActiveMode::Normal);
        assert_eq!(app.loaded.config.ui.theme, ThemeName::Elite);
        assert_eq!(app.loaded.config.ui.mascot_mode, MascotMode::Rich);
        assert!(app.loaded.config.ui.background);
        assert!(!app.loaded.config.ui.hero_mascot);
        assert!(app.loaded.config.ui.hide_ui);
        assert!(!app.loaded.config.ui.animations);
        assert_eq!(app.loaded.config.refresh.interval_secs, 6);

        let saved_toml = fs::read_to_string(&paths.config_toml).expect("read config.toml");
        assert!(saved_toml.contains("theme = \"elite\""));
        assert!(saved_toml.contains("mascot_mode = \"rich\""));
        assert!(saved_toml.contains("background = true"));
        assert!(saved_toml.contains("hero_mascot = false"));
        assert!(saved_toml.contains("hide_ui = true"));
        assert!(saved_toml.contains("animations = false"));
        assert!(saved_toml.contains("interval_secs = 6"));

        // Pressing Esc in Normal mode when hide_ui is true restores the UI
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.loaded.config.ui.hide_ui);
        // Pressing 'b' toggles hide_ui back on and off
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert!(app.loaded.config.ui.hide_ui);
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert!(!app.loaded.config.ui.hide_ui);

        // Re-open Options, change theme to Indigo, then Esc to cancel without saving
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.mode, ActiveMode::Normal);
        assert_eq!(app.loaded.config.ui.theme, ThemeName::Elite);

        // Re-enable hero_mascot and verify 'm' toggles show_hero_mascot and resets reveal timer
        app.loaded.config.ui.hero_mascot = true;
        app.anim_elapsed = Duration::from_millis(900);
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(app.show_hero_mascot);
        assert_eq!(app.anim_elapsed, Duration::ZERO);
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(!app.show_hero_mascot);
    }

    #[test]
    fn smart_filter_supports_multi_token_status_and_field_qualifiers() {
        let mut loaded = LoadedConfigs::default();
        let mut ctx7 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        ctx7.description = Some("Upstash library documentation".to_string());
        loaded.mcp.mcps.insert("context7".to_string(), ctx7);
        loaded.mcp.mcps.insert(
            "github".to_string(),
            McpServerConfig::new_stdio(
                "npx",
                vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-github".to_string(),
                ],
            ),
        );
        let mut disabled_stdio =
            McpServerConfig::new_stdio("uvx", vec!["mcp-server-sqlite".to_string()]);
        disabled_stdio.enabled = false;
        disabled_stdio.description = Some("Local SQLite database analytics".to_string());
        loaded.mcp.mcps.insert("sqlite".to_string(), disabled_stdio);

        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();
        // Give claude a conflicting definition for "github" so it is marked conflicted
        loaded.claude.servers.insert(
            "github".to_string(),
            McpServerConfig::new_stdio("npx", vec!["--different".to_string()]),
        );

        let mut app = App::new(None, loaded, 80, 24);
        assert!(app.is_conflicted("github"));

        let names = |app: &App| -> Vec<String> {
            app.visible_servers()
                .into_iter()
                .map(|(n, _, _)| n)
                .collect()
        };

        // Plain status keywords
        app.filter_query = "enabled".to_string();
        assert_eq!(names(&app), vec!["context7", "github"]);

        app.filter_query = "disabled".to_string();
        assert_eq!(names(&app), vec!["sqlite"]);

        app.filter_query = "off".to_string();
        assert_eq!(names(&app), vec!["sqlite"]);

        app.filter_query = "conflict".to_string();
        assert_eq!(names(&app), vec!["github"]);

        // Free-text description matching
        app.filter_query = "upstash".to_string();
        assert_eq!(names(&app), vec!["context7"]);

        app.filter_query = "analytics".to_string();
        assert_eq!(names(&app), vec!["sqlite"]);

        // Multi-token AND query combining status, transport, and command
        app.filter_query = "enabled stdio npx".to_string();
        assert_eq!(names(&app), vec!["github"]);

        // Field-scoped key:value qualifiers
        app.filter_query = "is:enabled type:http".to_string();
        assert_eq!(names(&app), vec!["context7"]);

        app.filter_query = "desc:upstash".to_string();
        assert_eq!(names(&app), vec!["context7"]);

        app.filter_query = "description:database is:disabled".to_string();
        assert_eq!(names(&app), vec!["sqlite"]);

        app.filter_query = "is:disabled transport:stdio cmd:uvx".to_string();
        assert_eq!(names(&app), vec!["sqlite"]);

        app.filter_query = "is:conflict".to_string();
        assert_eq!(names(&app), vec!["github"]);

        app.filter_query = "status:off".to_string();
        assert_eq!(names(&app), vec!["sqlite"]);

        app.filter_query = "name:context7 url:https://mcp.context7.com".to_string();
        assert_eq!(names(&app), vec!["context7"]);

        // Disambiguating command vs server name with qualifiers
        app.filter_query = "name:npx".to_string();
        assert!(names(&app).is_empty());
        app.filter_query = "cmd:npx".to_string();
        assert_eq!(names(&app), vec!["github"]);

        // Incomplete key: prefix does not blank the list mid-typing
        app.filter_query = "is: type: cmd: name: desc: description:".to_string();
        assert_eq!(names(&app), vec!["context7", "github", "sqlite"]);

        // Unprefixed URL containing ':' matches endpoint_summary
        app.filter_query = "https://mcp.context7.com/mcp".to_string();
        assert_eq!(names(&app), vec!["context7"]);
    }

    #[test]
    fn keyboard_gaze_tracks_navigation_filtering_modals_and_autonomous_idle_wander() {
        let mut loaded = LoadedConfigs::default();
        for i in 1..=6 {
            loaded.mcp.mcps.insert(
                format!("srv-{i}"),
                McpServerConfig::new_http(format!("https://s{i}.example.com/mcp")),
            );
        }
        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();
        let mut app = App::new(None, loaded, 80, 24);

        // Initial gaze looks left toward top of SERVERS list
        let initial_state = app.mascot_anim_state();
        assert!(initial_state.gaze_x < -0.5);
        assert!(initial_state.gaze_y < -0.5);
        assert_eq!(initial_state.reaction, MascotReaction::Idle);

        // Navigating down to last server shifts gaze_target.1 downward and interpolates gaze_current
        for _ in 0..5 {
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        }
        assert!(app.gaze_target.1 > 0.5);
        app.set_frame_delta(Duration::from_millis(250));
        assert!(app.mascot_anim_state().gaze_y > 0.0);

        // Pressing Space triggers Casting reaction and decays back to Idle after duration
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(app.mascot_anim_state().reaction, MascotReaction::Casting);
        app.set_frame_delta(Duration::from_millis(800));
        assert_eq!(app.mascot_anim_state().reaction, MascotReaction::Idle);

        // Entering filter mode ('/') shifts gaze up-left and sets Scrying reaction
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(app.gaze_target, (-0.75, -0.75));
        assert_eq!(app.mascot_anim_state().reaction, MascotReaction::Scrying);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        // Opening Help modal ('?') centers horizontal gaze
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(app.gaze_target, (0.0, 0.0));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        // Leaving input idle for >= 2.5s transitions into autonomous idle gaze wander
        let focus_target = app.gaze_target;
        app.set_frame_delta(Duration::from_millis(2800));
        assert_ne!(app.gaze_target, focus_target);
    }

    #[test]
    fn details_focus_scrolls_and_probe_selected_polls_results() {
        use crate::domain::probe::{McpToolInfo, ProbeOutcome};

        let mut app = sample_app();
        assert!(!app.details_focused);
        assert_eq!(app.details_scroll, 0);

        // Press 't' to focus DETAILS panel
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert!(app.details_focused);
        assert!(app.gaze_target.0 > 0.0);

        // 'j' and PageDown scroll DETAILS without changing selected server
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.selected, 0);
        assert_eq!(app.details_scroll, 1);

        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.details_scroll, 11);

        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(app.details_scroll, 10);

        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.details_scroll, 0);

        // Esc exits DETAILS focus back to SERVERS
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.details_focused);

        // Simulate probe event via channel and verify poll_probe_results updates state & reaction
        app.probe_states
            .insert("alpha".to_string(), ServerProbeState::Probing);
        assert!(app.has_active_probe());
        app.probe_tx
            .send(ProbeEvent {
                name: "alpha".to_string(),
                state: ServerProbeState::Online(ProbeOutcome {
                    latency_ms: 42,
                    server_info: Some("alpha-srv v1.0".to_string()),
                    protocol_version: Some("2024-11-05".to_string()),
                    tools: vec![McpToolInfo {
                        name: "ping_tool".to_string(),
                        description: Some("Ping test".to_string()),
                    }],
                }),
            })
            .expect("send probe event");

        assert!(app.poll_probe_results());
        assert!(!app.has_active_probe());
        assert!(matches!(
            app.probe_state("alpha"),
            Some(ServerProbeState::Online(_))
        ));
        assert_eq!(app.mascot_anim_state().reaction, MascotReaction::Casting);
    }

    #[test]
    fn plugins_section_accordion_folding_toggling_filtering_and_refresh() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("create tempdir");
        let mut paths = StoragePaths::in_dir(dir.path());
        paths.agy_json = dir.path().join("agy").join("mcp_config.json");
        paths.copilot_json = dir.path().join("copilot").join("mcp-config.json");
        paths.grok_toml = dir.path().join("grok").join("config.toml");
        paths.opencode_json = dir.path().join("opencode").join("opencode.json");

        let cfg = MagaConfig::default();
        fs::create_dir_all(paths.agy_json.parent().unwrap()).unwrap();
        fs::create_dir_all(paths.opencode_json.parent().unwrap()).unwrap();
        let ponytail_dir = paths.effective_agy_plugins_dir(&cfg).join("ponytail");
        fs::create_dir_all(&ponytail_dir).unwrap();

        fs::write(
            paths.effective_agy_plugin_config_path(&cfg),
            r#"{"plugins": {"ponytail": {"enabled": true}}}"#,
        )
        .unwrap();
        fs::write(&paths.opencode_json, r#"{"plugin": ["opencode-wakatime"]}"#).unwrap();

        let loaded = LoadedConfigs::load(&paths).expect("load configs with plugins");
        let mut app = App::new(Some(paths.clone()), loaded, 80, 24);
        assert_eq!(app.section, WorkspaceSection::Mcps);
        assert_eq!(app.total_plugins_counts(), (2, 2));
        assert!(paths.plugins_toml.exists());

        // Switch to Plugins section with '2' (and verify Tab cycles Mcps -> Plugins -> Guidelines -> Refs -> Mcps)
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Plugins);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Guidelines);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Refs);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Mcps);
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Plugins);

        // Visible rows: 5 harness headers + 2 plugins = 7 rows
        // Row 0: Antigravity header (1/1)
        // Row 1: ponytail plugin
        let rows = app.visible_plugin_rows();
        assert_eq!(rows.len(), 7);
        assert!(matches!(
            rows[0],
            PluginRow::HarnessHeader {
                harness: HarnessKind::Antigravity,
                enabled_count: 1,
                total_count: 1,
                collapsed: false
            }
        ));

        // Pressing Space or Enter on row 0 folds Antigravity group
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(
            app.collapsed_plugin_groups
                .contains(&HarnessKind::Antigravity)
        );
        assert_eq!(app.visible_plugin_rows().len(), 6);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            !app.collapsed_plugin_groups
                .contains(&HarnessKind::Antigravity)
        );
        assert_eq!(app.visible_plugin_rows().len(), 7);

        // Move down to row 1 (ponytail) and press 'h' -> folds parent Antigravity group and moves cursor to row 0
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.plugin_selected, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert!(
            app.collapsed_plugin_groups
                .contains(&HarnessKind::Antigravity)
        );
        assert_eq!(app.plugin_selected, 0);
        // Pressing 'l' on collapsed Antigravity header expands it
        app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert!(
            !app.collapsed_plugin_groups
                .contains(&HarnessKind::Antigravity)
        );
        assert_eq!(app.visible_plugin_rows().len(), 7);

        // Move down to row 1 (ponytail) and toggle OFF with Space
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.plugin_selected, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(app.total_plugins_counts(), (1, 2));

        // Pressing 'E' in Plugins view queues direct_file_path for $EDITOR (manifest or config_path)
        app.handle_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT));
        let editor_req = app.pending_editor.take().expect("pending_editor");
        assert_eq!(
            editor_req.direct_file_path,
            Some(paths.effective_agy_plugin_config_path(&cfg))
        );

        let agy_cfg_text =
            fs::read_to_string(paths.effective_agy_plugin_config_path(&cfg)).unwrap();
        assert!(agy_cfg_text.contains(r#""enabled": false"#));

        // Filter plugins with '/'
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "harness:open waka".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let filtered_rows = app.visible_plugin_rows();
        // Only OpenCode header + opencode-wakatime row should match
        assert_eq!(filtered_rows.len(), 2);
        assert!(matches!(
            filtered_rows[0],
            PluginRow::HarnessHeader {
                harness: HarnessKind::OpenCode,
                ..
            }
        ));

        // Select opencode-wakatime and toggle OFF -> removed from opencode.json, retained in plugins.toml
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(app.total_plugins_counts(), (0, 2));
        let oc_text = fs::read_to_string(&paths.opencode_json).unwrap();
        assert!(!oc_text.contains("opencode-wakatime"));

        // Refresh from disk ('r') keeps disabled opencode-wakatime visible via plugins.toml
        // and preserves selection on opencode-wakatime (row 1 of filtered view)
        assert_eq!(app.plugin_selected, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(app.total_plugins_counts(), (0, 2));
        assert_eq!(app.plugin_selected, 1);

        // Clear filter with Esc
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.plugin_filter_query.is_empty());
        assert_eq!(app.visible_plugin_rows().len(), 7);

        // Select opencode-wakatime (last row, index 6) and press 'd' -> ConfirmDeletePlugin modal
        app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.plugin_selected, 6);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(matches!(
            app.mode,
            ActiveMode::ConfirmDeletePlugin {
                harness: HarnessKind::OpenCode,
                ref id,
                ..
            } if id == "opencode-wakatime"
        ));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(app.mode, ActiveMode::Normal);
        assert_eq!(app.total_plugins_counts(), (0, 1));
        assert_eq!(app.visible_plugin_rows().len(), 6);

        // Verify desc:, skill:, hook:, and mcp: filter qualifiers against bundled metadata
        if let Some(agy_plugins) = app
            .loaded
            .harness_plugins
            .get_mut(&HarnessKind::Antigravity)
            && let Some(p) = agy_plugins.first_mut()
        {
            p.description = Some("Ponytail workflow helper".to_string());
            p.bundled.skill_names = vec!["review".to_string(), "triage".to_string()];
            p.bundled.hook_names = vec!["PreToolUse".to_string()];
            p.bundled.mcp_names = vec!["sqlite-db".to_string()];
        }
        app.plugin_filter_query = "desc:workflow skill:triage hook:pretool mcp:sqlite".to_string();
        assert_eq!(app.visible_plugin_rows().len(), 2);
        app.plugin_filter_query = "skill:nonexistent".to_string();
        assert!(app.visible_plugin_rows().is_empty());

        // Clear filter, select directory-installed ponytail (row 1), press 'd' -> ConfirmDeletePlugin with install_path, confirm 'y' -> deletes ponytail_dir
        app.plugin_filter_query.clear();
        app.clamp_plugin_selection();
        app.plugin_selected = 1;
        assert!(ponytail_dir.exists());
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(matches!(
            app.mode,
            ActiveMode::ConfirmDeletePlugin {
                harness: HarnessKind::Antigravity,
                ref id,
                install_path: Some(ref p),
                ..
            } if id == "ponytail" && p == &ponytail_dir
        ));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(app.mode, ActiveMode::Normal);
        assert!(!ponytail_dir.exists());
        assert_eq!(app.total_plugins_counts(), (0, 0));
    }

    #[test]
    fn app_reconciles_and_probes_using_scoped_dotenv_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        std::fs::write(
            dir.path().join(".env.local"),
            "PROBE_PORT=65531\nDB_PASS=ScopedPass99\n",
        )
        .expect("write .env.local");

        let mut loaded = LoadedConfigs::default();
        let templated =
            McpServerConfig::new_http("http://127.0.0.1:${PROBE_PORT}/mcp?p=${DB_PASS}");
        loaded.mcp.mcps.insert("templated".to_string(), templated);
        loaded
            .save_all(&paths, &loaded.mcp.mcps.clone())
            .expect("save_all");

        let reloaded = LoadedConfigs::load(&paths).expect("reload");
        let mut app = App::new(Some(paths), reloaded, 120, 40);
        assert_eq!(app.mode, ActiveMode::Normal);
        assert!(app.outcome.conflicts.is_empty());

        app.probe_selected();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.has_active_probe() && std::time::Instant::now() < deadline {
            app.poll_probe_results();
            std::thread::sleep(Duration::from_millis(20));
        }

        match app.probe_state("templated") {
            Some(ServerProbeState::Failed { error, .. }) => {
                assert!(
                    !error.contains("${PROBE_PORT}"),
                    "probe URL should have been interpolated, got error: {error}"
                );
            }
            other => panic!("expected connection failure to closed port, got: {other:?}"),
        }
    }

    #[test]
    fn guidelines_tab_navigation_and_editor_trigger() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = StoragePaths {
            config_toml: dir.path().join("config.toml"),
            mcp_toml: dir.path().join("mcp.toml"),
            plugins_toml: dir.path().join("plugins.toml"),
            profiles_toml: dir.path().join("profiles.toml"),
            refs_toml: dir.path().join("refs.toml"),
            refs_cache_dir: dir.path().join("refs"),
            agy_json: dir.path().join("mcp_config.json"),
            claude_json: dir.path().join(".claude.json"),
            copilot_json: dir.path().join("copilot-config.json"),
            grok_toml: dir.path().join("grok.toml"),
            opencode_json: dir.path().join("opencode.json"),
            agy_overridden: true,
            claude_overridden: true,
            copilot_overridden: true,
            grok_overridden: true,
            opencode_overridden: true,
            isolated: true,
        };

        let loaded = LoadedConfigs::load(&paths).expect("load");
        let mut app = App::new(Some(paths), loaded, 100, 30);
        assert_eq!(app.section, WorkspaceSection::Mcps);

        // Switch to Guidelines view with '3'
        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Guidelines);

        // Navigation in guidelines view
        let total = app.guidelines_statuses.len();
        if total > 0 {
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
            assert_eq!(app.guidelines_selected, 1 % total);
            app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
            assert_eq!(app.guidelines_selected, 0);
        }

        // Press 'e' to open editor
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(app.pending_editor.is_some());
        let req = app.pending_editor.take().unwrap();
        assert!(req.direct_file_path.is_some());
    }

    #[test]
    fn refs_tab_navigation_and_actions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = StoragePaths::in_dir(dir.path());
        let mut loaded = LoadedConfigs::load(&paths).expect("load");

        loaded.refs.refs.insert(
            "pangolin".to_string(),
            crate::domain::refs::RefRepoEntry {
                url: "https://github.com/fosrl/docs".to_string(),
                branch: Some("main".to_string()),
                description: Some("Pangolin documentation".to_string()),
                path: None,
            },
        );
        loaded.refs.refs.insert(
            "react".to_string(),
            crate::domain::refs::RefRepoEntry {
                url: "https://github.com/reactjs/react.dev".to_string(),
                branch: None,
                description: Some("React documentation".to_string()),
                path: None,
            },
        );

        let mut app = App::new(Some(paths.clone()), loaded, 100, 30);
        assert_eq!(app.section, WorkspaceSection::Mcps);

        // Switch to Refs view with '4'
        app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Refs);

        // Navigation in refs view
        assert_eq!(app.refs_selected, 0);
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.refs_selected, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(app.refs_selected, 0);

        // Press 'e' to open refs.toml in editor
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(app.pending_editor.is_some());
        let req = app.pending_editor.take().unwrap();
        assert_eq!(req.direct_file_path, Some(paths.refs_toml));

        // Tab cycling: Refs -> Mcps
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.section, WorkspaceSection::Mcps);
    }
}
