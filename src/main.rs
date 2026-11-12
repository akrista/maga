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

    /// Reconcile and synchronize canonical MCPs to all enabled harnesses non-interactively.
    #[arg(long)]
    sync: bool,

    /// Force authoritative sync from mcp.toml to all enabled harnesses, overriding conflicts.
    #[arg(long)]
    force: bool,

    /// Check whether canonical state and harnesses are converged without writing any files.
    #[arg(long)]
    check: bool,

    /// Check the status of global guidelines links across all harnesses without writing any files.
    #[arg(long)]
    check_guidelines: bool,

    /// Synchronize canonical guidelines (~/.agents/AGENTS.md) to all harness native locations.
    #[arg(long)]
    sync_guidelines: bool,

    /// Check whether a newer version of maga is available on GitHub Releases without updating.
    #[arg(long)]
    check_update: bool,

    /// Download and install the latest release of maga in-place.
    #[arg(long)]
    update: bool,
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

    if cli.check_update {
        println!("\x1b[1;36m✦ maga update check ✦\x1b[0m");
        match crate::domain::update::check_latest_release(crate::domain::update::DEFAULT_REPO) {
            Ok(info) => {
                println!("  Current version: v{}", info.current_version);
                println!("  Latest version:  {}", info.latest_version);
                if info.is_newer {
                    println!("\x1b[1;33m  Update available! Run `maga --update` to upgrade.\x1b[0m");
                    if let Some(notes) = &info.release_notes_url {
                        println!("  Release notes:   {notes}");
                    }
                } else {
                    println!("\x1b[1;32m  maga is already up to date.\x1b[0m");
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("\x1b[1;31m✗ Update check failed:\x1b[0m {e}");
                std::process::exit(1);
            }
        }
    }

    if cli.update {
        println!("\x1b[1;36m✦ Checking for updates... ✦\x1b[0m");
        match crate::domain::update::check_latest_release(crate::domain::update::DEFAULT_REPO) {
            Ok(info) => {
                println!("  Current version: v{}", info.current_version);
                println!("  Latest version:  {}", info.latest_version);
                if !info.is_newer {
                    println!("\x1b[1;32m✓ maga is already up to date (v{}).\x1b[0m", info.current_version);
                    std::process::exit(0);
                }
                println!("\x1b[1;33m✦ Downloading update {} for {}... ✦\x1b[0m", info.latest_version, info.target);
                match crate::domain::update::apply_update(&info) {
                    Ok(()) => {
                        println!("\x1b[1;32m✓ Successfully updated maga to {}!\x1b[0m", info.latest_version);
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("\x1b[1;31m✗ Failed to apply update:\x1b[0m {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("\x1b[1;31m✗ Update check failed:\x1b[0m {e}");
                std::process::exit(1);
            }
        }
    }

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

    if cli.check_guidelines {
        match crate::domain::guidelines::check_guidelines_status() {
            Ok(statuses) => {
                println!("\x1b[1;36m✦ Global Guidelines Status ✦\x1b[0m");
                let mut all_ok = true;
                for s in statuses {
                    let mark = if s.exists && s.matches_canonical {
                        "\x1b[1;32m✓\x1b[0m"
                    } else {
                        all_ok = false;
                        "\x1b[1;31m✗\x1b[0m"
                    };
                    let sym_tag = if s.is_symlink { " (symlink)" } else { "" };
                    println!(
                        "  {mark} {:<22} -> {}{}",
                        s.harness.to_string(),
                        s.path.display(),
                        sym_tag
                    );
                }
                if all_ok {
                    std::process::exit(0);
                } else {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("\x1b[1;31m✗ Error checking guidelines:\x1b[0m {e}");
                std::process::exit(3);
            }
        }
    }

    if cli.sync_guidelines {
        match crate::domain::guidelines::sync_guidelines() {
            Ok(results) => {
                println!("\x1b[1;32m✓ Synchronizing Global Guidelines\x1b[0m");
                let mut all_ok = true;
                for r in results {
                    if r.success {
                        println!(
                            "  \x1b[1;32m✓\x1b[0m {:<22} -> {}",
                            r.harness.to_string(),
                            r.path.display()
                        );
                    } else {
                        all_ok = false;
                        println!(
                            "  \x1b[1;31m✗\x1b[0m {:<22} -> {} ({})",
                            r.harness.to_string(),
                            r.path.display(),
                            r.action
                        );
                    }
                }
                if all_ok {
                    std::process::exit(0);
                } else {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("\x1b[1;31m✗ Error syncing guidelines:\x1b[0m {e}");
                std::process::exit(3);
            }
        }
    }

    let mut loaded = match LoadedConfigs::load(&paths) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("\x1b[1;31m✗ maga error:\x1b[0m {e}");
            std::process::exit(3);
        }
    };

    // Headless --check mode: zero disk mutation, clean exit codes (0 = in-sync, 1 = drift, 2 = conflict)
    if cli.check {
        match loaded.check_headless(&paths) {
            crate::domain::reconcile::ConvergenceStatus::Converged => {
                println!(
                    "\x1b[1;32m✓\x1b[0m All enabled harnesses are converged with canonical registry."
                );
                std::process::exit(0);
            }
            crate::domain::reconcile::ConvergenceStatus::Drift {
                drift_harnesses,
                imported_count,
            } => {
                println!(
                    "\x1b[1;33m!\x1b[0m Drift detected ({} untracked, {} harness(es) out of sync).",
                    imported_count,
                    drift_harnesses.len()
                );
                for h in drift_harnesses {
                    println!("    - {}", h.display_name());
                }
                std::process::exit(1);
            }
            crate::domain::reconcile::ConvergenceStatus::Conflicted { conflicts } => {
                eprintln!(
                    "\x1b[1;31m!\x1b[0m {} conflict(s) detected across harnesses:",
                    conflicts.len()
                );
                for c in &conflicts {
                    eprintln!("    - {} ({:?})", c.name, c.differing_fields());
                }
                std::process::exit(2);
            }
        }
    }

    // Headless --sync mode: two-way reconciliation or forced authoritative push
    if cli.sync {
        match loaded.sync_headless(&paths, cli.force) {
            Ok(true) => {
                let force_msg = if cli.force { " (forced)" } else { "" };
                println!(
                    "\x1b[1;32m✓\x1b[0m Successfully synchronized canonical registry to all enabled harnesses{force_msg}."
                );
                std::process::exit(0);
            }
            Ok(false) => {
                println!(
                    "\x1b[1;32m✓\x1b[0m All enabled harnesses are already converged (nothing to sync)."
                );
                std::process::exit(0);
            }
            Err(crate::domain::storage::HeadlessSyncError::Conflicted(conflicts)) => {
                eprintln!(
                    "\x1b[1;31m✗ Sync aborted:\x1b[0m {} conflict(s) detected across harnesses. Launch `maga` to resolve or use `--force` to push canonical mcp.toml.",
                    conflicts.len()
                );
                for c in &conflicts {
                    eprintln!("    - {} ({:?})", c.name, c.differing_fields());
                }
                std::process::exit(2);
            }
            Err(crate::domain::storage::HeadlessSyncError::Io(e)) => {
                eprintln!("\x1b[1;31m✗ Sync failed:\x1b[0m {e}");
                std::process::exit(3);
            }
        }
    }

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
