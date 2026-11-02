use crate::app::{OptionsField, OptionsState};
use crate::screens::form::centered_rect;
use crate::theme::{PADDING_SM, PADDING_XS, Theme};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};

/// Renders the interactive in-TUI Options modal (`o` keybinding).
pub fn draw_options_modal(f: &mut Frame, area: Rect, opt: &OptionsState, theme: &Theme) {
    let modal_area = centered_rect(72, 23, area);
    f.render_widget(Clear, modal_area);

    let block = Block::default()
        .title(Span::styled(
            " OPTIONS · config.toml (j/k: field  Space/←/→: change  Enter: save  Esc: cancel) ",
            theme.header,
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.active_border)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "[ui] Appearance & Mascot",
        theme.muted,
    )));

    lines.push(render_option_line(
        "Theme             ",
        &format!("◂ {} ▸", opt.theme.display_name()),
        opt.focused == OptionsField::Theme,
        theme,
    ));
    lines.push(render_option_line(
        "Companion (Chibi) ",
        &format!("◂ {} ▸", opt.mascot_mode.display_name()),
        opt.focused == OptionsField::MascotMode,
        theme,
    ));
    lines.push(render_option_line(
        "Background Art    ",
        if opt.background {
            "[x] Enabled (Riverflow Watermark)"
        } else {
            "[ ] Disabled"
        },
        opt.focused == OptionsField::Background,
        theme,
    ));
    lines.push(render_option_line(
        "Hero Mascot (m)   ",
        if opt.hero_mascot {
            "[x] Enabled (Arcane Showcase)"
        } else {
            "[ ] Disabled"
        },
        opt.focused == OptionsField::HeroMascot,
        theme,
    ));
    lines.push(render_option_line(
        "Hide UI (b)       ",
        if opt.hide_ui {
            "[x] Background Only (UI Hidden)"
        } else {
            "[ ] Normal Workspace UI"
        },
        opt.focused == OptionsField::HideUi,
        theme,
    ));
    lines.push(render_option_line(
        "Animations        ",
        if opt.animations {
            "[x] Enabled"
        } else {
            "[ ] Disabled"
        },
        opt.focused == OptionsField::Animations,
        theme,
    ));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[refresh] Live Reload",
        theme.muted,
    )));

    lines.push(render_option_line(
        "Auto-Refresh      ",
        if opt.refresh_enabled {
            "[x] Enabled"
        } else {
            "[ ] Disabled"
        },
        opt.focused == OptionsField::AutoRefresh,
        theme,
    ));
    lines.push(render_option_line(
        "Refresh Interval  ",
        &format!("◂ {}s ▸  (-/+ or ←/→)", opt.refresh_interval_secs),
        opt.focused == OptionsField::RefreshInterval,
        theme,
    ));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[tools] Target Harnesses",
        theme.muted,
    )));

    lines.push(render_option_line(
        "Antigravity       ",
        if opt.antigravity_enabled {
            "[x] Synced"
        } else {
            "[ ] Ignored"
        },
        opt.focused == OptionsField::Antigravity,
        theme,
    ));
    lines.push(render_option_line(
        "Claude Code       ",
        if opt.claude_enabled {
            "[x] Synced"
        } else {
            "[ ] Ignored"
        },
        opt.focused == OptionsField::Claude,
        theme,
    ));
    lines.push(render_option_line(
        "GitHub Copilot    ",
        if opt.copilot_enabled {
            "[x] Synced"
        } else {
            "[ ] Ignored"
        },
        opt.focused == OptionsField::Copilot,
        theme,
    ));
    lines.push(render_option_line(
        "Grok CLI          ",
        if opt.grok_enabled {
            "[x] Synced"
        } else {
            "[ ] Ignored"
        },
        opt.focused == OptionsField::Grok,
        theme,
    ));
    lines.push(render_option_line(
        "OpenCode          ",
        if opt.opencode_enabled {
            "[x] Synced"
        } else {
            "[ ] Ignored"
        },
        opt.focused == OptionsField::OpenCode,
        theme,
    ));

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, modal_area);
}

fn render_option_line<'a>(label: &'a str, value: &str, focused: bool, theme: &Theme) -> Line<'a> {
    let prefix = if focused { "▸ " } else { "  " };
    let label_style = if focused { theme.accent } else { theme.muted };
    let value_style = if focused { theme.highlight } else { theme.base };

    Line::from(vec![
        Span::styled(format!("{prefix}{label}: "), label_style),
        Span::styled(format!(" {value} "), value_style),
    ])
}
