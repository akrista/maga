use crate::app::{ActiveMode, App, EditorRequest};
use crate::theme::Theme;
use crate::ui;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::Builder;

/// Runs the main interactive event loop, including `$EDITOR` suspension, periodic auto-refresh, and terminal restoration.
pub fn run_event_loop<B>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    theme: &Theme,
) -> io::Result<()>
where
    B: Backend + Write,
    io::Error: From<B::Error>,
{
    let mut last_refresh = Instant::now();
    let mut last_anim = Instant::now();
    let mut last_frame = Instant::now();
    let anim_interval = Duration::from_millis(250);

    loop {
        app.poll_probe_results();

        if app.auto_refresh_enabled()
            && app.mode == ActiveMode::Normal
            && app.pending_editor.is_none()
            && last_refresh.elapsed() >= app.auto_refresh_interval()
        {
            app.refresh_from_disk(true);
            last_refresh = Instant::now();
        }

        if app.animations_enabled() && last_anim.elapsed() >= anim_interval {
            app.advance_animation(theme.mono);
            last_anim = Instant::now();
        }

        let frame_delta = last_frame.elapsed().max(Duration::from_millis(1));
        last_frame = Instant::now();
        app.set_frame_delta(frame_delta);

        terminal.draw(|f| ui::draw(f, app, theme))?;

        if let Some(req) = app.pending_editor.take() {
            let edited = launch_external_editor(terminal, &req)?;
            app.apply_editor_result(req, &edited);
            last_refresh = Instant::now();
            last_frame = Instant::now();
            continue;
        }

        if app.should_quit {
            break;
        }

        let poll_timeout = compute_poll_timeout(app, theme.mono);
        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key);
                }
                Event::Resize(w, h) => {
                    app.resize(w, h);
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

/// Computes the adaptive poll interval:
/// - `16ms` (~60 FPS) during active `tachyonfx` transitions
/// - `33ms` (~30 FPS, matching `ascii-effect.tsx`'s `frameRate = 30`) when `ui.animations` is enabled and any animated visual tier (`mascot_mode == Rich`, Hero Showcase, Background Watermark, or Background-Only) is visible
/// - `50ms` when all animated surfaces are hidden, in `lite` mode without background/hero, disabled, or monochrome.
pub fn compute_poll_timeout(app: &App, mono: bool) -> Duration {
    if !mono && app.has_active_fx_transition() {
        Duration::from_millis(16)
    } else if (!mono && app.has_visible_animated_surface()) || app.has_active_probe() {
        Duration::from_millis(33)
    } else {
        Duration::from_millis(50)
    }
}

/// Suspends raw mode and the alternate screen, launches `$VISUAL` / `$EDITOR` (fallback `vi`),
/// reads the resulting TOML snippet, and restores the TUI cleanly.
fn launch_external_editor<B>(
    terminal: &mut Terminal<B>,
    req: &EditorRequest,
) -> io::Result<String>
where
    B: Backend + Write,
    io::Error: From<B::Error>,
{
    if let Some(ref target_path) = req.direct_file_path {
        if let Some(parent) = target_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        let editor = resolve_editor_command();
        let status_res = Command::new(&editor).arg(target_path).status();

        crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen)?;
        enable_raw_mode()?;
        terminal.hide_cursor()?;
        terminal.clear()?;

        return match status_res {
            Ok(_) => Ok(String::new()),
            Err(e) => Err(io::Error::other(format!(
                "Failed to launch editor '{editor}': {e}"
            ))),
        };
    }

    let mut tmp = Builder::new().prefix("maga-").suffix(".toml").tempfile()?;
    tmp.write_all(req.initial_toml.as_bytes())?;
    tmp.flush()?;
    let tmp_path = tmp.path().to_path_buf();

    // Suspend TUI terminal state
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let editor = resolve_editor_command();
    let status_res = Command::new(&editor).arg(&tmp_path).status();

    // Always restore TUI terminal state before checking editor status
    crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    match status_res {
        Ok(status) if status.success() => fs::read_to_string(&tmp_path),
        Ok(_) => Ok(String::new()),
        Err(e) => Err(io::Error::other(format!(
            "Failed to launch editor '{editor}': {e}"
        ))),
    }
}

pub fn resolve_editor_command() -> String {
    std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{MascotMode, McpServerConfig};
    use crate::domain::storage::LoadedConfigs;

    #[test]
    fn invalid_editor_toml_sets_error_banner_without_crashing() {
        let mut app = App::new(None, LoadedConfigs::default(), 80, 24);
        let req = EditorRequest {
            original_name: None,
            conflict_index: None,
            initial_toml: String::new(),
            direct_file_path: None,
        };

        app.apply_editor_result(req.clone(), "not valid toml = = =");
        let (msg, is_err) = app.status_message.as_ref().expect("status message");
        assert!(*is_err);
        assert!(msg.contains("validation error"));

        let valid = r#"
name = "custom-mcp"
enabled = true
transport = "http"
url = "https://mcp.example.com/mcp"
"#;
        app.apply_editor_result(req, valid);
        assert!(app.outcome.canonical.contains_key("custom-mcp"));
        assert_eq!(
            app.outcome.canonical["custom-mcp"],
            McpServerConfig::new_http("https://mcp.example.com/mcp")
        );
    }

    #[test]
    fn adaptive_poll_timeout_switches_between_60fps_30fps_and_idle() {
        let mut loaded = LoadedConfigs::default();
        loaded.mcp.mcps.insert(
            "srv".to_string(),
            McpServerConfig::new_http("https://example.com/mcp"),
        );
        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();

        let mut app = App::new(None, loaded, 80, 24);
        // Startup trigger is queued initially -> 16ms poll timeout in color mode
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(16));
        // Monochrome mode always uses 50ms poll timeout
        assert_eq!(compute_poll_timeout(&app, true), Duration::from_millis(50));

        // Clear transient triggers: default Lite mascot mode with non-empty servers & background=false relaxes to 50ms
        let _ = app.take_pending_fx();
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(50));

        // Hero Showcase toggled on ('m') in Lite mode runs at 33ms (~30 FPS)
        app.show_hero_mascot = true;
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(33));
        app.show_hero_mascot = false;

        // Background Watermark enabled in Lite mode runs at 33ms (~30 FPS)
        app.loaded.config.ui.background = true;
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(33));
        app.loaded.config.ui.background = false;

        // Background-Only (hide_ui) mode runs at 33ms (~30 FPS)
        app.loaded.config.ui.hide_ui = true;
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(33));
        app.loaded.config.ui.hide_ui = false;

        // Rich companion mode with animations enabled uses 33ms (~30 FPS)
        app.loaded.config.ui.mascot_mode = MascotMode::Rich;
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(33));

        // Disabling animations switches to 50ms poll timeout
        app.loaded.config.ui.animations = false;
        assert_eq!(compute_poll_timeout(&app, false), Duration::from_millis(50));
    }
}
