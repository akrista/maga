pub mod app;
pub mod domain;
pub mod event;
pub mod fx;
pub mod keybind;
pub mod mascot;
pub mod screens;
pub mod theme;
pub mod ui;

use crate::app::App;
use crate::domain::storage::{LoadedConfigs, StoragePaths};
use crate::theme::Theme;
use clap::Parser;
use crossterm::cursor::Show;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

/// Unified TUI manager for global MCP servers across Antigravity, Claude Code, Copilot, Grok, and OpenCode.
#[derive(Debug, Parser)]
#[command(name = "maga", version, about)]
struct Cli {
    /// Override path to general `maga` TOML config (`~/.config/maga/config.toml`).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override path to canonical `maga` MCP registry (`~/.config/maga/mcp.toml`).
    #[arg(long)]
    mcp_config: Option<PathBuf>,

    /// Override path to `maga` per-harness plugins ledger (`~/.config/maga/plugins.toml`).
    #[arg(long)]
    plugins_config: Option<PathBuf>,

    /// Override path to Antigravity `mcp_config.json` (`~/.gemini/config/mcp_config.json`).
    #[arg(long)]
    agy_config: Option<PathBuf>,

    /// Override path to Claude Code `.claude.json` (`~/.claude.json`).
    #[arg(long)]
    claude_config: Option<PathBuf>,

    /// Override path to GitHub Copilot `mcp-config.json` (`~/.copilot/mcp-config.json`).
    #[arg(long)]
    copilot_config: Option<PathBuf>,

    /// Override path to Grok `config.toml` (`~/.grok/config.toml`).
    #[arg(long)]
    grok_config: Option<PathBuf>,

    /// Override path to OpenCode `opencode.json` (`~/.config/opencode/opencode.json`).
    #[arg(long)]
    opencode_config: Option<PathBuf>,

    /// Print reconciled MCP server status in plain text and exit without launching the TUI.
    #[arg(long)]
    dump: bool,
}

/// RAII guard that ensures raw mode and alternate screen are always restored on exit or panic.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        crossterm::execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen, Show);
        original_hook(panic_info);
    }));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let mut paths = StoragePaths::resolve_default()?;
    if let Some(cfg) = cli.config {
        paths.config_toml = cfg;
    }
    if let Some(mcp_cfg) = cli.mcp_config {
        paths.mcp_toml = mcp_cfg;
    }
    if let Some(plugins_cfg) = cli.plugins_config {
        paths.plugins_toml = plugins_cfg;
    }
    if let Some(agy) = cli.agy_config {
        paths.agy_json = agy;
        paths.agy_overridden = true;
    }
    if let Some(claude) = cli.claude_config {
        paths.claude_json = claude;
        paths.claude_overridden = true;
    }
    if let Some(copilot) = cli.copilot_config {
        paths.copilot_json = copilot;
        paths.copilot_overridden = true;
    }
    if let Some(grok) = cli.grok_config {
        paths.grok_toml = grok;
        paths.grok_overridden = true;
    }
    if let Some(opencode) = cli.opencode_config {
        paths.opencode_json = opencode;
        paths.opencode_overridden = true;
    }

    let mut loaded = LoadedConfigs::load(&paths)?;
    if !loaded.config_exists || loaded.config_needs_save {
        loaded.save_config(&paths)?;
    }

    // Non-interactive plain output if `--dump` is passed or stdout is not a TTY
    if cli.dump || !io::stdout().is_terminal() {
        let app = App::new(None, loaded, 80, 24);
        println!(
            "maga ({} total MCPs, {} conflicts)",
            app.outcome.canonical.len(),
            app.outcome.conflicts.len()
        );
        for (name, cfg, conflicted) in app.visible_servers() {
            let status = if conflicted {
                "!"
            } else if cfg.enabled {
                "✓"
            } else {
                "○"
            };
            println!(
                "  {status} {name:<28} {:<6} {}",
                cfg.transport.as_str(),
                cfg.endpoint_summary()
            );
        }
        let (enabled_plugins, total_plugins) = app.total_plugins_counts();
        println!("\nplugins ({enabled_plugins}/{total_plugins} enabled)");
        for kind in crate::domain::model::HarnessKind::ALL {
            if !app.loaded.config.tools.is_enabled(kind) {
                continue;
            }
            let list = app
                .loaded
                .harness_plugins
                .get(&kind)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if list.is_empty() {
                continue;
            }
            let enabled_in_harness = list.iter().filter(|p| p.enabled).count();
            println!(
                "  {} ({}/{})",
                kind.display_name(),
                enabled_in_harness,
                list.len()
            );
            for p in list {
                let status = if p.enabled { "✓" } else { "○" };
                let badge = p
                    .bundled
                    .summary_badge()
                    .map(|b| format!("  {b}"))
                    .unwrap_or_default();
                let ver = p
                    .version
                    .as_deref()
                    .map(|v| format!(" v{v}"))
                    .unwrap_or_default();
                println!(
                    "    {status} {:<26} {:<6}{}{}",
                    p.display_name,
                    p.kind.as_str(),
                    ver,
                    badge
                );
            }
        }
        return Ok(());
    }

    install_panic_hook();
    let _guard = TerminalGuard::enter()?;

    let (w, h) = size().unwrap_or((80, 24));
    let mut app = App::new(Some(paths), loaded, w, h);
    let theme = Theme::from_env();

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    event::run_event_loop(&mut terminal, &mut app, &theme)?;

    Ok(())
}
