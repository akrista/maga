use crate::domain::model::McpServerConfig;
use crate::domain::reconcile::{ConflictKind, McpConflict};
use crate::screens::form::centered_rect;
use crate::theme::{PADDING_SM, PADDING_XS, Theme};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};

/// Renders the grouped-variant Conflict Resolution modal.
pub fn draw_conflict_modal(
    f: &mut Frame,
    area: Rect,
    conflicts: &[McpConflict],
    index: usize,
    theme: &Theme,
) {
    if conflicts.is_empty() {
        return;
    }
    let idx = index.min(conflicts.len() - 1);
    let conflict = &conflicts[idx];

    let modal_area = centered_rect(area.width.saturating_sub(4).min(96), 20, area);
    f.render_widget(Clear, modal_area);

    let kind_label = match &conflict.kind {
        ConflictKind::DefinitionDrift => "Definition Drift".to_string(),
        ConflictKind::CaseCollision { alt_names } => {
            format!(
                "Case Collision ('{}' vs '{}')",
                conflict.name,
                alt_names.join("', '")
            )
        }
    };

    let title = format!(
        " ! CONFLICT ({}/{}) · {} · {} ",
        idx + 1,
        conflicts.len(),
        conflict.name,
        kind_label
    );

    let outer = Block::default()
        .title(Span::styled(title, theme.warning))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.warning)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let inner = outer.inner(modal_area);
    f.render_widget(outer, modal_area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let diff_fields = conflict.differing_fields().join(", ");
    let banner = Paragraph::new(Line::from(vec![
        Span::styled("Differing fields: ", theme.muted),
        Span::styled(
            if diff_fields.is_empty() {
                "server state".to_string()
            } else {
                diff_fields
            },
            theme.warning,
        ),
    ]));
    f.render_widget(banner, rows[0]);

    let count = conflict.variants.len();
    if count > 0 {
        let constraints: Vec<Constraint> = (0..count)
            .map(|_| Constraint::Ratio(1, count as u32))
            .collect();
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(rows[1]);

        for (v_idx, variant) in conflict.variants.iter().enumerate() {
            let card_title = format!(
                " [{}] {} ({}) ",
                v_idx + 1,
                variant.sources_label(),
                variant.name
            );
            draw_side_card(f, cols[v_idx], &card_title, Some(&variant.config), theme);
        }
    }

    let mut action_spans = Vec::new();
    for (v_idx, variant) in conflict.variants.iter().enumerate() {
        action_spans.push(Span::styled(format!("[{}]", v_idx + 1), theme.accent));
        action_spans.push(Span::styled(
            format!(" Keep {}   ", variant.sources_label()),
            theme.base,
        ));
    }
    action_spans.push(Span::styled("[E]", theme.accent));
    action_spans.push(Span::styled(" Merge in $EDITOR   ", theme.base));
    action_spans.push(Span::styled("[s/Esc]", theme.muted));
    action_spans.push(Span::styled(" Skip for now", theme.muted));

    let actions = Paragraph::new(Line::from(action_spans)).alignment(Alignment::Center);
    f.render_widget(actions, rows[2]);
}

fn draw_side_card(
    f: &mut Frame,
    area: Rect,
    title: &str,
    cfg: Option<&McpServerConfig>,
    theme: &Theme,
) {
    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border)
        .padding(Padding::horizontal(1));

    let Some(server) = cfg else {
        let p = Paragraph::new("(not present in this harness)")
            .style(theme.muted)
            .block(block);
        f.render_widget(p, area);
        return;
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Enabled   : ", theme.muted),
        Span::styled(
            if server.enabled {
                "✓ true"
            } else {
                "○ false"
            },
            if server.enabled {
                theme.success
            } else {
                theme.muted
            },
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Transport : ", theme.muted),
        Span::styled(server.transport.as_str(), theme.base),
    ]));
    if let Some(desc) = &server.description {
        lines.push(Line::from(vec![
            Span::styled("Desc      : ", theme.muted),
            Span::styled(desc.clone(), theme.base),
        ]));
    }
    if let Some(cmd) = &server.command {
        lines.push(Line::from(vec![
            Span::styled("Command   : ", theme.muted),
            Span::styled(cmd.clone(), theme.base),
        ]));
    }
    if !server.args.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Args      : ", theme.muted),
            Span::styled(server.args.join(" "), theme.base),
        ]));
    }
    if let Some(url) = &server.url {
        lines.push(Line::from(vec![
            Span::styled("URL       : ", theme.muted),
            Span::styled(url.clone(), theme.base),
        ]));
    }
    if !server.env.is_empty() {
        let keys: Vec<String> = server.env.keys().cloned().collect();
        lines.push(Line::from(vec![
            Span::styled("Env Keys  : ", theme.muted),
            Span::styled(keys.join(", "), theme.accent),
        ]));
    }
    if !server.headers.is_empty() {
        let keys: Vec<String> = server.headers.keys().cloned().collect();
        lines.push(Line::from(vec![
            Span::styled("Headers   : ", theme.muted),
            Span::styled(keys.join(", "), theme.accent),
        ]));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}
