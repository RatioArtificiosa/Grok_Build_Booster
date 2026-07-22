use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme::{self, c, spinner_frame};
use crate::state::AppState;

pub fn draw_header(f: &mut Frame, area: Rect, state: &AppState, tick: u64) {
    let t = &state.telemetry;
    let live = t.active_subagents > 0 || t.tokens_per_second > 1.0;
    let spin = if live {
        spinner_frame(tick)
    } else {
        '◆'
    };

    let oauth_ok = state.oauth_status.contains("logged in");
    let oauth_span = if oauth_ok {
        Span::styled(" OAuth ● ", Style::default().fg(c::SUCCESS))
    } else {
        Span::styled(" OAuth ○ ", Style::default().fg(c::TEXT_MUTED))
    };

    let budget = if t.budget_hard_hit {
        Span::styled(
            " BUDGET HARD ",
            Style::default()
                .fg(c::VOID)
                .bg(c::DANGER)
                .add_modifier(Modifier::BOLD),
        )
    } else if t.budget_soft_hit {
        Span::styled(
            " budget soft ",
            Style::default()
                .fg(c::VOID)
                .bg(c::WARN)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" budget ok ", Style::default().fg(c::SUCCESS))
    };

    let rpm = t.active_subagents;
    let model = t.model_id.as_deref().unwrap_or("—");
    let model_short = if model.len() > 18 {
        format!("{}…", &model[..16])
    } else {
        model.to_string()
    };

    let left = Line::from(vec![
        Span::styled(
            format!(" {spin} "),
            Style::default()
                .fg(if live { c::ACCENT } else { c::TEXT_DIM })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "GROK BUILD BOOSTER",
            Style::default()
                .fg(c::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", theme::style_muted()),
        Span::styled("MISSION CONTROL", theme::style_accent()),
    ]);

    let right = Line::from(vec![
        Span::styled(
            format!(" RPM {rpm}/8 "),
            if rpm > 0 {
                Style::default().fg(c::ACCENT).add_modifier(Modifier::BOLD)
            } else {
                theme::style_dim()
            },
        ),
        Span::styled("│", theme::style_muted()),
        Span::styled(format!(" {model_short} "), theme::style_dim()),
        Span::styled("│", theme::style_muted()),
        oauth_span,
        Span::styled("│", theme::style_muted()),
        budget,
        Span::raw(" "),
    ]);

    // Two-line header: brand + meta
    let fuel_pct = (t.fuel_ratio() * 100.0).round() as i32;
    let mid = Line::from(vec![
        Span::styled("  context ", theme::style_muted()),
        Span::styled(
            format!("{fuel_pct}%"),
            Style::default().fg(theme::fuel_color(t.fuel_ratio(), t.budget_hard_hit)),
        ),
        Span::styled("  ·  ", theme::style_muted()),
        Span::styled("sess ", theme::style_muted()),
        Span::styled(
            format!("${:.4}", t.session_cost_usd),
            Style::default().fg(c::GOLD),
        ),
        Span::styled("  ·  ", theme::style_muted()),
        Span::styled("turns ", theme::style_muted()),
        Span::styled(format!("{}", t.turn_count), theme::style_text()),
        Span::styled("  ·  ", theme::style_muted()),
        Span::styled(
            if t.signals_source {
                "live signals"
            } else {
                "estimated"
            },
            theme::style_dim(),
        ),
        Span::styled("  ·  ", theme::style_muted()),
        Span::styled(
            state.hooks_alive_label(),
            if state.hook_events_total > 0 {
                Style::default().fg(c::SUCCESS)
            } else {
                Style::default().fg(c::WARN)
            },
        ),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(c::BORDER))
        .style(Style::default());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);

    // Brand row: left brand, right status chips (best-effort single line)
    let brand_row = Line::from(
        left.spans
            .iter()
            .cloned()
            .chain(std::iter::once(Span::raw("  ")))
            .chain(right.spans.iter().cloned())
            .collect::<Vec<_>>(),
    );
    f.render_widget(Paragraph::new(brand_row), chunks[0]);
    f.render_widget(Paragraph::new(mid), chunks[1]);
}

pub fn draw_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let hints = if state.show_confirm_rollback {
        Line::from(vec![
            Span::styled(
                " CONFIRM REWIND ASSIST ",
                Style::default()
                    .fg(c::VOID)
                    .bg(c::WARN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(" Y ", Style::default().fg(c::VOID).bg(c::SUCCESS).add_modifier(Modifier::BOLD)),
            Span::styled(" yes  ", theme::style_dim()),
            Span::styled(" N ", Style::default().fg(c::VOID).bg(c::DANGER).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel ", theme::style_dim()),
        ])
    } else {
        let mut spans = Vec::new();
        for (k, l) in [
            ("↑↓", "nav"),
            ("R", "rewind"),
            ("E", "export"),
            ("S", "save"),
            ("q", "quit"),
        ] {
            spans.extend(theme::key_hint(k, l));
        }
        spans.push(Span::styled(
            format!("  {}", truncate_status(&state.status_line, 48)),
            theme::style_dim(),
        ));
        Line::from(spans)
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(c::BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(hints), inner);
}

fn truncate_status(s: &str, max: usize) -> String {
    let t = s.replace('\n', " · ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}…", t.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
