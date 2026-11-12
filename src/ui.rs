use crate::app::{ActiveMode, App, WorkspaceSection};
use crate::domain::model::{HarnessKind, ProjectPolicy};
use crate::mascot::{
    build_ascii_splash_lines, render_fullscreen_background_canvas_with_state, sparkle_glyph,
};
use crate::screens::conflict::draw_conflict_modal;
use crate::screens::form::{centered_rect, draw_form_modal};
use crate::screens::options::draw_options_modal;
use crate::screens::workspace::draw_workspace;
use crate::theme::{
    HEADER_HEIGHT, MIN_HEIGHT, MIN_WIDTH, PADDING_SM, PADDING_XS, STATUS_BAR_HEIGHT, Theme,
};
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};

/// Root render function (pure function of `App` state + `Theme`).
pub fn draw(f: &mut Frame, app: &App, base_theme: &Theme) {
    let (preview_theme_name, animations_enabled, hide_ui) = match &app.mode {
        ActiveMode::Options(opt) => (opt.theme, opt.animations, opt.hide_ui),
        _ => (
            app.loaded.config.ui.theme,
            app.animations_enabled(),
            app.loaded.config.ui.hide_ui,
        ),
    };
    let active_theme = Theme::with_theme(preview_theme_name, base_theme.mono);
    let theme = &active_theme;
    let area = f.area();

    let fx_areas = app.fx.borrow().areas.clone();
    fx_areas.begin_frame(area);

    // Minimum-size gate (`50x16`)
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!(
            "! Terminal too small ({}x{}).\nMinimum required: {}x{}.",
            area.width, area.height, MIN_WIDTH, MIN_HEIGHT
        );
        let p = Paragraph::new(msg)
            .style(theme.warning)
            .alignment(Alignment::Center);
        f.render_widget(p, area);
        return;
    }

    if hide_ui {
        fx_areas.workspace.set(area);
        fx_areas.companion_portrait.set(area);
        render_fullscreen_background_canvas_with_state(
            f.buffer_mut(),
            area,
            app.anim_elapsed,
            app.mascot_anim_state(),
            animations_enabled,
            theme.mono,
            theme,
        );
        let hint_area = Rect {
            x: area.x,
            y: area.y.saturating_add(area.height.saturating_sub(1)),
            width: area.width,
            height: 1,
        };
        let sp = sparkle_glyph(app.anim_tick, animations_enabled, theme.mono);
        let hint = Line::from(vec![
            Span::styled(format!(" {sp} maga · background showcase "), theme.accent),
            Span::styled(
                " · b/Esc: show UI  o: options  ?: help  q: quit ",
                theme.muted,
            ),
        ]);
        f.render_widget(Paragraph::new(hint).alignment(Alignment::Center), hint_area);
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(HEADER_HEIGHT),
                Constraint::Min(0),
                Constraint::Length(STATUS_BAR_HEIGHT),
            ])
            .split(area);

        fx_areas
            .header_badge
            .set(Rect::new(rows[0].x, rows[0].y, rows[0].width.min(9), 1));
        fx_areas.workspace.set(rows[1]);
        fx_areas.status_bar.set(rows[2]);

        draw_header(f, rows[0], app, theme);
        draw_workspace(f, rows[1], app, theme);
        draw_status_bar(f, rows[2], app, theme);
    }

    match &app.mode {
        ActiveMode::Normal | ActiveMode::Filtering => {}
        ActiveMode::Form(form) => {
            fx_areas.active_modal.set(centered_rect(72, 20, area));
            draw_form_modal(f, area, form, theme);
        }
        ActiveMode::Options(opt) => {
            fx_areas.active_modal.set(centered_rect(72, 23, area));
            draw_options_modal(f, area, opt, theme);
        }
        ActiveMode::ConflictModal { index } => {
            fx_areas.active_modal.set(centered_rect(
                area.width.saturating_sub(4).min(96),
                20,
                area,
            ));
            draw_conflict_modal(f, area, &app.outcome.conflicts, *index, theme);
        }
        ActiveMode::ConfirmDelete { name } => {
            fx_areas.active_modal.set(centered_rect(64, 8, area));
            let targets = app.enabled_harnesses_label();
            draw_confirm_delete_modal(f, area, name, &targets, theme);
        }
        ActiveMode::ConfirmDeletePlugin {
            harness,
            display_name,
            install_path,
            ..
        } => {
            let modal_h = if install_path.is_some() { 10 } else { 8 };
            fx_areas.active_modal.set(centered_rect(68, modal_h, area));
            draw_confirm_delete_plugin_modal(
                f,
                area,
                display_name,
                harness.display_name(),
                install_path.as_deref(),
                theme,
            );
        }
        ActiveMode::ProfilesModal { selected, items } => {
            let modal_h = (items.len() + 8).clamp(10, 22) as u16;
            fx_areas
                .active_modal
                .set(centered_rect(58, modal_h, area));
            draw_profiles_modal(f, area, app, *selected, items, theme);
        }
        ActiveMode::NewProfileModal { input } => {
            fx_areas
                .active_modal
                .set(centered_rect(52, 8, area));
            draw_new_profile_modal(f, area, input, theme);
        }
        ActiveMode::MatrixModal { server_name, selected } => {
            let project_count = app.loaded.claude.project_paths().len();
            let modal_h = (9 + HarnessKind::ALL.len() + if project_count > 0 { 1 + project_count } else { 0 }).clamp(14, 26) as u16;
            fx_areas
                .active_modal
                .set(centered_rect(68, modal_h, area));
            draw_matrix_modal(f, area, app, server_name, *selected, theme);
        }
        ActiveMode::Help => {
            let (help_w, help_h) = help_modal_dimensions(area);
            fx_areas
                .active_modal
                .set(centered_rect(help_w, help_h, area));
            draw_help_modal(f, area, app, theme);
        }
    }

    let triggers = app.take_pending_fx();
    let mut fx = app.fx.borrow_mut();
    if !triggers.is_empty() {
        fx.apply_triggers(&triggers, theme, animations_enabled);
    }
    fx.process(
        app.last_frame_delta,
        f.buffer_mut(),
        theme,
        animations_enabled,
    );
}

