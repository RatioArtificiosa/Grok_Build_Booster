use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::Telemetry;

/// Cap stored bookmarks so state.json cannot grow without bound.
pub const MAX_BOOKMARKS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicCategory {
    Security,
    Database,
    Ui,
    Tests,
    Api,
    Config,
    Refactor,
    Devops,
    Docs,
    Other,
}

impl TopicCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Security => "security",
            Self::Database => "database",
            Self::Ui => "ui",
            Self::Tests => "tests",
            Self::Api => "api",
            Self::Config => "config",
            Self::Refactor => "refactor",
            Self::Devops => "devops",
            Self::Docs => "docs",
            Self::Other => "other",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "security" | "sec" => Some(Self::Security),
            "database" | "db" => Some(Self::Database),
            "ui" | "frontend" => Some(Self::Ui),
            "tests" | "test" => Some(Self::Tests),
            "api" => Some(Self::Api),
            "config" => Some(Self::Config),
            "refactor" => Some(Self::Refactor),
            "devops" | "ops" | "infra" => Some(Self::Devops),
            "docs" | "documentation" => Some(Self::Docs),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: usize,
    pub short_desc: String,
    pub full_prompt: String,
    pub category: TopicCategory,
    pub git_commit_hash: Option<String>,
    pub llm_message_index: usize,
    pub timestamp: u64,
    pub remote_enriched: bool,
    pub session_id: Option<String>,
    pub changed_files: Vec<String>,
}

#[derive(Debug)]
pub struct AppState {
    pub bookmarks: VecDeque<Bookmark>,
    pub selected: usize,
    pub telemetry: Telemetry,
    pub status_line: String,
    pub oauth_status: String,
    pub project_cwd: Option<String>,
    pub show_confirm_rollback: bool,
    pub last_rollback_hint: Option<String>,
    pub last_export_path: Option<String>,
    pub session_dir: Option<String>,
    /// When true, background watchers must not clobber status_line.
    pub status_pinned: bool,
    /// Live hook activity counters (not restored from disk — only this process).
    pub hook_events_total: u64,
    pub last_hook_event: Option<String>,
    pub last_hook_at: Option<std::time::Instant>,
    next_id: AtomicUsize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            bookmarks: VecDeque::new(),
            selected: 0,
            telemetry: Telemetry::default(),
            status_line: "Waiting for Grok hooks…  (in Grok: /hooks → press r to reload)".into(),
            oauth_status: "oauth: unknown".into(),
            project_cwd: None,
            show_confirm_rollback: false,
            last_rollback_hint: None,
            last_export_path: None,
            session_dir: None,
            status_pinned: false,
            hook_events_total: 0,
            last_hook_event: None,
            last_hook_at: None,
            next_id: AtomicUsize::new(1),
        }
    }
}

impl AppState {
    pub fn peek_next_id(&self) -> usize {
        self.next_id.load(Ordering::SeqCst)
    }

    pub fn set_next_id(&self, v: usize) {
        self.next_id.store(v.max(1), Ordering::SeqCst);
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        if !self.status_pinned && !self.show_confirm_rollback {
            self.status_line = msg.into();
        }
    }

    pub fn set_status_force(&mut self, msg: impl Into<String>) {
        self.status_line = msg.into();
    }

    pub fn note_hook(&mut self, event: &str) {
        self.hook_events_total = self.hook_events_total.saturating_add(1);
        self.last_hook_event = Some(event.to_string());
        self.last_hook_at = Some(std::time::Instant::now());
    }

    pub fn hooks_alive_label(&self) -> String {
        match (self.hook_events_total, self.last_hook_at, self.last_hook_event.as_deref()) {
            (0, _, _) => "hooks: waiting (reload in Grok: /hooks → r)".into(),
            (n, Some(at), Some(ev)) => {
                let secs = at.elapsed().as_secs();
                format!("hooks: {n} events · last {ev} · {secs}s ago")
            }
            (n, _, _) => format!("hooks: {n} events"),
        }
    }

    pub fn add_bookmark(
        &mut self,
        full_prompt: String,
        short_desc: String,
        category: TopicCategory,
        remote_enriched: bool,
        git_hash: Option<String>,
        session_id: Option<String>,
    ) -> usize {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let llm_message_index = self.bookmarks.len();

        // Cap prompt storage
        let full_prompt = if full_prompt.len() > 32_768 {
            let mut t = full_prompt.chars().take(32_000).collect::<String>();
            t.push_str("…[truncated]");
            t
        } else {
            full_prompt
        };

        self.bookmarks.push_back(Bookmark {
            id,
            short_desc: short_desc.chars().take(80).collect(),
            full_prompt,
            category,
            git_commit_hash: git_hash,
            llm_message_index,
            timestamp,
            remote_enriched,
            session_id,
            changed_files: Vec::new(),
        });

        while self.bookmarks.len() > MAX_BOOKMARKS {
            self.bookmarks.pop_front();
            if self.selected > 0 {
                self.selected -= 1;
            }
        }

        self.selected = self.bookmarks.len().saturating_sub(1);
        self.telemetry.on_new_prompt();
        id
    }

    /// Update enrichment fields on a bookmark by id (async enrich completion).
    pub fn apply_enrichment(
        &mut self,
        id: usize,
        short_desc: String,
        category: TopicCategory,
        remote: bool,
    ) -> bool {
        if let Some(bm) = self.bookmarks.iter_mut().find(|b| b.id == id) {
            bm.short_desc = short_desc.chars().take(80).collect();
            bm.category = category;
            bm.remote_enriched = remote;
            true
        } else {
            false
        }
    }

    pub fn selected_bookmark(&self) -> Option<&Bookmark> {
        self.bookmarks.get(self.selected)
    }

    pub fn select_next(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.bookmarks.len() - 1);
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn clamp_selection(&mut self) {
        if self.bookmarks.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.bookmarks.len() - 1);
        }
    }
}
