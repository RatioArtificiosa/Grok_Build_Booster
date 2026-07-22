//! Flight recorder — export a Markdown timeline of the session.

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::booster_data_dir;
use crate::state::AppState;

pub fn default_export_path() -> Result<PathBuf> {
    let dir = booster_data_dir()?.join("exports");
    fs::create_dir_all(&dir)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    Ok(dir.join(format!("flight-recorder-{stamp}.md")))
}

pub fn export_markdown(state: &AppState, path: Option<&Path>) -> Result<PathBuf> {
    let out = match path {
        Some(p) => p.to_path_buf(),
        None => default_export_path()?,
    };
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }

    let md = render_markdown(state);
    fs::write(&out, md).with_context(|| format!("write {}", out.display()))?;
    Ok(out)
}

pub fn render_markdown(state: &AppState) -> String {
    let t = &state.telemetry;
    let mut md = String::new();
    md.push_str("# Grok Build Booster — Flight Recorder\n\n");
    md.push_str(&format!(
        "- Generated: {}\n",
        Utc::now().to_rfc3339()
    ));
    md.push_str(&format!(
        "- Project: {}\n",
        state.project_cwd.as_deref().unwrap_or("(unknown)")
    ));
    md.push_str(&format!(
        "- Model: {}\n",
        t.model_id.as_deref().unwrap_or("(unknown)")
    ));
    md.push_str(&format!(
        "- Context: {} / {} ({:.0}%)\n",
        t.context_used,
        t.context_limit,
        t.fuel_ratio() * 100.0
    ));
    md.push_str(&format!(
        "- Tokens lifetime: {} · trip: {} · session cost est: ${:.4} · trip cost: ${:.4}\n",
        t.total_tokens_lifetime, t.trip_tokens, t.session_cost_usd, t.trip_cost_usd
    ));
    md.push_str(&format!(
        "- Tools ok/fail: {}/{} · temp: {:.0}° · turns: {} · compactions: {}\n",
        t.tool_ok, t.tool_fail, t.error_temperature, t.turn_count, t.compaction_count
    ));
    md.push_str(&format!(
        "- Signals source: {}\n\n",
        if t.signals_source {
            "session signals.json"
        } else {
            "hooks/estimates only"
        }
    ));

    md.push_str("## Bookmarks\n\n");
    if state.bookmarks.is_empty() {
        md.push_str("_No bookmarks recorded._\n");
        return md;
    }

    for b in &state.bookmarks {
        let ts = Utc
            .timestamp_opt(b.timestamp as i64, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| b.timestamp.to_string());
        md.push_str(&format!(
            "### #{} — {} `{}`\n\n",
            b.id,
            b.short_desc,
            b.category.label()
        ));
        md.push_str(&format!(
            "- Time: {ts}\n- Turn index: {}\n- Enrich: {}\n",
            b.llm_message_index,
            if b.remote_enriched {
                "SuperGrok OAuth"
            } else {
                "rules"
            }
        ));
        if let Some(h) = &b.git_commit_hash {
            md.push_str(&format!("- Git HEAD: `{h}`\n"));
        }
        if let Some(sid) = &b.session_id {
            md.push_str(&format!("- Session: `{sid}`\n"));
        }
        md.push_str("\n```\n");
        md.push_str(&b.full_prompt);
        md.push_str("\n```\n\n");
        if !b.changed_files.is_empty() {
            md.push_str("**Files (snapshot):**\n\n");
            for f in &b.changed_files {
                md.push_str(&format!("- `{f}`\n"));
            }
            md.push('\n');
        }
    }

    if let Some(hint) = &state.last_rollback_hint {
        md.push_str("## Last rollback assist\n\n```\n");
        md.push_str(hint);
        md.push_str("\n```\n");
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{TopicCategory, AppState};

    #[test]
    fn renders_bookmark() {
        let mut s = AppState::default();
        s.add_bookmark(
            "fix auth".into(),
            "Fix Auth Flow".into(),
            TopicCategory::Security,
            false,
            None,
            None,
        );
        let md = render_markdown(&s);
        assert!(md.contains("Fix Auth Flow"));
        assert!(md.contains("security"));
    }
}
