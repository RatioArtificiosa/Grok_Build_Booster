use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Padding, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::{self, c, category_badge, category_color, panel};
use crate::state::AppState;

pub fn draw_bookmark_list(f: &mut Frame, area: Rect, state: &AppState) {
    let title = format!("TIMELINE  ·  {}", state.bookmarks.len());
    let block = panel(title, true).padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = if state.bookmarks.is_empty() {
        vec![
            ListItem::new(Line::from("")),
            ListItem::new(Line::from(Span::styled(
                "  No turns yet",
                theme::style_dim(),
            ))),
            ListItem::new(Line::from(Span::styled(
                "  Launch grok in another pane.",
                theme::style_muted(),
            ))),
            ListItem::new(Line::from(Span::styled(
                "  Prompts stream in live.",
                theme::style_muted(),
            ))),
        ]
    } else {
        state
            .bookmarks
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let selected = i == state.selected;
                let badge = category_badge(b.category);
                let badge_color = category_color(b.category);
                let enrich = if b.remote_enriched { "✦" } else { " " };
                let marker = if selected { "▶" } else { " " };

                let row_style = if selected {
                    theme::style_selected()
                } else {
                    Style::default().fg(c::TEXT_DIM)
                };

                let id_str = format!("{:02}", b.id.min(99));
                let desc = truncate(&b.short_desc, 22);

                let line = Line::from(vec![
                    Span::styled(format!("{marker} "), Style::default().fg(c::ACCENT)),
                    Span::styled(
                        format!("{id_str} "),
                        if selected {
                            Style::default().fg(c::TEXT_MUTED)
                        } else {
                            theme::style_muted()
                        },
                    ),
                    Span::styled(
                        format!("{badge} "),
                        Style::default()
                            .fg(badge_color)
                            .add_modifier(if selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Span::styled(format!("{enrich} "), Style::default().fg(c::GOLD)),
                    Span::styled(desc, row_style),
                ]);

                if selected {
                    ListItem::new(line).style(Style::default().bg(c::SELECT_BG))
                } else {
                    ListItem::new(line)
                }
            })
            .collect()
    };

    let list = List::new(items);
    f.render_widget(list, inner);
}

pub fn draw_detail(f: &mut Frame, area: Rect, state: &AppState) {
    let block = panel("INSPECT", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(b) = state.selected_bookmark() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // meta chips
                Constraint::Length(1), // rule
                Constraint::Min(3),    // prompt
                Constraint::Length(1), // rule
                Constraint::Length(6), // files
            ])
            .split(inner);

        // Meta header
        let cat_c = category_color(b.category);
        let hash = b
            .git_commit_hash
            .as_deref()
            .map(|h| {
                if h.len() > 10 {
                    h[..10].to_string()
                } else {
                    h.to_string()
                }
            })
            .unwrap_or_else(|| "—".into());

        let meta = vec![
            Line::from(vec![
                Span::styled(" #", theme::style_muted()),
                Span::styled(
                    format!("{}", b.id),
                    Style::default()
                        .fg(c::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  turn ", theme::style_muted()),
                Span::styled(
                    format!("{}", b.llm_message_index),
                    theme::style_text(),
                ),
                Span::raw("  "),
                Span::styled(
                    format!(" {} ", category_badge(b.category).trim()),
                    Style::default()
                        .fg(c::VOID)
                        .bg(cat_c)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                if b.remote_enriched {
                    Span::styled(
                        " SuperGrok ✦ ",
                        Style::default().fg(c::VOID).bg(c::GOLD),
                    )
                } else {
                    Span::styled(" rules ", Style::default().fg(c::TEXT_MUTED).bg(c::BORDER))
                },
            ]),
            Line::from(vec![
                Span::styled(" HEAD ", theme::style_muted()),
                Span::styled(hash, Style::default().fg(c::ACCENT_SOFT)),
                Span::styled("  ·  ", theme::style_muted()),
                Span::styled(
                    format!("{} file(s)", b.changed_files.len()),
                    theme::style_dim(),
                ),
            ]),
        ];
        f.render_widget(Paragraph::new(meta), chunks[0]);
        f.render_widget(Paragraph::new(theme::rule_line(chunks[1].width)), chunks[1]);

        // Prompt body
        let prompt_block = Block::default()
            .borders(Borders::NONE)
            .title(Line::from(Span::styled(" PROMPT ", theme::style_muted())));
        let prompt_inner = prompt_block.inner(chunks[2]);
        f.render_widget(prompt_block, chunks[2]);
        f.render_widget(
            Paragraph::new(b.full_prompt.as_str())
                .style(theme::style_text())
                .wrap(Wrap { trim: false }),
            prompt_inner,
        );

        f.render_widget(Paragraph::new(theme::rule_line(chunks[3].width)), chunks[3]);

        // Files
        let files: Vec<Line> = if b.changed_files.is_empty() {
            vec![Line::from(Span::styled(
                "  no file edits tracked yet",
                theme::style_muted(),
            ))]
        } else {
            b.changed_files
                .iter()
                .take(5)
                .map(|path| {
                    Line::from(vec![
                        Span::styled("  ▸ ", Style::default().fg(c::ACCENT)),
                        Span::styled(truncate(path, (chunks[4].width as usize).saturating_sub(6)), theme::style_dim()),
                    ])
                })
                .collect()
        };
        let files_title = if b.changed_files.len() > 5 {
            format!(" FILES (+{}) ", b.changed_files.len() - 5)
        } else {
            " FILES ".into()
        };
        let files_block = Block::default()
            .borders(Borders::NONE)
            .title(Line::from(Span::styled(files_title, theme::style_muted())));
        let files_inner = files_block.inner(chunks[4]);
        f.render_widget(files_block, chunks[4]);
        f.render_widget(Paragraph::new(files), files_inner);
    } else {
        draw_welcome(f, inner);
    }
}

fn draw_welcome(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ◆  ", theme::style_accent_bold()),
            Span::styled(
                "Mission control for Grok Build",
                Style::default()
                    .fg(c::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Dual-pane workflow",
            theme::style_dim(),
        )),
        Line::from(vec![
            Span::styled("     A  ", theme::style_accent()),
            Span::styled("grok", theme::style_text()),
            Span::styled("  — agent chat & tools", theme::style_muted()),
        ]),
        Line::from(vec![
            Span::styled("     B  ", theme::style_accent()),
            Span::styled("booster", theme::style_text()),
            Span::styled("  — this dashboard", theme::style_muted()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Bookmarks appear on every UserPromptSubmit.",
            theme::style_dim(),
        )),
        Line::from(vec![
            Span::styled("  Optional  ", theme::style_muted()),
            Span::styled("grok-build-booster login", theme::style_accent()),
            Span::styled("  → SuperGrok titles", theme::style_muted()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  R", Style::default().fg(c::VOID).bg(c::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(" rewind assist   ", theme::style_dim()),
            Span::styled(" E", Style::default().fg(c::VOID).bg(c::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(" export flight log", theme::style_dim()),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn truncate(s: &str, max: usize) -> String {
    if max <= 1 {
        return "…".into();
    }
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
