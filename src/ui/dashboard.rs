use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Padding, Paragraph};
use ratatui::Frame;

use super::theme::{self, c, fuel_color, format_num, panel_alert, AlertKind};
use crate::state::AppState;

pub fn draw_dashboard(f: &mut Frame, area: Rect, state: &AppState) {
    let t = &state.telemetry;
    let ratio = t.fuel_ratio();
    let fc = fuel_color(ratio, t.budget_hard_hit);

    let alert = if t.budget_hard_hit {
        AlertKind::Danger
    } else if state.show_confirm_rollback || t.budget_soft_hit || ratio >= 0.75 {
        AlertKind::Warn
    } else {
        AlertKind::Ok
    };

    let outer = panel_alert("TELEMETRY", alert).padding(Padding::horizontal(1));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // fuel
            Constraint::Length(1), // spacer rule
            Constraint::Length(5), // metric cards row 1
            Constraint::Length(5), // metric cards row 2
            Constraint::Min(2),    // status strip
        ])
        .split(inner);

    // —— Context fuel ——
    let src = if t.signals_source { "LIVE" } else { "EST" };
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title(Line::from(vec![
                    Span::styled(" CONTEXT  ", theme::style_muted()),
                    Span::styled(
                        format!("{:.0}%", ratio * 100.0),
                        Style::default().fg(fc).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "   {} / {}   ",
                            format_num(t.context_used as u64),
                            format_num(t.context_limit as u64)
                        ),
                        theme::style_dim(),
                    ),
                    Span::styled(
                        format!(" {src} "),
                        Style::default()
                            .fg(c::VOID)
                            .bg(if t.signals_source {
                                c::SUCCESS
                            } else {
                                c::BORDER
                            }),
                    ),
                ])),
        )
        .gauge_style(Style::default().fg(fc).bg(c::GAUGE_TRACK))
        .ratio(ratio)
        .label("");
    f.render_widget(gauge, chunks[0]);

    f.render_widget(Paragraph::new(theme::rule_line(chunks[1].width)), chunks[1]);

    // —— Metric cards row 1 ——
    let row1 = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(chunks[2]);

    draw_metric(
        f,
        row1[0],
        "RPM",
        &format!("{}/8", t.active_subagents),
        if t.active_subagents > 0 {
            c::ACCENT
        } else {
            c::TEXT_DIM
        },
        "sub-agents",
    );
    draw_metric(
        f,
        row1[1],
        "ODO",
        &format_num(t.total_tokens_lifetime),
        c::TEXT,
        "tokens life",
    );
    draw_metric(
        f,
        row1[2],
        "TRIP",
        &format_num(t.trip_tokens as u64),
        c::ACCENT_SOFT,
        "this prompt",
    );
    draw_metric(
        f,
        row1[3],
        "COST",
        &format!("${:.4}", t.trip_cost_usd),
        c::GOLD,
        "trip USD",
    );

    // —— Metric cards row 2 ——
    let row2 = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(chunks[3]);

    let temp_c = if t.error_temperature >= 40.0 {
        c::DANGER
    } else if t.error_temperature >= 15.0 {
        c::WARN
    } else {
        c::SUCCESS
    };
    draw_metric(
        f,
        row2[0],
        "TEMP",
        &format!("{:.0}°", t.error_temperature),
        temp_c,
        "error heat",
    );
    draw_metric(
        f,
        row2[1],
        "SPD",
        &format!("{:.0}", t.tokens_per_second),
        c::PURPLE,
        "tok/s",
    );
    draw_metric(
        f,
        row2[2],
        "BURN",
        &format!("${:.1}/h", t.burn_rate_usd_per_hour()),
        c::ORANGE,
        "pace",
    );
    draw_metric(
        f,
        row2[3],
        "SESS",
        &format!("${:.4}", t.session_cost_usd),
        if t.budget_hard_hit {
            c::DANGER
        } else if t.budget_soft_hit {
            c::WARN
        } else {
            c::GOLD
        },
        "session USD",
    );

    // —— Status ——
    let path = state
        .project_cwd
        .as_deref()
        .map(short_path)
        .unwrap_or_else(|| "awaiting SessionStart…".into());
    let status_lines = vec![
        Line::from(vec![
            Span::styled(" ● ", Style::default().fg(status_dot_color(state))),
            Span::styled(
                truncate(&state.status_line.replace('\n', " · "), 72),
                theme::style_text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("   ", Style::default()),
            Span::styled(&state.oauth_status, theme::style_dim()),
            Span::styled("  ·  ", theme::style_muted()),
            Span::styled(path, theme::style_muted()),
        ]),
    ];
    f.render_widget(Paragraph::new(status_lines), chunks[4]);
}

fn draw_metric(f: &mut Frame, area: Rect, label: &str, value: &str, color: ratatui::style::Color, hint: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(c::BORDER_MUTED))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(c::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(hint, theme::style_muted())),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn status_dot_color(state: &AppState) -> ratatui::style::Color {
    let t = &state.telemetry;
    if t.budget_hard_hit {
        c::DANGER
    } else if state.show_confirm_rollback || t.budget_soft_hit {
        c::WARN
    } else if t.active_subagents > 0 {
        c::ACCENT
    } else {
        c::SUCCESS
    }
}

fn short_path(p: &str) -> String {
    // Prefer last two path segments for polish
    let norm = p.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        truncate(p, 40)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
