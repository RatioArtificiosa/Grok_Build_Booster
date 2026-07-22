//! Persist bookmarks + telemetry counters across Booster restarts.
//!
//! Writes are serialized via a process-wide mutex and use temp-file + rename
//! to reduce the chance of a torn state.json.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{AppState, Bookmark};
use crate::config::booster_data_dir;

static SAVE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize)]
struct PersistedState {
    version: u32,
    bookmarks: Vec<Bookmark>,
    selected: usize,
    project_cwd: Option<String>,
    next_id: usize,
    total_tokens_lifetime: u64,
    tool_ok: u64,
    tool_fail: u64,
    stop_failures: u64,
    context_limit: u32,
    context_used: u32,
    price_per_mtok_usd: f64,
    session_cost_usd: f64,
    budget_hard_hit: bool,
}

impl PersistedState {
    fn from_app(state: &AppState) -> Self {
        Self {
            version: 1,
            bookmarks: state.bookmarks.iter().cloned().collect(),
            selected: state.selected,
            project_cwd: state.project_cwd.clone(),
            next_id: state.peek_next_id(),
            total_tokens_lifetime: state.telemetry.total_tokens_lifetime,
            tool_ok: state.telemetry.tool_ok,
            tool_fail: state.telemetry.tool_fail,
            stop_failures: state.telemetry.stop_failures,
            context_limit: state.telemetry.context_limit.max(1),
            context_used: state.telemetry.context_used,
            price_per_mtok_usd: state.telemetry.price_per_mtok_usd,
            session_cost_usd: state.telemetry.session_cost_usd,
            budget_hard_hit: state.telemetry.budget_hard_hit,
        }
    }

    fn apply_to(self, state: &mut AppState) {
        state.bookmarks = VecDeque::from(self.bookmarks);
        state.clamp_selection();
        if self.selected < state.bookmarks.len() {
            state.selected = self.selected;
        }
        if state.project_cwd.is_none() {
            state.project_cwd = self.project_cwd;
        }
        // next_id must be > any existing bookmark id
        let max_id = state.bookmarks.iter().map(|b| b.id).max().unwrap_or(0);
        state.set_next_id(self.next_id.max(max_id.saturating_add(1)).max(1));

        state.telemetry.total_tokens_lifetime = self.total_tokens_lifetime;
        state.telemetry.tool_ok = self.tool_ok;
        state.telemetry.tool_fail = self.tool_fail;
        state.telemetry.stop_failures = self.stop_failures;
        state.telemetry.context_limit = self.context_limit.max(1);
        state.telemetry.context_used = self.context_used.min(state.telemetry.context_limit);
        state.telemetry.price_per_mtok_usd = self.price_per_mtok_usd;
        state.telemetry.session_cost_usd = self.session_cost_usd;
        state.telemetry.budget_hard_hit = self.budget_hard_hit;
        state.telemetry.recompute_temperature_pub();
    }
}

pub fn state_path() -> Result<PathBuf> {
    Ok(booster_data_dir()?.join("state.json"))
}

pub fn save_state(state: &AppState) -> Result<()> {
    let _guard = SAVE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let path = state_path()?;
    let payload = PersistedState::from_app(state);
    let raw = serde_json::to_string_pretty(&payload)?;

    // Unique temp name avoids collisions if two processes race
    let tmp = path.with_extension(format!(
        "json.tmp.{}",
        std::process::id()
    ));
    fs::write(&tmp, raw.as_bytes()).context("write state tmp")?;

    // Atomic replace when the FS supports it
    if let Err(e) = fs::rename(&tmp, &path) {
        // Windows: destination exists — try remove then rename
        let _ = fs::remove_file(&path);
        if let Err(e2) = fs::rename(&tmp, &path) {
            // Last resort: copy
            fs::copy(&tmp, &path).with_context(|| {
                format!("persist rename failed ({e}; {e2}); copy also failed")
            })?;
            let _ = fs::remove_file(&tmp);
        }
    }
    Ok(())
}

pub fn load_state(state: &mut AppState) -> Result<bool> {
    let path = state_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let raw = fs::read_to_string(&path).context("read state.json")?;
    if raw.trim().is_empty() {
        return Ok(false);
    }
    let persisted: PersistedState =
        serde_json::from_str(&raw).context("parse state.json (corrupt?)")?;
    let n = persisted.bookmarks.len();
    persisted.apply_to(state);
    state.set_status_force(format!("Restored {n} bookmark(s) from disk"));
    Ok(true)
}

/// Periodic / event-driven save helper (never panics).
pub fn maybe_save(state: &AppState) {
    if let Err(e) = save_state(state) {
        tracing::warn!("persist failed: {e:#}");
    }
}
