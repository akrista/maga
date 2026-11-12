use crate::app::{ActiveMode, App, PluginRow, WorkspaceSection};
use crate::domain::model::{HarnessKind, MascotMode, McpServerConfig, Transport};
use crate::domain::plugins::HarnessPlugin;
use crate::domain::probe::ServerProbeState;
use crate::mascot::{
    build_ascii_splash_lines, build_companion_dialogue, companion_mood_for_app,
    render_background_watermark_with_state, render_chibi_companion_with_state,
    render_hero_mascot_lines_with_state, sparkle_glyph,
};
use crate::theme::{COMPANION_MIN_HEIGHT, PADDING_SM, PADDING_XS, SPLIT_BREAKPOINT_WIDTH, Theme};
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use unicode_width::UnicodeWidthStr;

/// Renders the Master-Detail workspace (or single-column list when `< 60` cols).
pub fn draw_workspace(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let fx_areas = app.fx.borrow().areas.clone();
    fx_areas.workspace.set(area);

    let (mascot_mode, background_enabled, hero_mascot_enabled) = match &app.mode {
        ActiveMode::Options(opt) => (opt.mascot_mode, opt.background, opt.hero_mascot),
        _ => (
            app.loaded.config.ui.mascot_mode,
            app.loaded.config.ui.background,
            app.loaded.config.ui.hero_mascot,
        ),
    };

    if area.width < SPLIT_BREAKPOINT_WIDTH {
        fx_areas.server_list.set(area);
        match app.section {
            WorkspaceSection::Mcps => draw_server_list(f, area, app, theme, &fx_areas),
            WorkspaceSection::Plugins => draw_plugin_list(f, area, app, theme, &fx_areas),
            WorkspaceSection::Guidelines => draw_guidelines_list(f, area, app, theme, &fx_areas),
            WorkspaceSection::Refs => draw_refs_list(f, area, app, theme, &fx_areas),
        }
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(area);

    fx_areas.server_list.set(columns[0]);
    match app.section {
        WorkspaceSection::Mcps => draw_server_list(f, columns[0], app, theme, &fx_areas),
        WorkspaceSection::Plugins => draw_plugin_list(f, columns[0], app, theme, &fx_areas),
        WorkspaceSection::Guidelines => draw_guidelines_list(f, columns[0], app, theme, &fx_areas),
        WorkspaceSection::Refs => draw_refs_list(f, columns[0], app, theme, &fx_areas),
    }

    let show_hero_showcase = hero_mascot_enabled
        && (app.show_hero_mascot
            || (app.section == WorkspaceSection::Mcps && app.outcome.canonical.is_empty()));
    let show_rich_companion =
        mascot_mode == MascotMode::Rich && f.area().height >= COMPANION_MIN_HEIGHT;

    if show_hero_showcase {
        fx_areas.detail_panel.set(columns[1]);
        draw_hero_showcase_panel(f, columns[1], app, theme, &fx_areas);
    } else if show_rich_companion {
        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(10)])
            .split(columns[1]);
        fx_areas.detail_panel.set(right_rows[0]);
        match app.section {
            WorkspaceSection::Mcps => draw_detail_panel(f, right_rows[0], app, theme),
            WorkspaceSection::Plugins => draw_plugin_detail_panel(f, right_rows[0], app, theme),
            WorkspaceSection::Guidelines => {
                draw_guidelines_detail_panel(f, right_rows[0], app, theme)
            }
            WorkspaceSection::Refs => {
                draw_refs_detail_panel(f, right_rows[0], app, theme)
            }
        }
        draw_companion_panel(f, right_rows[1], app, theme, &fx_areas);
    } else {
        fx_areas.detail_panel.set(columns[1]);
        match app.section {
            WorkspaceSection::Mcps => draw_detail_panel(f, columns[1], app, theme),
            WorkspaceSection::Plugins => draw_plugin_detail_panel(f, columns[1], app, theme),
            WorkspaceSection::Guidelines => {
                draw_guidelines_detail_panel(f, columns[1], app, theme)
            }
            WorkspaceSection::Refs => {
                draw_refs_detail_panel(f, columns[1], app, theme)
            }
        }
    }

    if background_enabled {
        render_background_watermark_with_state(
            f.buffer_mut(),
            area,
            app.anim_elapsed,
            app.mascot_anim_state(),
            app.animations_enabled(),
            theme.mono,
            theme,
        );
    }
}