fn draw_header(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let total = app.outcome.canonical.len();
    let active = app.outcome.canonical.values().filter(|s| s.enabled).count();
    let (active_plugins, total_plugins) = app.total_plugins_counts();
    let conflicts = app.outcome.conflicts.len();
    let sp = sparkle_glyph(app.anim_tick, app.animations_enabled(), theme.mono);

    let (sync_badge, sync_style) = if conflicts > 0 {
        (format!("! {conflicts} conflict(s)"), theme.warning)
    } else {
        let label = app.enabled_harnesses_label();
        (format!("✓ In Sync ({label})"), theme.success)
    };

    let (refresh_badge, refresh_style) = if app.auto_refresh_enabled() {
        (
            format!("  ↻ auto: {}s", app.loaded.config.refresh.interval_secs),
            theme.accent,
        )
    } else {
        ("  ↻ auto: off".to_string(), theme.muted)
    };

    let active_guidelines = app
        .guidelines_statuses
        .iter()
        .filter(|s| s.exists && s.matches_canonical)
        .count();
    let total_guidelines = app.guidelines_statuses.len();

    let total_refs = app.loaded.refs.refs.len();
    let cache_dir = app
        .paths
        .as_ref()
        .map(|p| p.refs_cache_dir.clone())
        .unwrap_or_else(|| std::path::PathBuf::from("refs"));
    let active_refs = app
        .loaded
        .refs
        .refs
        .iter()
        .filter(|(alias, entry)| {
            let path = app.loaded.refs.effective_path(alias, entry, &cache_dir);
            path.exists() && path.join(".git").exists()
        })
        .count();

    let (mcp_tab_style, plugin_tab_style, guidelines_tab_style, refs_tab_style, section_active, section_total) =
        match app.section {
            WorkspaceSection::Mcps => (
                theme.highlight,
                theme.muted,
                theme.muted,
                theme.muted,
                active,
                total,
            ),
            WorkspaceSection::Plugins => (
                theme.muted,
                theme.highlight,
                theme.muted,
                theme.muted,
                active_plugins,
                total_plugins,
            ),
            WorkspaceSection::Guidelines => (
                theme.muted,
                theme.muted,
                theme.highlight,
                theme.muted,
                active_guidelines,
                total_guidelines,
            ),
            WorkspaceSection::Refs => (
                theme.muted,
                theme.muted,
                theme.muted,
                theme.highlight,
                active_refs,
                total_refs,
            ),
        };

    let tab3_span = if area.width >= 105 {
        Some(Span::styled(
            format!("[3: Guidelines ({active_guidelines}/{total_guidelines})]"),
            guidelines_tab_style,
        ))
    } else if area.width >= 88 {
        Some(Span::styled("[3: Rules]".to_string(), guidelines_tab_style))
    } else if app.section == WorkspaceSection::Guidelines {
        Some(Span::styled("[3: Guidelines]".to_string(), guidelines_tab_style))
    } else {
        None
    };

    let tab4_span = if area.width >= 115 {
        Some(Span::styled(
            format!("[4: Refs ({active_refs}/{total_refs})]"),
            refs_tab_style,
        ))
    } else if area.width >= 98 {
        Some(Span::styled("[4: Refs]".to_string(), refs_tab_style))
    } else if app.section == WorkspaceSection::Refs {
        Some(Span::styled("[4: Refs]".to_string(), refs_tab_style))
    } else {
        None
    };

    let mut line1_spans = vec![
        Span::styled(format!(" {sp} maga "), theme.highlight),
        Span::styled(" · ", theme.header),
        Span::styled(format!("[1: MCPs ({total})]"), mcp_tab_style),
        Span::styled("  ", theme.base),
        Span::styled(format!("[2: Plugins ({total_plugins})]"), plugin_tab_style),
    ];
    if let Some(s) = tab3_span {
        line1_spans.push(Span::styled("  ", theme.base));
        line1_spans.push(s);
    }
    if let Some(s) = tab4_span {
        line1_spans.push(Span::styled("  ", theme.base));
        line1_spans.push(s);
    }
    line1_spans.push(Span::styled("  ", theme.base));
    line1_spans.push(Span::styled(sync_badge, sync_style));
    line1_spans.push(Span::styled(format!("  [{section_active}/{section_total}]"), theme.muted));
    if let Some(ref prof) = app.loaded.config.active_profile {
        line1_spans.push(Span::styled("  ", theme.base));
        line1_spans.push(Span::styled(format!("[Profile: {prof}]"), theme.accent));
    }
    let line1 = Line::from(line1_spans);
    let mut line2_spans = vec![Span::styled(refresh_badge, refresh_style)];
    if let Some((ref msg, is_err)) = app.status_message {
        let msg_style = if is_err { theme.error } else { theme.accent };
        line2_spans.push(Span::styled("  ·  ", theme.muted));
        line2_spans.push(Span::styled(msg.clone(), msg_style));
    }
    let line2 = Line::from(line2_spans);

    let p = Paragraph::new(vec![line1, line2]);
    f.render_widget(p, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let hints = match app.mode {
        ActiveMode::Filtering => match app.section {
            WorkspaceSection::Mcps => {
                " Filter MCPs (name, desc, enabled/disabled/conflict, stdio/http, cmd/url, is:/type:/cmd:/desc:) · Enter: keep · Esc: clear"
            }
            WorkspaceSection::Plugins => {
                " Filter Plugins (name, desc, harness, skill/hook/mcp, is:/harness:/type:/desc:/skill:/hook:/mcp:) · Enter: keep · Esc: clear"
            }
            WorkspaceSection::Guidelines | WorkspaceSection::Refs => {
                " Guidelines View · Esc: back to normal"
            }
        },
        ActiveMode::Form(_) => {
            " Tab/Shift+Tab: field · Space: switch transport · Enter: save · Esc: cancel"
        }
        ActiveMode::Options(_) => {
            " j/k/Tab: field · Space/←/→: change value · Enter: save to config.toml · Esc: cancel"
        }
        ActiveMode::ConflictModal { .. } => {
            " 1..N: Keep Variant · E: Merge in $EDITOR · s/Esc: Skip"
        }
        ActiveMode::ConfirmDelete { .. } | ActiveMode::ConfirmDeletePlugin { .. } => {
            " y/Enter: Confirm delete · n/Esc: Cancel"
        }
        ActiveMode::ProfilesModal { .. } => {
            " ↑/↓/j/k: Select · Enter: Apply · a: New · e: $EDITOR · d: Delete · Esc: Cancel"
        }
        ActiveMode::NewProfileModal { .. } => " Enter: Create & switch profile · Esc: Cancel",
        ActiveMode::MatrixModal { .. } => " ↑/↓/j/k: Navigate · Space: Toggle override · Enter/Esc: Close & Save",
        ActiveMode::Help => " j/k/↑/↓/PgUp/PgDn/g/G: Scroll shortcuts · Esc/q/?: Close help",
        ActiveMode::Normal if app.details_focused => {
            " j/k/PgUp/PgDn/g/G: scroll DETAILS  p: re-probe  t/Esc: return to list  ?: help  q: quit"
        }
        ActiveMode::Normal => match app.section {
            WorkspaceSection::Mcps => {
                " Tab/1/2/3/4: view  j/k: nav  Space: toggle  w: profile  x: matrix  p/P: probe  t: tools  m: mascot  o: options  r/R: refresh  /: filter  a/e/E: edit  d: del  c: conflicts  ?: help  q: quit"
            }
            WorkspaceSection::Plugins => {
                " Tab/1/2/3/4: view  j/k: nav  h/l: fold  Space/Enter: toggle/fold  E: edit config  d: del  t: details  m: mascot  o: options  r/R: refresh  /: filter  ?: help  q: quit"
            }
            WorkspaceSection::Guidelines => {
                " Tab/1/2/3/4: view  j/k: nav  e/Enter: edit AGENTS.md  s: resync links  t: details  r/R: refresh  ?: help  q: quit"
            }
            WorkspaceSection::Refs => {
                " Tab/1/2/3/4: view  j/k: nav  Enter: sync ref  r/R: refresh  ?: help  q: quit"
            }
        },
    };

    let update_badge = app
        .available_update()
        .map(|ver| format!("✦ Update: {ver} (maga --update) "));

    let p = if let Some(badge) = update_badge {
        let badge_len = badge.chars().count();
        if (area.width as usize) > badge_len + 35 {
            let left_width = (area.width as usize).saturating_sub(badge_len);
            let truncated_hints: String = hints.chars().take(left_width).collect();
            let pad_len = left_width.saturating_sub(truncated_hints.chars().count());
            let padding = " ".repeat(pad_len);
            Paragraph::new(Line::from(vec![
                Span::styled(truncated_hints, theme.status_bar),
                Span::raw(padding),
                Span::styled(badge, theme.warning),
            ]))
        } else {
            Paragraph::new(Span::styled(hints, theme.status_bar))
        }
    } else {
        Paragraph::new(Span::styled(hints, theme.status_bar))
    };
    f.render_widget(p, area);
}

fn draw_confirm_delete_modal(f: &mut Frame, area: Rect, name: &str, targets: &str, theme: &Theme) {
    let modal = centered_rect(64, 8, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .title(Span::styled(" ! CONFIRM DELETE ", theme.warning))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.warning)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let text = vec![
        Line::from(vec![
            Span::styled("Remove ", theme.base),
            Span::styled(format!("'{name}'"), theme.bold),
            Span::styled(format!(" from maga and {targets}?"), theme.base),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[y / Enter]", theme.error),
            Span::styled(" Yes, delete    ", theme.base),
            Span::styled("[n / Esc]", theme.accent),
            Span::styled(" Cancel", theme.base),
        ]),
    ];

    let p = Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, modal);
}

