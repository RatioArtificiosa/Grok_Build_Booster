//! Discover and poll Grok session artifacts under ~/.grok/sessions.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Subset of Grok `SessionSignals` (camelCase) we care about for the dashboard.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionSignalsSnapshot {
    pub turn_count: u32,
    pub error_count: u32,
    pub tool_failure_count: u32,
    pub tool_call_count: u32,
    pub context_window_usage: u8,
    pub context_tokens_used: u64,
    pub context_window_tokens: u64,
    pub primary_model_id: Option<String>,
    pub compaction_count: u32,
    pub cancellation_count: u32,
}

impl SessionSignalsSnapshot {
    pub fn from_json_str(raw: &str) -> Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }

    pub fn from_value(v: &Value) -> Self {
        serde_json::from_value(v.clone()).unwrap_or_default()
    }
}

/// Encode cwd the same way Grok groups sessions (URL-encode path).
pub fn encode_cwd_group(cwd: &str) -> String {
    // Grok uses URL-encoding of the working directory path.
    urlencoding_lite(cwd)
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            // Windows paths: keep a readable form; Grok uses percent-encoding
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn grok_sessions_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("no home dir")?;
    // GROK_HOME override
    if let Ok(gh) = std::env::var("GROK_HOME") {
        return Ok(PathBuf::from(gh).join("sessions"));
    }
    Ok(home.join(".grok").join("sessions"))
}

/// Find the most recently modified session directory for a workspace cwd.
pub fn find_latest_session_dir(cwd: &str) -> Result<Option<PathBuf>> {
    let root = grok_sessions_root()?;
    if !root.exists() {
        return Ok(None);
    }

    let encoded = encode_cwd_group(cwd);
    let group = root.join(&encoded);

    // Also try scanning all groups if exact match missing (path encoding variance)
    let candidates: Vec<PathBuf> = if group.is_dir() {
        list_session_dirs(&group)?
    } else {
        let mut all = Vec::new();
        if let Ok(entries) = fs::read_dir(&root) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_dir() {
                    all.extend(list_session_dirs(&p)?);
                }
            }
        }
        // Prefer sessions whose summary.json mentions this cwd
        all.into_iter()
            .filter(|s| session_matches_cwd(s, cwd))
            .collect()
    };

    let mut best: Option<(SystemTime, PathBuf)> = None;
    for s in candidates {
        let mtime = fs::metadata(&s)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        // Prefer dirs with signals.json
        let score_path = if s.join("signals.json").exists() {
            s.clone()
        } else {
            s
        };
        match &best {
            None => best = Some((mtime, score_path)),
            Some((t, _)) if mtime >= *t => best = Some((mtime, score_path)),
            _ => {}
        }
    }
    Ok(best.map(|(_, p)| p))
}

fn list_session_dirs(group: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !group.is_dir() {
        return Ok(out);
    }
    for ent in fs::read_dir(group)?.flatten() {
        let p = ent.path();
        if p.is_dir()
            && (p.join("signals.json").exists()
                || p.join("summary.json").exists()
                || p.join("updates.jsonl").exists())
        {
            out.push(p);
        }
    }
    Ok(out)
}

fn session_matches_cwd(session_dir: &Path, cwd: &str) -> bool {
    let summary = session_dir.join("summary.json");
    if let Ok(raw) = fs::read_to_string(&summary) {
        if raw.contains(cwd) {
            return true;
        }
        // normalized slashes
        let norm = cwd.replace('\\', "/");
        if raw.contains(&norm) {
            return true;
        }
    }
    // Parent group name is url-encoded cwd
    if let Some(group) = session_dir.parent() {
        if let Some(name) = group.file_name().and_then(|n| n.to_str()) {
            let decoded = percent_decode_basic(name);
            let dec_norm = decoded.replace('/', "\\");
            if decoded.eq_ignore_ascii_case(cwd)
                || dec_norm.eq_ignore_ascii_case(cwd)
                || decoded.replace('\\', "/") == cwd.replace('\\', "/")
            {
                return true;
            }
        }
    }
    false
}

fn percent_decode_basic(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn read_signals(session_dir: &Path) -> Result<Option<SessionSignalsSnapshot>> {
    let path = session_dir.join("signals.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(Some(SessionSignalsSnapshot::default()));
    }
    Ok(Some(SessionSignalsSnapshot::from_json_str(&raw)?))
}

/// Apply a signals snapshot into booster telemetry fields.
pub fn apply_signals_to_telemetry(
    signals: &SessionSignalsSnapshot,
    telemetry: &mut crate::state::Telemetry,
) {
    if signals.context_window_tokens > 0 {
        telemetry.set_context_limit(
            signals.context_window_tokens.min(u64::from(u32::MAX)) as u32,
        );
    }
    if signals.context_tokens_used > 0 {
        telemetry.set_context_used(signals.context_tokens_used.min(u64::from(u32::MAX)) as u32);
    } else if signals.context_window_usage > 0 && telemetry.context_limit > 0 {
        let used = (u64::from(telemetry.context_limit) * u64::from(signals.context_window_usage)
            / 100) as u32;
        telemetry.set_context_used(used);
    }

    // Prefer real tool counters from signals over hook estimates when higher.
    if signals.tool_call_count as u64 > telemetry.tool_ok + telemetry.tool_fail {
        // Keep fail count from signals if available
        let fails = signals.tool_failure_count as u64;
        let ok = (signals.tool_call_count as u64).saturating_sub(fails);
        telemetry.tool_ok = ok;
        telemetry.tool_fail = fails;
    } else if signals.tool_failure_count as u64 > telemetry.tool_fail {
        telemetry.tool_fail = signals.tool_failure_count as u64;
    }

    if signals.error_count as u64 > telemetry.stop_failures {
        // Map general errors into temperature inputs
        telemetry.stop_failures = signals.error_count as u64;
    }

    telemetry.recompute_temperature_pub();
    telemetry.model_id = signals.primary_model_id.clone();
    telemetry.compaction_count = signals.compaction_count;
    telemetry.turn_count = signals.turn_count;
    telemetry.signals_source = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_signals_camel_case() {
        let raw = r#"{
            "turnCount": 3,
            "contextTokensUsed": 12000,
            "contextWindowTokens": 256000,
            "contextWindowUsage": 5,
            "toolCallCount": 10,
            "toolFailureCount": 1,
            "errorCount": 0,
            "primaryModelId": "grok-4.5"
        }"#;
        let s = SessionSignalsSnapshot::from_json_str(raw).unwrap();
        assert_eq!(s.turn_count, 3);
        assert_eq!(s.context_tokens_used, 12000);
        assert_eq!(s.primary_model_id.as_deref(), Some("grok-4.5"));
    }

    #[test]
    fn encode_is_stable() {
        let a = encode_cwd_group(r"E:\proj");
        assert!(a.contains("E"));
        assert!(a.contains('%') || a.contains("proj"));
    }
}