fn draw_server_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    fx_areas: &crate::fx::FxAreas,
) {
    let servers = app.visible_servers();
    let total = servers.len();
    let selected_idx = app.selected.min(total.saturating_sub(1));
    let pos_suffix = if total > 0 {
        format!(" ({}/{total})", selected_idx + 1)
    } else {
        String::new()
    };

    let filter_active = matches!(app.mode, ActiveMode::Filtering);
    let title = if filter_active {
        format!(" SERVERS{pos_suffix} · /{}█ ", app.filter_query)
    } else if !app.filter_query.is_empty() {
        format!(
            " SERVERS{pos_suffix} · /{} (Esc to clear) ",
            app.filter_query
        )
    } else {
        format!(" SERVERS{pos_suffix} ")
    };

    let border_style = if (matches!(app.mode, ActiveMode::Normal) && !app.details_focused)
        || matches!(app.mode, ActiveMode::Filtering)
    {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(area);

    if servers.is_empty() {
        let mut lines = build_ascii_splash_lines(theme, app.anim_tick, app.animations_enabled());
        lines.push(Line::from(""));
        if !app.filter_query.is_empty() {
            lines.push(Line::from(Span::styled(
                "No MCP servers match the current filter.",
                theme.muted,
            )));
            lines.push(Line::from(Span::styled(
                "Press Esc to clear filter.",
                theme.accent,
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "No MCP servers configured yet.",
                theme.muted,
            )));
            lines.push(Line::from(Span::styled(
                "Press 'a' to add one or 'E' to open $EDITOR.",
                theme.accent,
            )));
        }
        let p = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(p, area);
        return;
    }

    let max_name_width = inner.width.saturating_sub(15) as usize;

    let items: Vec<ListItem> = servers
        .iter()
        .enumerate()
        .map(|(idx, (name, cfg, conflicted))| {
            let is_selected = idx == selected_idx;
            let cursor = if is_selected { "▸ " } else { "  " };

            let (badge, badge_style) = if *conflicted {
                ("[!]", theme.warning)
            } else if cfg.enabled {
                ("[x]", theme.success)
            } else {
                ("[ ]", theme.muted)
            };

            let display_name = truncate_display_width(name, max_name_width.max(8));
            let pad_len = max_name_width
                .max(8)
                .saturating_sub(UnicodeWidthStr::width(display_name.as_str()));
            let padding = " ".repeat(pad_len + 1);
            let (probe_glyph, probe_style) = match app.probe_state(name) {
                Some(ServerProbeState::Probing) => ("↻ ", theme.accent),
                Some(ServerProbeState::Online(_)) => ("● ", theme.success),
                Some(ServerProbeState::Failed { .. }) => ("✗ ", theme.error),
                None => ("  ", theme.muted),
            };

            let row_style = if is_selected {
                theme.highlight
            } else if cfg.enabled {
                theme.base
            } else {
                theme.muted
            };

            let line = Line::from(vec![
                Span::styled(cursor, row_style),
                Span::styled(badge, if is_selected { row_style } else { badge_style }),
                Span::styled(" ", row_style),
                Span::styled(display_name, row_style),
                Span::styled(padding, row_style),
                Span::styled(
                    probe_glyph,
                    if is_selected { row_style } else { probe_style },
                ),
                Span::styled(
                    format!("{:>5}", cfg.transport.as_str()),
                    if is_selected { row_style } else { theme.muted },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let viewport_height = inner.height.max(1) as usize;
    let mut offset = app.scroll_offset;
    if selected_idx < offset {
        offset = selected_idx;
    } else if selected_idx >= offset + viewport_height {
        offset = selected_idx + 1 - viewport_height;
    }
    offset = offset.min(total.saturating_sub(viewport_height));

    let mut list_state = ListState::default()
        .with_offset(offset)
        .with_selected(Some(selected_idx));

    if selected_idx >= offset && selected_idx < offset + viewport_height {
        let row_y = inner.y.saturating_add((selected_idx - offset) as u16);
        fx_areas
            .selected_row
            .set(Rect::new(inner.x, row_y, inner.width, 1));
    }

    let list = List::new(items).block(block);
    f.render_stateful_widget(list, area, &mut list_state);

    if total > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total).position(selected_idx);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme.muted);
        let scrollbar_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: area.height.saturating_sub(2),
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn draw_plugin_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    fx_areas: &crate::fx::FxAreas,
) {
    let rows = app.visible_plugin_rows();
    let total = rows.len();
    let selected_idx = app.plugin_selected.min(total.saturating_sub(1));
    let (enabled_plugins, total_plugins) = app.total_plugins_counts();
    let pos_suffix = format!(" ({enabled_plugins}/{total_plugins})");

    let filter_active = matches!(app.mode, ActiveMode::Filtering);
    let title = if filter_active {
        format!(" PLUGINS{pos_suffix} · /{}█ ", app.plugin_filter_query)
    } else if !app.plugin_filter_query.is_empty() {
        format!(
            " PLUGINS{pos_suffix} · /{} (Esc to clear) ",
            app.plugin_filter_query
        )
    } else {
        format!(" PLUGINS{pos_suffix} ")
    };

    let border_style = if (matches!(app.mode, ActiveMode::Normal) && !app.details_focused)
        || matches!(app.mode, ActiveMode::Filtering)
    {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(area);

    if rows.is_empty() {
        let mut lines = build_ascii_splash_lines(theme, app.anim_tick, app.animations_enabled());
        lines.push(Line::from(""));
        if !app.plugin_filter_query.is_empty() {
            lines.push(Line::from(Span::styled(
                "No harness plugins match the current filter.",
                theme.muted,
            )));
            lines.push(Line::from(Span::styled(
                "Press Esc to clear filter.",
                theme.accent,
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "No enabled harnesses configured.",
                theme.muted,
            )));
            lines.push(Line::from(Span::styled(
                "Press 'o' to enable harnesses in Options.",
                theme.accent,
            )));
        }
        let p = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(p, area);
        return;
    }

    let max_name_width = inner.width.saturating_sub(15) as usize;

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let is_selected = idx == selected_idx;
            match row {
                PluginRow::HarnessHeader {
                    harness,
                    enabled_count,
                    total_count,
                    collapsed,
                } => {
                    let fold_glyph = if *collapsed { "▸" } else { "▾" };
                    let row_style = if is_selected {
                        theme.highlight
                    } else {
                        theme.header
                    };
                    let count_style = if is_selected {
                        theme.highlight
                    } else {
                        theme.muted
                    };
                    let line = Line::from(vec![
                        Span::styled(format!("{fold_glyph} "), row_style),
                        Span::styled(harness.display_name(), row_style),
                        Span::styled(format!(" ({enabled_count}/{total_count})"), count_style),
                    ]);
                    ListItem::new(line)
                }
                PluginRow::Plugin { plugin, .. } => {
                    let cursor = if is_selected { "▸ " } else { "  " };
                    let (badge, badge_style) = if plugin.enabled {
                        ("[x]", theme.success)
                    } else {
                        ("[ ]", theme.muted)
                    };
                    let display_name =
                        truncate_display_width(&plugin.display_name, max_name_width.max(8));
                    let pad_len = max_name_width
                        .max(8)
                        .saturating_sub(UnicodeWidthStr::width(display_name.as_str()));
                    let padding = " ".repeat(pad_len + 1);
                    let row_style = if is_selected {
                        theme.highlight
                    } else if plugin.enabled {
                        theme.base
                    } else {
                        theme.muted
                    };
                    let line = Line::from(vec![
                        Span::styled(cursor, row_style),
                        Span::styled(badge, if is_selected { row_style } else { badge_style }),
                        Span::styled(" ", row_style),
                        Span::styled(display_name, row_style),
                        Span::styled(padding, row_style),
                        Span::styled(
                            format!("{:>6}", plugin.badge_type_label()),
                            if is_selected { row_style } else { theme.muted },
                        ),
                    ]);
                    ListItem::new(line)
                }
            }
        })
        .collect();

    let viewport_height = inner.height.max(1) as usize;
    let mut offset = app.plugin_scroll_offset;
    if selected_idx < offset {
        offset = selected_idx;
    } else if selected_idx >= offset + viewport_height {
        offset = selected_idx + 1 - viewport_height;
    }
    offset = offset.min(total.saturating_sub(viewport_height));

    let mut list_state = ListState::default()
        .with_offset(offset)
        .with_selected(Some(selected_idx));

    if selected_idx >= offset && selected_idx < offset + viewport_height {
        let row_y = inner.y.saturating_add((selected_idx - offset) as u16);
        fx_areas
            .selected_row
            .set(Rect::new(inner.x, row_y, inner.width, 1));
    }

    let list = List::new(items).block(block);
    f.render_stateful_widget(list, area, &mut list_state);

    if total > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total).position(selected_idx);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme.muted);
        let scrollbar_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: area.height.saturating_sub(2),
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn draw_plugin_detail_panel(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let selected = app.selected_plugin_row();
    let subject = match &selected {
        Some(PluginRow::HarnessHeader { harness, .. }) => Some(harness.display_name().to_string()),
        Some(PluginRow::Plugin { plugin, .. }) => Some(plugin.display_name.clone()),
        None => None,
    };
    let title = match (&subject, app.details_focused) {
        (Some(name), true) => format!(" DETAILS · {} (j/k: scroll · t/Esc: plugins) ", name),
        (Some(name), false) => format!(" DETAILS · {} ", name),
        (None, _) => " DETAILS ".to_string(),
    };

    let border_style = if matches!(app.mode, ActiveMode::Normal) && app.details_focused {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(area);

    let Some(row) = selected else {
        let p = Paragraph::new("Select a harness group or plugin to inspect its details.")
            .style(theme.muted)
            .block(block);
        f.render_widget(p, area);
        return;
    };

    let lines = match row {
        PluginRow::HarnessHeader {
            harness,
            enabled_count,
            total_count,
            collapsed,
        } => build_harness_group_inspector_lines(
            harness,
            enabled_count,
            total_count,
            collapsed,
            theme,
        ),
        PluginRow::Plugin { plugin, .. } => build_plugin_inspector_lines(&plugin, theme),
    };

    let total_lines = lines.len();
    let viewport_height = inner.height.max(1) as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let scroll_y = app.details_scroll.min(max_scroll);

    let p = Paragraph::new(lines)
        .block(block)
        .scroll((scroll_y as u16, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);

    if total_lines > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll_y);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(if app.details_focused {
                theme.accent
            } else {
                theme.muted
            });
        let scrollbar_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: area.height.saturating_sub(2),
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn build_harness_group_inspector_lines<'a>(
    harness: HarnessKind,
    enabled_count: usize,
    total_count: usize,
    collapsed: bool,
    theme: &Theme,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    let state_label = if collapsed {
        "▸ Collapsed (press Space or Enter to expand)"
    } else {
        "▾ Expanded (press Space or Enter to collapse)"
    };
    lines.push(Line::from(vec![
        Span::styled("Harness    : ", theme.muted),
        Span::styled(harness.display_name(), theme.bold),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Plugins    : ", theme.muted),
        Span::styled(
            format!("{enabled_count} enabled / {total_count} discovered"),
            theme.accent,
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Group State: ", theme.muted),
        Span::styled(state_label, theme.base),
    ]));
    lines.push(Line::from(""));
    if total_count == 0 {
        lines.push(Line::from(Span::styled(
            "  (no plugins discovered for this harness)",
            theme.muted,
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Navigate down with j / ↓ to inspect and toggle individual plugins.",
            theme.muted,
        )));
    }
    lines
}

fn build_plugin_inspector_lines<'a>(plugin: &HarnessPlugin, theme: &Theme) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    let (status_text, status_style) = if plugin.enabled {
        (
            format!("✓ Enabled in {}", plugin.harness.display_name()),
            theme.success,
        )
    } else {
        (
            format!("○ Disabled in {}", plugin.harness.display_name()),
            theme.muted,
        )
    };

    let kind_source = match &plugin.source {
        Some(src) if !src.is_empty() => format!("{} · {src}", plugin.kind.as_str()),
        _ => plugin.kind.as_str().to_string(),
    };

    lines.push(Line::from(vec![
        Span::styled("Harness    : ", theme.muted),
        Span::styled(plugin.harness.display_name(), theme.bold),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Identifier : ", theme.muted),
        Span::styled(plugin.id.clone(), theme.bold),
    ]));
    if let Some(ref desc) = plugin.description {
        lines.push(Line::from(vec![
            Span::styled("Description: ", theme.muted),
            Span::styled(desc.clone(), theme.base),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Status     : ", theme.muted),
        Span::styled(status_text, status_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Kind/Source: ", theme.muted),
        Span::styled(kind_source, theme.accent),
    ]));
    if let Some(ref version) = plugin.version {
        lines.push(Line::from(vec![
            Span::styled("Version    : ", theme.muted),
            Span::styled(version.clone(), theme.base),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Config Path: ", theme.muted),
        Span::styled(plugin.config_path.display().to_string(), theme.base),
    ]));
    if let Some(ref install_path) = plugin.install_path {
        lines.push(Line::from(vec![
            Span::styled("Install Dir: ", theme.muted),
            Span::styled(install_path.display().to_string(), theme.base),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Bundled Contents", theme.header)));
    if plugin.bundled.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no local bundled skills, hooks, or MCPs detected)",
            theme.muted,
        )));
    } else {
        let mut skill_spans = vec![
            Span::styled("  • Skills : ", theme.muted),
            Span::styled(plugin.bundled.skills.to_string(), theme.base),
        ];
        if !plugin.bundled.skill_names.is_empty() {
            skill_spans.push(Span::styled(
                format!(" ({})", plugin.bundled.skill_names.join(", ")),
                theme.muted,
            ));
        }
        lines.push(Line::from(skill_spans));

        let mut hook_spans = vec![
            Span::styled("  • Hooks  : ", theme.muted),
            Span::styled(plugin.bundled.hooks.to_string(), theme.base),
        ];
        if !plugin.bundled.hook_names.is_empty() {
            hook_spans.push(Span::styled(
                format!(" ({})", plugin.bundled.hook_names.join(", ")),
                theme.muted,
            ));
        }
        lines.push(Line::from(hook_spans));

        let mut mcp_spans = vec![
            Span::styled("  • MCPs   : ", theme.muted),
            Span::styled(plugin.bundled.mcps.to_string(), theme.base),
        ];
        if !plugin.bundled.mcp_names.is_empty() {
            mcp_spans.push(Span::styled(
                format!(" ({})", plugin.bundled.mcp_names.join(", ")),
                theme.muted,
            ));
        }
        lines.push(Line::from(mcp_spans));
    }

    lines
}

fn draw_detail_panel(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let selected = app.selected_server();
    let title = match (&selected, app.details_focused) {
        (Some((name, _, _)), true) => {
            format!(" DETAILS · {} (j/k: scroll · t/Esc: servers) ", name)
        }
        (Some((name, _, _)), false) => format!(" DETAILS · {} ", name),
        (None, _) => " DETAILS ".to_string(),
    };

    let border_style = if matches!(app.mode, ActiveMode::Normal) && app.details_focused {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(area);

    let Some((name, cfg, conflicted)) = selected else {
        let p = Paragraph::new("Select an MCP server to inspect its configuration.")
            .style(theme.muted)
            .block(block);
        f.render_widget(p, area);
        return;
    };

    let targets = app.enabled_harnesses_label();
    let probe_state = app.probe_state(&name);
    let lines = build_inspector_lines(&name, &cfg, conflicted, &targets, probe_state, theme);
    let total_lines = lines.len();
    let viewport_height = inner.height.max(1) as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let scroll_y = app.details_scroll.min(max_scroll);

    let p = Paragraph::new(lines)
        .block(block)
        .scroll((scroll_y as u16, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);

    if total_lines > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll_y);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(if app.details_focused {
                theme.accent
            } else {
                theme.muted
            });
        let scrollbar_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: area.height.saturating_sub(2),
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn draw_hero_showcase_panel(
    f: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    fx_areas: &crate::fx::FxAreas,
) {
    let sp = sparkle_glyph(app.anim_tick, app.animations_enabled(), theme.mono);
    let block = Block::default()
        .title(Span::styled(
            format!(" {sp} MAGA · HERO SHOWCASE (m: inspector) "),
            theme.header,
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.active_border)
        .padding(Padding::new(PADDING_XS, PADDING_XS, 0, 0));

    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let art_height = inner.height.saturating_sub(1).max(1);
    let art_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: art_height,
    };
    fx_areas.companion_portrait.set(art_area);

    let hero_lines = render_hero_mascot_lines_with_state(
        art_area.width,
        art_area.height,
        app.anim_elapsed,
        app.mascot_anim_state(),
        app.animations_enabled(),
        theme.mono,
        theme,
    );
    f.render_widget(
        Paragraph::new(hero_lines).alignment(Alignment::Center),
        art_area,
    );

    if inner.height > 1 {
        let footer_area = Rect {
            x: inner.x,
            y: inner.y.saturating_add(inner.height.saturating_sub(1)),
            width: inner.width,
            height: 1,
        };
        let footer = Line::from(vec![
            Span::styled(format!("{sp} "), theme.accent),
            Span::styled("Arcane ASCII Showcase", theme.bold),
            Span::styled(" · m: inspector · o: options", theme.muted),
        ]);
        f.render_widget(
            Paragraph::new(footer).alignment(Alignment::Center),
            footer_area,
        );
    }
}

fn draw_companion_panel(
    f: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    fx_areas: &crate::fx::FxAreas,
) {
    let sp = sparkle_glyph(app.anim_tick, app.animations_enabled(), theme.mono);
    let block = Block::default()
        .title(Span::styled(
            format!(" {sp} COMPANION · Maga "),
            theme.header,
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border)
        .padding(Padding::new(PADDING_XS, PADDING_XS, 0, 0));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(10)])
        .split(inner);

    fx_areas.companion_portrait.set(cols[0]);
    let mood = companion_mood_for_app(app);
    let mut portrait = Vec::with_capacity(7);
    portrait.push(Line::from(""));
    portrait.extend(render_chibi_companion_with_state(
        mood,
        app.anim_tick,
        app.mascot_anim_state(),
        app.animations_enabled() && !app.anim_elapsed.is_zero(),
        theme.mono,
        theme,
    ));
    f.render_widget(Paragraph::new(portrait), cols[0]);

    let dialogue_area = Rect {
        x: cols[1].x,
        y: cols[1].y.saturating_add(1),
        width: cols[1].width,
        height: cols[1].height.saturating_sub(1),
    };
    let dialogue = build_companion_dialogue(app, theme, app.anim_tick);
    f.render_widget(
        Paragraph::new(dialogue).wrap(Wrap { trim: false }),
        dialogue_area,
    );
}

fn build_inspector_lines<'a>(
    name: &str,
    cfg: &McpServerConfig,
    conflicted: bool,
    targets: &str,
    probe_state: Option<&ServerProbeState>,
    theme: &Theme,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let (status_text, status_style) = if conflicted {
        (
            "! Conflict Pending (Press 'c' to resolve)".to_string(),
            theme.warning,
        )
    } else if cfg.enabled {
        (format!("✓ Enabled ({targets})"), theme.success)
    } else {
        (format!("○ Disabled ({targets})"), theme.muted)
    };

    let (health_text, health_style) = match probe_state {
        Some(ServerProbeState::Probing) => (
            "↻ Probing (MCP initialize + tools/list)…".to_string(),
            theme.accent,
        ),
        Some(ServerProbeState::Online(outcome)) => {
            let mut meta = format!("✓ Online ({}ms)", outcome.latency_ms);
            if let Some(ref info) = outcome.server_info {
                meta.push_str(&format!(" · {info}"));
            } else if let Some(ref proto) = outcome.protocol_version {
                meta.push_str(&format!(" · MCP {proto}"));
            }
            (meta, theme.success)
        }
        Some(ServerProbeState::Failed { error, latency_ms }) => {
            (format!("✗ Failed ({latency_ms}ms) · {error}"), theme.error)
        }
        None => (
            "○ Not probed yet (press 'p' to probe, 'P' for all)".to_string(),
            theme.muted,
        ),
    };

    lines.push(Line::from(vec![
        Span::styled("Name       : ", theme.muted),
        Span::styled(name.to_string(), theme.bold),
    ]));
    if let Some(ref desc) = cfg.description {
        lines.push(Line::from(vec![
            Span::styled("Description: ", theme.muted),
            Span::styled(desc.clone(), theme.base),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Status     : ", theme.muted),
        Span::styled(status_text, status_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Transport  : ", theme.muted),
        Span::styled(cfg.transport.as_str(), theme.accent),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Live Health: ", theme.muted),
        Span::styled(health_text, health_style),
    ]));
    lines.push(Line::from(""));

    match cfg.transport {
        Transport::Stdio => {
            lines.push(Line::from(vec![
                Span::styled("Command    : ", theme.muted),
                Span::styled(cfg.command.clone().unwrap_or_default(), theme.base),
            ]));
            let args_display = if cfg.args.is_empty() {
                "(none)".to_string()
            } else {
                cfg.args.join(" ")
            };
            lines.push(Line::from(vec![
                Span::styled("Args       : ", theme.muted),
                Span::styled(args_display, theme.base),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Environment ({})", cfg.env.len()),
                theme.header,
            )));
            if cfg.env.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (no environment variables)",
                    theme.muted,
                )));
            } else {
                for (k, v) in &cfg.env {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {k} = "), theme.muted),
                        Span::styled(mask_sensitive_value(k, v), theme.base),
                    ]));
                }
            }
        }
        Transport::Http => {
            lines.push(Line::from(vec![
                Span::styled("URL        : ", theme.muted),
                Span::styled(cfg.url.clone().unwrap_or_default(), theme.base),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("HTTP Headers ({})", cfg.headers.len()),
                theme.header,
            )));
            if cfg.headers.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (no custom headers)",
                    theme.muted,
                )));
            } else {
                for (k, v) in &cfg.headers {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {k} : "), theme.muted),
                        Span::styled(mask_sensitive_value(k, v), theme.base),
                    ]));
                }
            }
        }
    }

    if let Some(ServerProbeState::Online(outcome)) = probe_state {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("Exposed Tools ({})", outcome.tools.len()),
                theme.header,
            ),
            Span::styled("  (press 't' to scroll)", theme.muted),
        ]));
        if outcome.tools.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no tools returned by tools/list)",
                theme.muted,
            )));
        } else {
            for tool in &outcome.tools {
                match &tool.description {
                    Some(desc) => {
                        lines.push(Line::from(vec![
                            Span::styled(format!("  • {}", tool.name), theme.accent),
                            Span::styled(format!(" — {desc}"), theme.base),
                        ]));
                    }
                    None => {
                        lines.push(Line::from(Span::styled(
                            format!("  • {}", tool.name),
                            theme.accent,
                        )));
                    }
                }
            }
        }
    }

    lines
}

