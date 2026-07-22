//! Visual system — dark mission-control palette for Grok Build Booster.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

use crate::state::TopicCategory;

/// Brand / structural colors (xAI-adjacent: void black + plasma cyan + warm white).
pub mod c {
    use ratatui::style::Color;

    pub const VOID: Color = Color::Black;
    pub const BORDER: Color = Color::Rgb(42, 48, 58);
    pub const BORDER_FOCUS: Color = Color::Rgb(0, 212, 255); // plasma cyan
    pub const BORDER_MUTED: Color = Color::Rgb(32, 36, 44);
    pub const TEXT: Color = Color::Rgb(230, 232, 238);
    pub const TEXT_DIM: Color = Color::Rgb(120, 128, 140);
    pub const TEXT_MUTED: Color = Color::Rgb(80, 88, 100);
    pub const ACCENT: Color = Color::Rgb(0, 212, 255);
    pub const ACCENT_SOFT: Color = Color::Rgb(80, 180, 220);
    pub const GOLD: Color = Color::Rgb(255, 196, 72);
    pub const SUCCESS: Color = Color::Rgb(52, 211, 153);
    pub const WARN: Color = Color::Rgb(251, 191, 36);
    pub const DANGER: Color = Color::Rgb(248, 113, 113);
    pub const MAGENTA: Color = Color::Rgb(232, 121, 249);
    pub const PURPLE: Color = Color::Rgb(167, 139, 250);
    pub const ORANGE: Color = Color::Rgb(251, 146, 60);
    pub const SELECT_BG: Color = Color::Rgb(18, 32, 48);
    pub const GAUGE_TRACK: Color = Color::Rgb(28, 32, 40);
}

pub fn style_text() -> Style {
    Style::default().fg(c::TEXT)
}

pub fn style_dim() -> Style {
    Style::default().fg(c::TEXT_DIM)
}

pub fn style_muted() -> Style {
    Style::default().fg(c::TEXT_MUTED)
}

pub fn style_accent() -> Style {
    Style::default().fg(c::ACCENT)
}

pub fn style_accent_bold() -> Style {
    Style::default()
        .fg(c::ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn style_selected() -> Style {
    Style::default()
        .fg(c::TEXT)
        .bg(c::SELECT_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn panel(title: impl Into<String>, focused: bool) -> Block<'static> {
    let border = if focused { c::BORDER_FOCUS } else { c::BORDER };
    let title_style = if focused {
        style_accent_bold()
    } else {
        Style::default().fg(c::TEXT_DIM)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Line::from(Span::styled(format!(" {} ", title.into()), title_style)))
}

pub fn panel_alert(title: impl Into<String>, alert: AlertKind) -> Block<'static> {
    let (border, ts) = match alert {
        AlertKind::Danger => (c::DANGER, Style::default().fg(c::DANGER).add_modifier(Modifier::BOLD)),
        AlertKind::Warn => (c::WARN, Style::default().fg(c::WARN).add_modifier(Modifier::BOLD)),
        AlertKind::Ok => (c::BORDER, Style::default().fg(c::TEXT_DIM)),
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Line::from(Span::styled(format!(" {} ", title.into()), ts)))
}

#[derive(Clone, Copy)]
pub enum AlertKind {
    Ok,
    Warn,
    Danger,
}

pub fn category_color(cat: TopicCategory) -> Color {
    match cat {
        TopicCategory::Security => c::DANGER,
        TopicCategory::Database => c::GOLD,
        TopicCategory::Ui => c::MAGENTA,
        TopicCategory::Tests => c::SUCCESS,
        TopicCategory::Api => c::ACCENT,
        TopicCategory::Config => c::PURPLE,
        TopicCategory::Refactor => c::ORANGE,
        TopicCategory::Devops => c::ACCENT_SOFT,
        TopicCategory::Docs => c::TEXT_DIM,
        TopicCategory::Other => c::TEXT,
    }
}

pub fn category_badge(cat: TopicCategory) -> String {
    match cat {
        TopicCategory::Security => "SEC",
        TopicCategory::Database => "DB ",
        TopicCategory::Ui => "UI ",
        TopicCategory::Tests => "TST",
        TopicCategory::Api => "API",
        TopicCategory::Config => "CFG",
        TopicCategory::Refactor => "REF",
        TopicCategory::Devops => "OPS",
        TopicCategory::Docs => "DOC",
        TopicCategory::Other => " · ",
    }
    .to_string()
}

pub fn fuel_color(ratio: f64, hard: bool) -> Color {
    if hard || ratio >= 0.92 {
        c::DANGER
    } else if ratio >= 0.75 {
        c::WARN
    } else if ratio >= 0.45 {
        c::ACCENT
    } else {
        c::SUCCESS
    }
}

pub fn spinner_frame(tick: u64) -> char {
    const FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    FRAMES[(tick as usize) % FRAMES.len()]
}

pub fn format_num(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Horizontal separator line of `width` using a dim unicode rule.
pub fn rule_line(width: u16) -> Line<'static> {
    let w = width.saturating_sub(2) as usize;
    let s: String = std::iter::repeat('─').take(w.max(1)).collect();
    Line::from(Span::styled(s, style_muted()))
}

pub fn key_hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(" {key} "),
            Style::default()
                .fg(c::VOID)
                .bg(c::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}  "), style_dim()),
    ]
}