fn draw_confirm_delete_plugin_modal(
    f: &mut Frame,
    area: Rect,
    display_name: &str,
    harness_name: &str,
    install_path: Option<&std::path::Path>,
    theme: &Theme,
) {
    let modal_h = if install_path.is_some() { 10 } else { 8 };
    let modal = centered_rect(68, modal_h, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .title(Span::styled(" ! CONFIRM REMOVE PLUGIN ", theme.warning))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.warning)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let mut text = vec![Line::from(vec![
        Span::styled("Remove ", theme.base),
        Span::styled(format!("'{display_name}'"), theme.bold),
        Span::styled(
            format!(" from {harness_name} and maga plugins.toml?"),
            theme.base,
        ),
    ])];
    if let Some(dir) = install_path {
        text.push(Line::from(vec![
            Span::styled("Deletes dir: ", theme.muted),
            Span::styled(dir.display().to_string(), theme.warning),
        ]));
    }
    text.push(Line::from(""));
    text.push(Line::from(vec![
        Span::styled("[y / Enter]", theme.error),
        Span::styled(" Yes, remove    ", theme.base),
        Span::styled("[n / Esc]", theme.accent),
        Span::styled(" Cancel", theme.base),
    ]));

    let p = Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, modal);
}

fn draw_profiles_modal(
    f: &mut Frame,
    area: Rect,
    app: &App,
    selected: usize,
    items: &[String],
    theme: &Theme,
) {
    let modal_h = (items.len() + 8).clamp(10, 22) as u16;
    let modal = centered_rect(58, modal_h, area);
    f.render_widget(Clear, modal);

    let active_prof = app.loaded.config.active_profile.as_deref().unwrap_or("");
    let block = Block::default()
        .title(Span::styled(" ✦ SWITCH MCP PROFILE ✦ ", theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.accent)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Select active profile overlay (Enter: apply, a: new, e: edit, d: del):",
        theme.muted,
    )));
    lines.push(Line::from(""));

    for (idx, item) in items.iter().enumerate() {
        let is_selected = idx == selected;
        let is_active = (idx == 0 && active_prof.is_empty()) || item == active_prof;
        let prefix = if is_selected { "▶ " } else { "  " };
        let active_badge = if is_active { " [active]" } else { "" };
        let style = if is_selected {
            theme.highlight
        } else if is_active {
            theme.accent
        } else {
            theme.base
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, theme.accent),
            Span::styled(item.clone(), style),
            Span::styled(active_badge, theme.success),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[Enter: Apply] ", theme.accent),
        Span::styled("[a: New] ", theme.base),
        Span::styled("[e: $EDITOR] ", theme.base),
        Span::styled("[d: Delete] ", theme.error),
        Span::styled("[Esc: Cancel]", theme.muted),
    ]));

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, modal);
}

