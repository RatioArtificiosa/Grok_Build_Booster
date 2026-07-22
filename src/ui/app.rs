use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::CrosstermBackend;
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::Terminal;
use std::io::{stdout, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dashboard::draw_dashboard;
use super::header::{draw_footer, draw_header};
use super::sidebar::{draw_bookmark_list, draw_detail};
use super::theme::c;
use crate::core::git;
use crate::export::export_markdown;
use crate::hooks::SharedState;
use crate::oauth::TokenStore;
use crate::state::{maybe_save, AppState};

pub async fn run_tui(state: SharedState, tokens: Arc<TokenStore>) -> Result<()> {
    {
        let mut s = state.lock().await;
        s.oauth_status = format!(
            "oauth: {}",
            crate::oauth::login_status(tokens.as_ref()).unwrap_or_else(|_| "error".into())
        );
    }

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, state.clone()).await;

    {
        let s = state.lock().await;
        maybe_save(&s);
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: SharedState,
) -> Result<()> {
    let started = Instant::now();
    loop {
        let tick = started.elapsed().as_millis() as u64 / 80;
        {
            let s = state.lock().await;
            terminal.draw(|f| {
                // Full-frame backdrop
                f.render_widget(
                    Block::default().style(Style::default().bg(c::VOID).fg(c::TEXT)),
                    f.area(),
                );

                let root = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // header
                        Constraint::Min(8),    // main
                        Constraint::Length(2), // footer
                    ])
                    .split(f.area());

                draw_header(f, root[0], &s, tick);

                let main = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
                    .split(root[1]);

                let top = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                    .margin(0)
                    .split(main[0]);

                // slight horizontal breathing room via nested layout
                let left = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(1), Constraint::Min(10), Constraint::Length(0)])
                    .split(top[0]);
                let right = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(0), Constraint::Min(10), Constraint::Length(1)])
                    .split(top[1]);

                draw_bookmark_list(f, left[1], &s);
                draw_detail(f, right[1], &s);

                let dash = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Min(20),
                        Constraint::Length(1),
                    ])
                    .split(main[1]);
                draw_dashboard(f, dash[1], &s);

                draw_footer(f, root[2], &s);
            })?;
        }

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let mut s = state.lock().await;
                if s.show_confirm_rollback {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            s.show_confirm_rollback = false;
                            s.status_pinned = false;
                            perform_rollback_assist(&mut s);
                            maybe_save(&s);
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            s.show_confirm_rollback = false;
                            s.status_pinned = false;
                            s.set_status_force("Rollback cancelled");
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break,
                    KeyCode::Esc => break,
                    KeyCode::Down | KeyCode::Char('j') => s.select_next(),
                    KeyCode::Up | KeyCode::Char('k') => s.select_prev(),
                    KeyCode::Home | KeyCode::Char('g') => s.selected = 0,
                    KeyCode::End | KeyCode::Char('G') => {
                        s.selected = s.bookmarks.len().saturating_sub(1);
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        if s.selected_bookmark().is_some() {
                            s.show_confirm_rollback = true;
                            s.status_pinned = true;
                            s.set_status_force(
                                "Confirm rewind assist — Y snapshot + /rewind hint · N cancel",
                            );
                        } else {
                            s.set_status("No bookmark selected");
                        }
                    }
                    KeyCode::Char('e') | KeyCode::Char('E') => match export_markdown(&s, None) {
                        Ok(p) => {
                            s.last_export_path = Some(p.display().to_string());
                            s.set_status_force(format!("Flight recorder → {}", p.display()));
                        }
                        Err(err) => s.set_status_force(format!("Export failed: {err:#}")),
                    },
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        maybe_save(&s);
                        s.set_status_force("State saved");
                    }
                    _ => {}
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => state.lock().await.select_next(),
                MouseEventKind::ScrollUp => state.lock().await.select_prev(),
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
    Ok(())
}

fn perform_rollback_assist(state: &mut AppState) {
    let Some(b) = state.selected_bookmark().cloned() else {
        return;
    };

    let mut recovery = String::from("(no git repo / no commits)");
    if let Some(cwd) = state.project_cwd.as_ref() {
        let path = PathBuf::from(cwd);
        match git::create_recovery_branch(&path, &b.short_desc) {
            Ok(r) => recovery = r,
            Err(e) => recovery = format!("recovery branch failed: {e:#}"),
        }
        if let Ok(files) = git::dirty_files(&path) {
            if let Some(bm) = state.bookmarks.get_mut(state.selected) {
                if !files.is_empty() {
                    bm.changed_files = files;
                }
            }
        }
    }

    let file_n = state
        .selected_bookmark()
        .map(|b| b.changed_files.len())
        .unwrap_or(0);

    let hint = format!(
        "Rollback assist for bookmark #{} (turn {}):\n\
         1. Safety snapshot: {recovery}\n\
         2. Dirty / changed paths known: {file_n}\n\
         3. In the Grok TUI: Esc Esc (or /rewind) → pick the matching turn.\n\
         4. Grok restores file snapshots and truncates conversation memory.\n\
         Booster does NOT run git reset --hard.",
        b.id, b.llm_message_index
    );
    state.last_rollback_hint = Some(hint.clone());
    state.set_status_force(format!(
        "R assist ready → /rewind turn {} · {recovery}",
        b.llm_message_index
    ));
    tracing::info!("{hint}");
}