fn mask_sensitive_value(key: &str, val: &str) -> String {
    let lower = key.to_ascii_lowercase();
    if (lower.contains("key")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("auth"))
        && val.len() > 8
    {
        let prefix: String = val.chars().take(4).collect();
        format!("{prefix}••••••")
    } else {
        val.to_string()
    }
}

/// Truncates `text` cleanly on Unicode character boundaries so its terminal display width does not exceed `max_width`.
fn draw_guidelines_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    _fx_areas: &crate::fx::FxAreas,
) {
    let statuses = &app.guidelines_statuses;
    let total = statuses.len();
    let selected_idx = app.guidelines_selected.min(total.saturating_sub(1));
    let synced = statuses
        .iter()
        .filter(|s| s.exists && s.matches_canonical)
        .count();

    let title = format!(" HARNESS GUIDELINES ({synced}/{total} Synced) ");
    let border_style = if matches!(app.mode, ActiveMode::Normal) && !app.details_focused {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(area);

    let items: Vec<ListItem> = statuses
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_selected = idx == selected_idx;
            let cursor = if is_selected { "▸ " } else { "  " };

            let (badge, badge_style) = if s.exists && s.matches_canonical {
                let sym_tag = if s.is_symlink {
                    "[✓ Symlink]"
                } else {
                    "[✓ Link]"
                };
                (sym_tag, theme.success)
            } else if s.exists {
                ("[! Drift]", theme.warning)
            } else {
                ("[✗ Missing]", theme.error)
            };

            let row_style = if is_selected {
                theme.highlight
            } else {
                theme.base
            };

            let harness_name = s.harness.to_string();
            let path_str = s.path.display().to_string();
            let path_avail_width = inner.width.saturating_sub(35) as usize;
            let truncated_path = truncate_display_width(&path_str, path_avail_width.max(10));

            let line = Line::from(vec![
                Span::styled(
                    cursor,
                    if is_selected {
                        theme.accent
                    } else {
                        theme.muted
                    },
                ),
                Span::styled(format!("{badge:<13} "), badge_style),
                Span::styled(format!("{harness_name:<22} "), row_style),
                Span::styled(truncated_path, theme.muted),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_guidelines_detail_panel(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let border_style = if app.details_focused {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(" GUIDELINES INSPECTOR ", theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let canonical = crate::domain::guidelines::canonical_guidelines_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "~/.agents/AGENTS.md".to_string());

    let selected_status = app.guidelines_statuses.get(app.guidelines_selected);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Canonical Source: ", theme.header),
        Span::styled(canonical, theme.accent),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Role:             ", theme.muted),
        Span::styled(
            "Single source of truth for developer communication & cognitive ergonomics.",
            theme.base,
        ),
    ]));

    if let Ok(Some(metrics)) = crate::domain::guidelines::canonical_guidelines_metrics() {
        let (budget_label, budget_style) = if metrics.exceeds_recommended_budget {
            ("⚠ OVERSIZED (Prompt Bloat Warning)", theme.warning)
        } else {
            ("✓ Healthy (Within Budget)", theme.success)
        };

        lines.push(Line::from(vec![
            Span::styled("Prompt Budget:    ", theme.muted),
            Span::styled(
                format!("{} lines, {} chars ", metrics.line_count, metrics.char_count),
                theme.base,
            ),
            Span::styled(budget_label, budget_style),
        ]));
    }

    let refs_count = app.loaded.refs.refs.len();
    lines.push(Line::from(vec![
        Span::styled("Offline Refs:     ", theme.muted),
        Span::styled(
            format!("{refs_count} tracked in refs.toml"),
            theme.base,
        ),
    ]));
    lines.push(Line::from(""));

    if let Some(s) = selected_status {
        lines.push(Line::from(vec![
            Span::styled("Selected Harness: ", theme.header),
            Span::styled(s.harness.to_string(), theme.highlight),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Target Path:      ", theme.muted),
            Span::styled(s.path.display().to_string(), theme.base),
        ]));

        let (status_text, status_style) = if s.exists && s.matches_canonical {
            let link_desc = if s.is_symlink {
                "Valid Symbolic Link (In Sync)"
            } else {
                "Valid Hard Link (In Sync)"
            };
            (link_desc, theme.success)
        } else if s.exists {
            ("Content Differs from Canonical (Drift)", theme.warning)
        } else {
            ("Target File Missing", theme.error)
        };

        lines.push(Line::from(vec![
            Span::styled("Link Health:      ", theme.muted),
            Span::styled(status_text, status_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Is Symlink:       ", theme.muted),
            Span::styled(if s.is_symlink { "Yes" } else { "No" }, theme.base),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Actions:", theme.header)));
    lines.push(Line::from(vec![
        Span::styled("  e / Enter  ", theme.accent),
        Span::styled("Open ~/.agents/AGENTS.md in configured $EDITOR", theme.base),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  s          ", theme.accent),
        Span::styled("Re-synchronize & repair links to all 6 harnesses", theme.base),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  r          ", theme.accent),
        Span::styled("Refresh live status from filesystem", theme.base),
    ]));

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

fn draw_refs_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    _fx_areas: &crate::fx::FxAreas,
) {
    let refs_map = &app.loaded.refs.refs;
    let total = refs_map.len();
    let selected_idx = app.refs_selected.min(total.saturating_sub(1));

    let cache_dir = app
        .paths
        .as_ref()
        .map(|p| p.refs_cache_dir.clone())
        .unwrap_or_else(|| std::path::PathBuf::from("refs"));

    let cloned_count = refs_map
        .iter()
        .filter(|(alias, entry)| {
            let p = app.loaded.refs.effective_path(alias, entry, &cache_dir);
            p.exists() && p.join(".git").exists()
        })
        .count();

    let title = format!(" OFFLINE REFERENCES ({cloned_count}/{total} Cloned) ");
    let border_style = if matches!(app.mode, ActiveMode::Normal) && !app.details_focused {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = block.inner(area);

    if refs_map.is_empty() {
        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled("No references tracked in refs.toml", theme.muted)),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", theme.muted),
                Span::styled("e", theme.accent),
                Span::styled(" to open refs.toml in $EDITOR and add repositories.", theme.muted),
            ]),
        ];
        let p = Paragraph::new(empty_lines).block(block);
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = refs_map
        .iter()
        .enumerate()
        .map(|(idx, (alias, entry))| {
            let is_selected = idx == selected_idx;
            let cursor = if is_selected { "▸ " } else { "  " };

            let target_path = app.loaded.refs.effective_path(alias, entry, &cache_dir);
            let is_cloned = target_path.exists() && target_path.join(".git").exists();

            let (badge, badge_style) = if is_cloned {
                ("[✓ Cloned]", theme.success)
            } else {
                ("[✗ Not Cloned]", theme.warning)
            };

            let row_style = if is_selected {
                theme.highlight
            } else {
                theme.base
            };

            let path_str = target_path.display().to_string();
            let path_avail_width = inner.width.saturating_sub(35) as usize;
            let truncated_path = truncate_display_width(&path_str, path_avail_width.max(10));

            let line = Line::from(vec![
                Span::styled(cursor, theme.accent),
                Span::styled(format!("{:<16} ", alias), row_style),
                Span::styled(format!("{:<15} ", badge), badge_style),
                Span::styled(truncated_path, theme.muted),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_refs_detail_panel(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let border_style = if app.details_focused {
        theme.active_border
    } else {
        theme.border
    };

    let block = Block::default()
        .title(Span::styled(" REFERENCE INSPECTOR ", theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let cache_dir = app
        .paths
        .as_ref()
        .map(|p| p.refs_cache_dir.clone())
        .unwrap_or_else(|| std::path::PathBuf::from("refs"));

    let selected_entry = app
        .loaded
        .refs
        .refs
        .iter()
        .nth(app.refs_selected);

    let mut lines = Vec::new();

    if let Some((alias, entry)) = selected_entry {
        let target_path = app.loaded.refs.effective_path(alias, entry, &cache_dir);
        let is_cloned = target_path.exists() && target_path.join(".git").exists();

        lines.push(Line::from(vec![
            Span::styled("Alias:        ", theme.header),
            Span::styled(alias.clone(), theme.accent),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Repository:   ", theme.muted),
            Span::styled(entry.url.clone(), theme.base),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Branch/Tag:   ", theme.muted),
            Span::styled(
                entry.branch.as_deref().unwrap_or("default (HEAD)"),
                theme.base,
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Description:  ", theme.muted),
            Span::styled(
                entry.description.as_deref().unwrap_or("-"),
                theme.base,
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Local Path:   ", theme.muted),
            Span::styled(target_path.display().to_string(), theme.base),
        ]));

        let (status_text, status_style) = if is_cloned {
            ("Cloned (Available offline for AI agents)", theme.success)
        } else {
            ("Not Cloned (Pending shallow clone download)", theme.warning)
        };

        lines.push(Line::from(vec![
            Span::styled("Clone State:  ", theme.muted),
            Span::styled(status_text, status_style),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No reference selected.",
            theme.muted,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Actions:", theme.header)));
    lines.push(Line::from(vec![
        Span::styled("  c / Enter  ", theme.accent),
        Span::styled("Shallow-clone or refresh selected repository", theme.base),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  u          ", theme.accent),
        Span::styled("Pull latest commits (git pull --depth 1)", theme.base),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  e / E      ", theme.accent),
        Span::styled("Open refs.toml in configured $EDITOR", theme.base),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  s          ", theme.accent),
        Span::styled("Re-synchronize guidelines and update offline lookup index", theme.base),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  r          ", theme.accent),
        Span::styled("Reload references from disk", theme.base),
    ]));

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

pub fn truncate_display_width(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut current_w = 0;
    for ch in text.chars() {
        let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_w + ch_w + 1 > max_width {
            break;
        }
        out.push(ch);
        current_w += ch_w;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_unicode_strings_by_display_width() {
        let s = "日本語_mcp_server_name";
        let truncated = truncate_display_width(s, 10);
        assert!(UnicodeWidthStr::width(truncated.as_str()) <= 10);
        assert!(truncated.ends_with('…'));
    }
}