fn draw_matrix_modal(
    f: &mut Frame,
    area: Rect,
    app: &App,
    server_name: &str,
    selected: usize,
    theme: &Theme,
) {
    let projects = app.loaded.claude.project_paths();
    let modal_h = (9 + HarnessKind::ALL.len() + if projects.is_empty() { 0 } else { 1 + projects.len() }).clamp(14, 26) as u16;
    let modal = centered_rect(68, modal_h, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .title(Span::styled(format!(" ✦ HARNESS MATRIX: {server_name} ✦ "), theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.accent)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let server_cfg = app.outcome.canonical.get(server_name).cloned().unwrap_or_else(|| {
        crate::domain::model::McpServerConfig::new_http("http://localhost")
    });
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Global Base: ", theme.muted),
        Span::styled(
            if server_cfg.enabled { "Enabled" } else { "Disabled" },
            if server_cfg.enabled { theme.success } else { theme.error },
        ),
        Span::styled("  ·  Space: toggle  ·  Enter/Esc: done", theme.muted),
    ]));
    lines.push(Line::from(""));

    // Row 0: Project Policy (Claude Code)
    let is_row_0 = selected == 0;
    let r0_prefix = if is_row_0 { "▶ " } else { "  " };
    let policy_str = match server_cfg.project_policy {
        ProjectPolicy::Mirror => "Mirror (syncs all projects)",
        ProjectPolicy::Preserve => "Preserve (keeps project custom overrides)",
    };
    lines.push(Line::from(vec![
        Span::styled(r0_prefix, theme.accent),
        Span::styled("Claude Project Policy: ", if is_row_0 { theme.highlight } else { theme.base }),
        Span::styled(policy_str, theme.accent),
    ]));

    // Rows 1..=5: Harness overrides
    for (i, kind) in HarnessKind::ALL.iter().enumerate() {
        let row_idx = i + 1;
        let is_sel = selected == row_idx;
        let prefix = if is_sel { "▶ " } else { "  " };
        let override_val = server_cfg.harnesses.get(kind);
        let status_str = match override_val {
            None => format!("Inherit (Global: {})", if server_cfg.enabled { "Enabled" } else { "Disabled" }),
            Some(true) => "Explicit: Enabled".to_string(),
            Some(false) => "Explicit: Disabled".to_string(),
        };
        let status_style = match override_val {
            None => theme.muted,
            Some(true) => theme.success,
            Some(false) => theme.error,
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, theme.accent),
            Span::styled(format!("{:<15}: ", kind.display_name()), if is_sel { theme.highlight } else { theme.base }),
            Span::styled(status_str, status_style),
        ]));
    }

    // Claude Local Projects (if any)
    if !projects.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "── CLAUDE LOCAL PROJECTS (Per-Project Overrides) ──",
            theme.header,
        )));
        for (i, proj_path) in projects.iter().enumerate() {
            let row_idx = 1 + HarnessKind::ALL.len() + i;
            let is_sel = selected == row_idx;
            let prefix = if is_sel { "▶ " } else { "  " };
            let state = app.loaded.claude.project_server_state(proj_path, server_name);
            let (status_str, status_style) = match state {
                None => (
                    format!("Inherit ({})", if server_cfg.enabled { "Global On" } else { "Global Off" }),
                    theme.muted,
                ),
                Some(true) => ("[Project: Enabled]".to_string(), theme.success),
                Some(false) => ("[Project: Disabled]".to_string(), theme.error),
            };

            let display_path = if proj_path.len() > 30 {
                let tail: String = proj_path.chars().rev().take(27).collect();
                format!("...{}", tail.chars().rev().collect::<String>())
            } else {
                proj_path.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, theme.accent),
                Span::styled(format!("{:<30}: ", display_path), if is_sel { theme.highlight } else { theme.base }),
                Span::styled(status_str, status_style),
            ]));
        }
    }

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, modal);
}

