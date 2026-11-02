use crate::app::{FormField, FormState};
use crate::domain::model::Transport;
use crate::theme::{PADDING_SM, PADDING_XS, Theme};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};

/// Renders the guided in-TUI Add/Edit modal over the workspace.
pub fn draw_form_modal(f: &mut Frame, area: Rect, form: &FormState, theme: &Theme) {
    let modal_area = centered_rect(72, 20, area);
    f.render_widget(Clear, modal_area);

    let title = if form.original_name.is_some() {
        " EDIT MCP SERVER (Tab: next  Enter: save  Esc: cancel) "
    } else {
        " ADD MCP SERVER (Tab: next  Enter: save  Esc: cancel) "
    };

    let block = Block::default()
        .title(Span::styled(title, theme.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.active_border)
        .padding(Padding::new(PADDING_SM, PADDING_SM, PADDING_XS, PADDING_XS));

    let mut lines = vec![
        render_field_line(
            "Name           ",
            &form.name,
            form.focused == FormField::Name,
            "e.g. context7 or astro-docs (no spaces)",
            theme,
        ),
        Line::from(""),
        render_field_line(
            "Description    ",
            &form.description,
            form.focused == FormField::Description,
            "optional notes or comments about this MCP server",
            theme,
        ),
        Line::from(""),
    ];

    let transport_display = match form.transport {
        Transport::Stdio => "[x] stdio    [ ] http   (Space/Left/Right to switch)",
        Transport::Http => "[ ] stdio    [x] http   (Space/Left/Right to switch)",
    };
    let transport_style = if form.focused == FormField::Transport {
        theme.accent
    } else {
        theme.base
    };
    lines.push(Line::from(vec![
        Span::styled(
            if form.focused == FormField::Transport {
                "▸ Transport      : "
            } else {
                "  Transport      : "
            },
            if form.focused == FormField::Transport {
                theme.accent
            } else {
                theme.muted
            },
        ),
        Span::styled(transport_display, transport_style),
    ]));
    lines.push(Line::from(""));

    match form.transport {
        Transport::Stdio => {
            lines.push(render_field_line(
                "Command        ",
                &form.endpoint,
                form.focused == FormField::Endpoint,
                "e.g. npx or /usr/local/bin/sqlite-mcp",
                theme,
            ));
            lines.push(Line::from(""));
            lines.push(render_field_line(
                "Args (spaced)  ",
                &form.args_input,
                form.focused == FormField::Args,
                "e.g. -y @upstash/context7-mcp",
                theme,
            ));
            lines.push(Line::from(""));
            lines.push(render_field_line(
                "Env (K=V, ...) ",
                &form.pairs_input,
                form.focused == FormField::Pairs,
                "e.g. APP_ENV=local, LOG_LEVEL=info",
                theme,
            ));
        }
        Transport::Http => {
            lines.push(render_field_line(
                "Endpoint URL   ",
                &form.endpoint,
                form.focused == FormField::Endpoint,
                "e.g. https://mcp.context7.com/mcp",
                theme,
            ));
            lines.push(Line::from(""));
            lines.push(render_field_line(
                "Headers (K=V)  ",
                &form.pairs_input,
                form.focused == FormField::Pairs,
                "e.g. Authorization=Bearer xxx",
                theme,
            ));
        }
    }

    if let Some(ref err) = form.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(format!("✗ {err}"), theme.error)));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, modal_area);
}

fn render_field_line<'a>(
    label: &'a str,
    value: &'a str,
    focused: bool,
    placeholder: &'a str,
    theme: &Theme,
) -> Line<'a> {
    let prefix = if focused { "▸ " } else { "  " };
    let label_style = if focused { theme.accent } else { theme.muted };
    let display_val = if value.is_empty() && !focused {
        Span::styled(placeholder, theme.muted)
    } else if focused {
        Span::styled(format!("{value}█"), theme.bold)
    } else {
        Span::styled(value, theme.base)
    };

    Line::from(vec![
        Span::styled(prefix, label_style),
        Span::styled(format!("{label}: "), label_style),
        display_val,
    ])
}

pub fn centered_rect(max_w: u16, max_h: u16, area: Rect) -> Rect {
    let w = max_w.min(area.width.saturating_sub(4)).max(20);
    let h = max_h.min(area.height.saturating_sub(2)).max(8);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