fn draw_new_profile_modal(
    f: &mut Frame,
    area: Rect,
    input: &str,
    theme: &Theme,
) {
    let modal = centered_rect(52, 8, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .title(Span::styled(" ✦ CREATE NEW MCP PROFILE ✦ ", theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.accent)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let lines = vec![
        Line::from(Span::styled("Profile name (e.g. work, personal, dev):", theme.muted)),
        Line::from(""),
        Line::from(vec![
            Span::styled("▶ ", theme.accent),
            Span::styled(input, theme.highlight),
            Span::styled("█", theme.accent),
        ]),
        Line::from(""),
        Line::from(Span::styled("[Enter: Create & Activate]  [Esc: Cancel]", theme.muted)),
    ];

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, modal);
}

fn help_modal_dimensions(area: Rect) -> (u16, u16) {
    let width = area.width.saturating_sub(4).clamp(68, 96);
    let height = area.height.saturating_sub(2).clamp(16, 36);
    (width, height)
}

fn draw_help_modal(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let (modal_w, modal_h) = help_modal_dimensions(area);
    let modal = centered_rect(modal_w, modal_h, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .title(Span::styled(
            " KEYBOARD SHORTCUTS (j/k/↑/↓: scroll · Esc/q/?: close) ",
            theme.header,
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.active_border)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(modal);

    let mut lines = build_ascii_splash_lines(theme, app.anim_tick, app.animations_enabled());
    lines.push(Line::from(""));

    let shortcut_lines = vec![
        Line::from(Span::styled(
            "── WORKSPACE & NAVIGATION (All Views) ──────────────────────────",
            theme.header,
        )),
        Line::from(vec![
            Span::styled("Tab / 1 / 2     ", theme.accent),
            Span::styled(
                "Switch workspace view between 1: MCPs and 2: Plugins",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("j / k / ↑ / ↓   ", theme.accent),
            Span::styled(
                "Navigate list rows (or scroll DETAILS inspector / Help modal)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("PgUp / PgDn     ", theme.accent),
            Span::styled("Jump 10 rows up / down (or Ctrl+u / Ctrl+d)", theme.base),
        ]),
        Line::from(vec![
            Span::styled("g / G / Home/End", theme.accent),
            Span::styled(
                "Jump to first (g) or last (G) row in active list",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("t               ", theme.accent),
            Span::styled(
                "Focus DETAILS inspector to scroll long tool/plugin output (Esc to return)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("/               ", theme.accent),
            Span::styled(
                "Live filter active list (Enter to keep filter, Esc to clear)",
                theme.base,
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "── 1: MCP SERVERS VIEW ([1: MCPs]) ─────────────────────────────",
            theme.header,
        )),
        Line::from(vec![
            Span::styled("Space           ", theme.accent),
            Span::styled(
                "Toggle selected MCP server ON/OFF across all enabled harnesses",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("a / e           ", theme.accent),
            Span::styled(
                "Add new (a) or edit selected (e) MCP server via guided form",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("E               ", theme.accent),
            Span::styled(
                "Open selected MCP server in $EDITOR as raw TOML",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("d               ", theme.accent),
            Span::styled(
                "Delete selected MCP server from maga and all harness configs",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("p / P           ", theme.accent),
            Span::styled(
                "Probe selected server (p) or all enabled servers (P) (initialize + tools/list)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("c               ", theme.accent),
            Span::styled("Open side-by-side Conflict Resolution modal", theme.base),
        ]),
        Line::from(vec![
            Span::styled("MCP Filters     ", theme.muted),
            Span::styled(
                "is:enabled|disabled|conflict  type:stdio|http  cmd:<str>  desc:<str>",
                theme.muted,
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "── 2: HARNESS PLUGINS VIEW ([2: Plugins]) ──────────────────────",
            theme.header,
        )),
        Line::from(vec![
            Span::styled("Space           ", theme.accent),
            Span::styled(
                "Toggle selected plugin ON/OFF in its harness (or fold/unfold group header)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("Enter           ", theme.accent),
            Span::styled(
                "Fold / unfold selected harness group header (▾ / ▸)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("h / ←           ", theme.accent),
            Span::styled(
                "Collapse current harness group (jumps from child plugin to group header)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("l / →           ", theme.accent),
            Span::styled("Expand collapsed harness group header", theme.base),
        ]),
        Line::from(vec![
            Span::styled("E               ", theme.accent),
            Span::styled(
                "Open plugin manifest (plugin.json, index.ts) or harness config in $EDITOR",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("d               ", theme.accent),
            Span::styled(
                "Uninstall selected plugin (config, registry, on-disk dir & plugins.toml)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("Plugin Filters  ", theme.muted),
            Span::styled(
                "is:on|off  harness:<name>  type:plugin|npm|local  desc:  skill:  hook:  mcp:",
                theme.muted,
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "── SYNC, OPTIONS & VISUALS (Global) ────────────────────────────",
            theme.header,
        )),
        Line::from(vec![
            Span::styled("o               ", theme.accent),
            Span::styled(
                "Open Options modal (theme, mascot, refresh, harnesses)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("w / x           ", theme.accent),
            Span::styled(
                "Open Profiles switcher (w) or Harness Matrix overrides (x)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("r / R           ", theme.accent),
            Span::styled(
                "Reload and reconcile configs from disk (r) or toggle auto-refresh (R)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("s               ", theme.accent),
            Span::styled(
                "Force push canonical MCP registry to all enabled harnesses now",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("m / b           ", theme.accent),
            Span::styled(
                "Toggle Maga Hero Showcase in right panel (m) or Background-Only mode (b)",
                theme.base,
            ),
        ]),
        Line::from(vec![
            Span::styled("? / Esc / q     ", theme.accent),
            Span::styled(
                "Toggle Help (?), close modal / clear filter (Esc), or quit maga (q / Ctrl+C)",
                theme.base,
            ),
        ]),
    ];
    lines.extend(shortcut_lines);

    let total_lines = lines.len();
    let viewport_height = inner.height.max(1) as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let scroll_y = app.help_scroll.min(max_scroll);

    let p = Paragraph::new(lines)
        .block(block)
        .scroll((scroll_y as u16, 0));
    f.render_widget(p, modal);

    if total_lines > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll_y);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme.accent);
        let scrollbar_area = Rect {
            x: modal.x,
            y: modal.y.saturating_add(1),
            width: modal.width,
            height: modal.height.saturating_sub(2),
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{MascotMode, McpServerConfig, ThemeName};
    use crate::domain::storage::LoadedConfigs;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn build_test_app(w: u16, h: u16) -> App {
        let mut loaded = LoadedConfigs::default();
        let mut c7 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        c7.description = Some("Library docs lookup".to_string());
        loaded.mcp.mcps.insert("context7".to_string(), c7);
        loaded.mcp.mcps.insert(
            "sqlite-mcp".to_string(),
            McpServerConfig::new_stdio("/usr/bin/sqlite-mcp", vec!["mcp".to_string()]),
        );
        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();
        App::new(None, loaded, w, h)
    }

    fn render_to_string(w: u16, h: u16, app: &App, mono: bool) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let theme = Theme::new(mono);
        terminal.draw(|f| draw(f, app, &theme)).expect("draw frame");
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_master_detail_at_80x24() {
        let mut app = build_test_app(80, 24);
        let frame_str = render_to_string(80, 24, &app, false);
        assert!(frame_str.contains("✓ In Sync (5 harnesses)"));
        assert!(frame_str.contains("SERVERS"));
        assert!(frame_str.contains("DETAILS · context7"));
        assert!(frame_str.contains("Description: Library docs lookup"));
        assert!(frame_str.contains("✓ Enabled (5 harnesses)"));
        assert!(!frame_str.contains("COMPANION · Maga"));
        assert!(frame_str.contains("[x] context7"));
        assert!(frame_str.contains("https://mcp.context7.com/mcp"));
        assert!(frame_str.contains("↻ auto: 5s"));

        app.loaded.config.ui.mascot_mode = MascotMode::Rich;
        let rich_frame = render_to_string(80, 24, &app, false);
        assert!(rich_frame.contains("COMPANION · Maga"));
        assert!(rich_frame.contains("Targets: 5 harnesses"));

        app.toggle_auto_refresh();
        let paused_frame = render_to_string(80, 24, &app, false);
        assert!(paused_frame.contains("↻ auto: off"));
    }

    #[test]
    fn hides_companion_panel_in_lite_mode_or_short_terminal() {
        let mut app = build_test_app(80, 24);
        app.loaded.config.ui.mascot_mode = MascotMode::Lite;
        let lite_frame = render_to_string(80, 24, &app, false);
        assert!(lite_frame.contains("DETAILS · context7"));
        assert!(!lite_frame.contains("COMPANION · Maga"));

        app.loaded.config.ui.mascot_mode = MascotMode::Rich;
        app.loaded.config.ui.theme = ThemeName::Blush;
        let short_frame = render_to_string(80, 20, &app, false);
        assert!(short_frame.contains("DETAILS · context7"));
        assert!(!short_frame.contains("COMPANION · Maga"));
    }

    #[test]
    fn renders_ascii_witch_splash_in_empty_state_and_help_modal() {
        let empty_app = App::new(None, LoadedConfigs::default(), 80, 24);
        let empty_frame = render_to_string(80, 24, &empty_app, false);
        assert!(empty_frame.contains("[●]━━[●]"));
        assert!(empty_frame.contains("No MCP servers configured"));

        let mut help_app = build_test_app(96, 28);
        help_app.mode = ActiveMode::Help;
        let help_frame = render_to_string(96, 28, &help_app, false);
        assert!(help_frame.contains("KEYBOARD SHORTCUTS"));
        assert!(help_frame.contains("[●]━━[●]"));
        assert!(help_frame.contains("WORKSPACE & NAVIGATION (All Views)"));
        assert!(help_frame.contains("1: MCP SERVERS VIEW ([1: MCPs])"));
        assert!(help_frame.contains("Probe selected server"));

        // Scroll down to bottom of Help modal (`G`) to see Plugins & Global sections
        help_app.help_scroll = usize::MAX / 2;
        let scrolled_help_frame = render_to_string(96, 28, &help_app, false);
        assert!(scrolled_help_frame.contains("2: HARNESS PLUGINS VIEW ([2: Plugins])"));
        assert!(scrolled_help_frame.contains("SYNC, OPTIONS & VISUALS (Global)"));
        assert!(scrolled_help_frame.contains("Open Options modal"));
        assert!(scrolled_help_frame.contains("Toggle Help (?), close modal / clear filter (Esc)"));
    }

    #[test]
    fn renders_live_probe_health_tools_and_scrollable_details() {
        use crate::domain::probe::{McpToolInfo, ProbeOutcome, ServerProbeState};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = build_test_app(110, 24);
        let tools: Vec<McpToolInfo> = (1..=25)
            .map(|i| McpToolInfo {
                name: format!("tool_{i:02}"),
                description: Some(format!("Description {i}")),
            })
            .collect();
        app.probe_states.insert(
            "context7".to_string(),
            ServerProbeState::Online(ProbeOutcome {
                latency_ms: 128,
                server_info: Some("context7 v1.4".to_string()),
                protocol_version: Some("2024-11-05".to_string()),
                tools,
            }),
        );

        let frame = render_to_string(110, 24, &app, false);
        assert!(frame.contains("✓ Online (128ms) · context7 v1.4"));
        assert!(frame.contains("Exposed Tools (25)"));
        assert!(frame.contains("tool_01"));
        assert!(!frame.contains("tool_25"));

        // Focus DETAILS with 't' and scroll to bottom with 'G'
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        let scrolled = render_to_string(110, 24, &app, false);
        assert!(scrolled.contains("DETAILS · context7 (j/k: scroll · t/Esc: servers)"));
        assert!(scrolled.contains("tool_25"));
    }

    #[test]
    fn renders_options_modal_when_opened() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = build_test_app(80, 24);
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        let frame_str = render_to_string(80, 24, &app, false);
        assert!(frame_str.contains("OPTIONS · config.toml"));
        assert!(frame_str.contains("blush (Warm Sunset)"));
        assert!(frame_str.contains("lite (ASCII Splash)"));
        assert!(frame_str.contains("Antigravity"));
        assert!(frame_str.contains("Claude Code"));
    }

    #[test]
    fn collapses_detail_panel_at_55x20() {
        let app = build_test_app(55, 20);
        let frame_str = render_to_string(55, 20, &app, true);
        assert!(frame_str.contains("SERVERS"));
        assert!(!frame_str.contains("DETAILS · context7"));
    }

    #[test]
    fn renders_minimum_size_gate_at_40x12() {
        let app = build_test_app(40, 12);
        let frame_str = render_to_string(40, 12, &app, true);
        assert!(frame_str.contains("Terminal too small (40x12)"));
    }

    #[test]
    fn renders_conflict_modal_when_conflicts_exist() {
        let mut loaded = LoadedConfigs::default();
        let sqlite_v1 = McpServerConfig::new_stdio("/bin/sqlite-mcp", vec!["mcp".to_string()]);
        loaded
            .agy
            .servers
            .insert("sqlite-mcp".to_string(), sqlite_v1.clone());
        loaded
            .copilot
            .servers
            .insert("sqlite-mcp".to_string(), sqlite_v1);

        let mut sqlite_v2 = McpServerConfig::new_stdio("/bin/sqlite-mcp", vec!["mcp".to_string()]);
        sqlite_v2
            .env
            .insert("APP_ENV".to_string(), "local".to_string());
        loaded
            .claude
            .servers
            .insert("sqlite-mcp".to_string(), sqlite_v2.clone());
        loaded
            .opencode
            .servers
            .insert("sqlite-mcp".to_string(), sqlite_v2);

        let app = App::new(None, loaded, 90, 24);
        let frame_str = render_to_string(90, 24, &app, false);
        assert!(frame_str.contains("CONFLICT (1/1)"));
        assert!(frame_str.contains("[1] Agy, Copilot (sqlite-mcp)"));
        assert!(frame_str.contains("[2] Claude, OpenCode (sqlite-mcp)"));
    }

    #[test]
    fn scrolls_server_list_viewport_past_21_items() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut loaded = LoadedConfigs::default();
        for i in 1..=28 {
            loaded.mcp.mcps.insert(
                format!("mcp-server-{i:02}"),
                McpServerConfig::new_http(format!("https://s{i}.example.com/mcp")),
            );
        }
        loaded.agy.servers = loaded.mcp.mcps.clone();
        loaded.claude.servers = loaded.mcp.mcps.clone();

        let mut app = App::new(None, loaded, 80, 24);

        // Initial render shows item 1 and position (1/28), not item 25
        let initial_frame = render_to_string(80, 24, &app, false);
        assert!(initial_frame.contains("SERVERS (1/28)"));
        assert!(initial_frame.contains("▸ [x] mcp-server-01"));
        assert!(!initial_frame.contains("mcp-server-25"));

        // Navigate down to item 25 (index 24)
        for _ in 0..24 {
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        }
        let scrolled_frame = render_to_string(80, 24, &app, false);
        assert!(scrolled_frame.contains("SERVERS (25/28)"));
        assert!(scrolled_frame.contains("▸ [x] mcp-server-25"));
        assert!(scrolled_frame.contains("DETAILS · mcp-server-25"));
        assert!(!scrolled_frame.contains("mcp-server-01"));
        // Verify vertical scrollbar thumb or track is rendered
        assert!(scrolled_frame.contains('┃') || scrolled_frame.contains('│'));
    }

    #[test]
    fn tachyonfx_pipeline_animates_color_frames_and_respects_mono_and_disabled_flags() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::time::Duration;

        let mut app = build_test_app(80, 24);
        app.loaded.config.ui.mascot_mode = MascotMode::Rich;
        let color_theme = Theme::new(false);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        // Frame 1: Static draw (delta = 0) populates FxAreas and registers Startup effect
        terminal
            .draw(|f| draw(f, &app, &color_theme))
            .expect("draw static");
        let static_buf = terminal.backend().buffer().clone();
        assert!(app.has_active_fx_transition());
        let portrait_rect = app.fx.borrow().areas.companion_portrait.get();
        assert_eq!(portrait_rect.width, 16);
        assert_eq!(portrait_rect.height, 8);
        assert!(app.fx.borrow().areas.selected_row.get().height == 1);

        // Verify Moe/Chibi Kaomoji witch is rendered inside the companion portrait
        let mut portrait_str = String::new();
        for y in portrait_rect.y..portrait_rect.y + portrait_rect.height {
            for x in portrait_rect.x..portrait_rect.x + portrait_rect.width {
                portrait_str.push_str(static_buf[(x, y)].symbol());
            }
        }
        assert!(portrait_str.contains("*=[o‿o]=*"));

        // Frame 2: Step 80ms into the startup materialization shader
        app.set_frame_delta(Duration::from_millis(80));
        terminal
            .draw(|f| draw(f, &app, &color_theme))
            .expect("draw animated");
        let mid_intro_buf = terminal.backend().buffer().clone();
        assert_ne!(mid_intro_buf, static_buf);

        // Toggle 'm' to summon the 54x22 Hero Mascot Showcase in the right panel
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(app.show_hero_mascot);
        app.set_frame_delta(Duration::from_millis(1500));
        terminal
            .draw(|f| draw(f, &app, &color_theme))
            .expect("draw hero settled");
        let settled_buf = terminal.backend().buffer().clone();
        let hero_rect = app.fx.borrow().areas.companion_portrait.get();
        assert!(hero_rect.width >= 30);
        assert!(hero_rect.height >= 14);

        app.set_frame_delta(Duration::from_millis(650));
        terminal
            .draw(|f| draw(f, &app, &color_theme))
            .expect("draw hero flow step");
        let flow_buf = terminal.backend().buffer().clone();
        assert_ne!(flow_buf, settled_buf);
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(!app.show_hero_mascot);

        // Toggle 'b' to enter Background-Only (hide_ui) mode and verify UI panels are hidden
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert!(app.loaded.config.ui.hide_ui);
        let bg_only_str = render_to_string(80, 24, &app, false);
        assert!(bg_only_str.contains("maga · background showcase"));
        assert!(!bg_only_str.contains("SERVERS ("));
        // Esc exits Background-Only mode and restores the UI
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.loaded.config.ui.hide_ui);

        // Trigger modal open ('o')
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert!(app.has_active_fx_transition());
        app.set_frame_delta(Duration::from_millis(50));
        terminal
            .draw(|f| draw(f, &app, &color_theme))
            .expect("draw modal intro");
        assert!(app.fx.borrow().areas.active_modal.get().width > 0);

        // Monochrome mode must never emit RGB colors even when frame delta > 0
        let mono_theme = Theme::new(true);
        let mut mono_terminal = Terminal::new(TestBackend::new(80, 24)).expect("mono terminal");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.set_frame_delta(Duration::from_millis(60));
        mono_terminal
            .draw(|f| draw(f, &app, &mono_theme))
            .expect("draw mono");
        let mono_buf = mono_terminal.backend().buffer();
        for y in 0..24 {
            for x in 0..80 {
                let cell = &mono_buf[(x, y)];
                assert_eq!(cell.fg, Color::Reset);
                assert_eq!(cell.bg, Color::Reset);
            }
        }
    }

    #[test]
    fn renders_plugins_grouped_accordion_view_and_inspector() {
        use crate::domain::model::HarnessKind;
        use crate::domain::plugins::{BundledCounts, HarnessPlugin, PluginKind};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::path::PathBuf;

        let mut app = build_test_app(96, 24);
        app.loaded.harness_plugins.insert(
            HarnessKind::Antigravity,
            vec![HarnessPlugin {
                harness: HarnessKind::Antigravity,
                id: "ponytail".to_string(),
                display_name: "ponytail".to_string(),
                description: Some("Workflow automation bundle".to_string()),
                enabled: true,
                kind: PluginKind::Bundle,
                source: Some("local".to_string()),
                version: Some("1.0.0".to_string()),
                install_path: Some(PathBuf::from("/tmp/plugins/ponytail")),
                config_path: PathBuf::from("/tmp/config.json"),
                bundled: BundledCounts {
                    skills: 2,
                    hooks: 1,
                    mcps: 1,
                    skill_names: vec!["review".to_string(), "triage".to_string()],
                    hook_names: vec!["PreToolUse".to_string()],
                    mcp_names: vec!["db".to_string()],
                },
            }],
        );
        app.loaded.harness_plugins.insert(
            HarnessKind::OpenCode,
            vec![HarnessPlugin {
                harness: HarnessKind::OpenCode,
                id: "opencode-wakatime".to_string(),
                display_name: "opencode-wakatime".to_string(),
                description: None,
                enabled: false,
                kind: PluginKind::Npm,
                source: Some("plugins.toml (disabled)".to_string()),
                version: None,
                install_path: None,
                config_path: PathBuf::from("/tmp/opencode.json"),
                bundled: BundledCounts::default(),
            }],
        );

        // In MCPs view, top header indicator shows [2/2] (enabled/total MCP servers)
        let mcp_frame = render_to_string(96, 24, &app, false);
        assert!(mcp_frame.contains("[1: MCPs (2)]"));
        assert!(mcp_frame.contains("[2: Plugins (2)]"));
        assert!(mcp_frame.contains("[2/2]"));
        assert!(!mcp_frame.contains("[1/2]"));

        // Switch to Plugins view with '2'; top header indicator updates to [1/2] (enabled/total plugins)
        app.loaded.config.ui.mascot_mode = MascotMode::Rich;
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        let group_frame = render_to_string(96, 24, &app, false);
        assert!(group_frame.contains("[1: MCPs (2)]"));
        assert!(group_frame.contains("[2: Plugins (2)]"));
        assert!(group_frame.contains("[1/2]"));
        assert!(!group_frame.contains("[2/2]"));
        assert!(group_frame.contains("✓ 1/2 harness plugins active!"));
        assert!(group_frame.contains("PLUGINS (1/2)"));
        app.loaded.config.ui.mascot_mode = MascotMode::Lite;
        assert!(group_frame.contains("▾ Antigravity (1/1)"));
        assert!(group_frame.contains("[x] ponytail"));
        assert!(group_frame.contains("▾ OpenCode (0/1)"));
        assert!(group_frame.contains("[ ] opencode-wakatime"));
        assert!(group_frame.contains("DETAILS · Antigravity"));

        // Move down to "ponytail" row and verify plugin DETAILS inspector
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let plugin_frame = render_to_string(96, 24, &app, false);
        assert!(plugin_frame.contains("DETAILS · ponytail"));
        assert!(plugin_frame.contains("Workflow automation bundle"));
        assert!(plugin_frame.contains("✓ Enabled in Antigravity"));
        assert!(plugin_frame.contains("plugin · local"));
        assert!(plugin_frame.contains("Bundled Contents"));
        assert!(plugin_frame.contains("• Skills : 2 (review, triage)"));
        assert!(plugin_frame.contains("• Hooks  : 1 (PreToolUse)"));
        assert!(plugin_frame.contains("• MCPs   : 1 (db)"));

        // Press 'd' on directory-backed "ponytail" row and verify modal displays install_path
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        let modal_frame = render_to_string(96, 24, &app, false);
        assert!(modal_frame.contains("CONFIRM REMOVE PLUGIN"));
        assert!(modal_frame.contains("Deletes dir: /tmp/plugins/ponytail"));
    }

    #[test]
    fn renders_profiles_modal_and_matrix_modal() {
        let mut app = App::new(None, crate::domain::storage::LoadedConfigs::default(), 96, 24);
        app.mode = ActiveMode::ProfilesModal {
            selected: 0,
            items: vec!["Default (All Servers)".to_string(), "work".to_string()],
        };
        let p_frame = render_to_string(96, 24, &app, false);
        assert!(p_frame.contains("SWITCH MCP PROFILE"));
        assert!(p_frame.contains("Default (All Servers)"));
        assert!(p_frame.contains("work"));
        assert!(p_frame.contains("[a: New]"));

        // Render NewProfileModal
        app.mode = ActiveMode::NewProfileModal {
            input: "client-a".to_string(),
        };
        let np_frame = render_to_string(96, 24, &app, false);
        assert!(np_frame.contains("CREATE NEW MCP PROFILE"));
        assert!(np_frame.contains("client-a"));

        // Matrix modal with local projects
        if let serde_json::Value::Object(map) = serde_json::json!({
            "projects": {
                "/home/user/code/maga": {
                    "enabledMcpServers": ["context7"]
                }
            }
        }) {
            app.loaded.claude.raw_root = map;
        }
        app.outcome.canonical.insert(
            "context7".to_string(),
            crate::domain::model::McpServerConfig::new_http("https://mcp.context7.com/mcp"),
        );
        app.mode = ActiveMode::MatrixModal {
            server_name: "context7".to_string(),
            selected: 0,
        };
        let m_frame = render_to_string(96, 24, &app, false);
        assert!(m_frame.contains("HARNESS MATRIX: context7"));
        assert!(m_frame.contains("Claude Project Policy"));
        assert!(m_frame.contains("Claude Code"));
        assert!(m_frame.contains("CLAUDE LOCAL PROJECTS"));
        assert!(m_frame.contains("/home/user/code/maga"));
        assert!(m_frame.contains("[Project: Enabled]"));
    }
}
