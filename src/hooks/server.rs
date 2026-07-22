use anyhow::Result;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::BoosterConfig;
use crate::core::git;
use crate::enrich::{categorize_rules, enrich_with_remote_grok, three_word_summary_rules};
use crate::oauth::TokenStore;
use crate::state::{maybe_save, AppState};

pub type SharedState = Arc<Mutex<AppState>>;
pub type SharedConfig = Arc<Mutex<BoosterConfig>>;

#[derive(Clone)]
struct HookCtx {
    state: SharedState,
    tokens: Arc<TokenStore>,
    config: SharedConfig,
}

pub async fn spawn_hook_server(
    port: u16,
    state: SharedState,
    tokens: Arc<TokenStore>,
    config: SharedConfig,
) -> Result<()> {
    let ctx = HookCtx {
        state,
        tokens,
        config,
    };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/hooks", post(handle_hook))
        .with_state(ctx);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Booster hook server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_hook(
    State(ctx): State<HookCtx>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let body = match body {
        Ok(Json(v)) => v,
        Err(e) => {
            tracing::warn!("invalid hook JSON: {e}");
            return (StatusCode::OK, Json(json!({})));
        }
    };

    let event = body
        .get("hookEventName")
        .or_else(|| body.get("hook_event_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase()
        .replace('-', "_");

    tracing::debug!(%event, "hook received");
    {
        let mut s = ctx.state.lock().await;
        s.note_hook(&event);
    }
    absorb_usage_fields(&ctx, &body).await;

    let response = match event.as_str() {
        "user_prompt_submit" | "userpromptsubmit" | "beforesubmitprompt" => {
            on_user_prompt(&ctx, &body).await;
            json!({})
        }
        "subagent_start" | "subagentstart" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.subagent_start();
            let rpm = s.telemetry.active_subagents;
            s.set_status(format!("Subagent start (rpm={rpm})"));
            json!({})
        }
        "subagent_stop" | "subagentstop" | "subagent_end" | "subagentend" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.subagent_stop();
            let rpm = s.telemetry.active_subagents;
            s.set_status(format!("Subagent stop (rpm={rpm})"));
            json!({})
        }
        "post_tool_use" | "posttooluse" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.tool_success();
            if !s.telemetry.signals_source {
                s.telemetry.record_tokens(400);
            }
            if let Some(tool) = body
                .get("toolName")
                .or_else(|| body.get("tool_name"))
                .and_then(|v| v.as_str())
            {
                s.set_status(format!("Tool ok: {tool}"));
                track_file_edit(&mut s, tool, &body);
            }
            apply_budget_and_context_warn(&ctx, &mut s).await;
            json!({})
        }
        "post_tool_use_failure" | "posttoolusefailure" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.tool_failure();
            s.set_status("Tool failure");
            json!({})
        }
        "stop_failure" | "stopfailure" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.stop_failure();
            s.set_status("StopFailure");
            json!({})
        }
        "stop" => on_stop(&ctx).await,
        "session_start" | "sessionstart" => {
            let mut s = ctx.state.lock().await;
            if let Some(cwd) = body
                .get("cwd")
                .or_else(|| body.get("workspaceRoot"))
                .or_else(|| body.get("workspace_root"))
                .and_then(|v| v.as_str())
            {
                s.project_cwd = Some(cwd.to_string());
            }
            s.set_status("Session started");
            maybe_save(&s);
            json!({})
        }
        "session_end" | "sessionend" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.active_subagents = 0;
            s.set_status("Session ended");
            maybe_save(&s);
            json!({})
        }
        "pre_compact" | "precompact" => {
            let mut s = ctx.state.lock().await;
            s.set_status("Compaction starting…");
            json!({})
        }
        "post_compact" | "postcompact" => {
            let mut s = ctx.state.lock().await;
            s.telemetry.compaction_count = s.telemetry.compaction_count.saturating_add(1);
            s.set_status("Compaction complete");
            json!({})
        }
        "pre_tool_use" | "pretooluse" | "permission_denied" | "permissiondenied" | "notification" => {
            json!({})
        }
        other => {
            tracing::trace!(event = other, "unhandled hook event");
            json!({})
        }
    };

    (StatusCode::OK, Json(response))
}

fn track_file_edit(s: &mut AppState, tool: &str, body: &Value) {
    let tool_l = tool.to_ascii_lowercase();
    let is_edit = matches!(
        tool_l.as_str(),
        "search_replace"
            | "write"
            | "edit"
            | "multiedit"
            | "apply_patch"
            | "str_replace"
            | "create_file"
    );
    if !is_edit {
        return;
    }
    let Some(path) = body
        .pointer("/toolInput/path")
        .or_else(|| body.pointer("/toolInput/file_path"))
        .or_else(|| body.pointer("/toolInput/target_file"))
        .or_else(|| body.pointer("/tool_input/path"))
        .or_else(|| body.pointer("/tool_input/file_path"))
        .or_else(|| body.pointer("/tool_input/target_file"))
        .and_then(|v| v.as_str())
    else {
        return;
    };
    if let Some(bm) = s.bookmarks.back_mut() {
        if bm.changed_files.len() < 200 && !bm.changed_files.iter().any(|f| f == path) {
            bm.changed_files.push(path.to_string());
        }
    }
}

async fn on_stop(ctx: &HookCtx) -> Value {
    // Lock order: state, then config (never reverse)
    let mut s = ctx.state.lock().await;
    if !s.telemetry.signals_source {
        let next = s.telemetry.context_used.saturating_add(1_500);
        s.telemetry.set_context_used(next);
    }
    apply_budget_locked(ctx, &mut s).await;

    // Soft budget must NOT return hookSpecificOutput — that forces another agent
    // round. Soft = Booster UI only. Hard = continue:false.
    let session_cost = s.telemetry.session_cost_usd;
    if s.telemetry.budget_hard_hit {
        s.set_status_force(format!("BUDGET HARD STOP — session ${session_cost:.4}"));
        maybe_save(&s);
        return json!({
            "continue": false,
            "stopReason": format!(
                "Grok Build Booster: estimated session budget exhausted (${session_cost:.4}). \
                 Raise hard_budget_usd in ~/.grok/booster/config.json, or /new and continue carefully."
            )
        });
    }

    if s.telemetry.budget_soft_hit {
        s.set_status(format!(
            "Budget soft warn — ${session_cost:.4} (session estimate)"
        ));
    } else {
        s.set_status("Turn complete");
    }
    maybe_save(&s);
    json!({})
}

async fn absorb_usage_fields(ctx: &HookCtx, body: &Value) {
    let input = body
        .pointer("/usage/input_tokens")
        .or_else(|| body.pointer("/usage/prompt_tokens"))
        .or_else(|| body.pointer("/tokenUsage/input"))
        .and_then(|v| v.as_u64());
    let output = body
        .pointer("/usage/output_tokens")
        .or_else(|| body.pointer("/usage/completion_tokens"))
        .or_else(|| body.pointer("/tokenUsage/output"))
        .and_then(|v| v.as_u64());
    if input.is_none() && output.is_none() {
        return;
    }
    let n = input.unwrap_or(0).saturating_add(output.unwrap_or(0));
    if n == 0 || n > 50_000_000 {
        return;
    }
    let mut s = ctx.state.lock().await;
    s.telemetry
        .record_tokens(n.min(u64::from(u32::MAX)) as u32);
    apply_budget_locked(ctx, &mut s).await;
}

async fn apply_budget_and_context_warn(ctx: &HookCtx, s: &mut AppState) {
    let soft_ratio = {
        apply_budget_locked(ctx, s).await;
        ctx.config.lock().await.soft_context_ratio
    };
    if s.telemetry.fuel_ratio() >= soft_ratio {
        s.set_status(format!(
            "Context soft warn {:.0}% — consider /compact",
            s.telemetry.fuel_ratio() * 100.0
        ));
    }
}

async fn apply_budget_locked(ctx: &HookCtx, s: &mut AppState) {
    let cfg = ctx.config.lock().await.clone();
    s.telemetry.price_per_mtok_usd = cfg.price_per_mtok_usd;
    s.telemetry
        .evaluate_budget(cfg.soft_budget_usd, cfg.hard_budget_usd);
}

/// Rules bookmark immediately so hook-forward returns inside Grok timeouts.
/// SuperGrok enrich runs in a background task and patches the bookmark by id.
fn extract_prompt(body: &Value) -> String {
    const KEYS: &[&str] = &["prompt", "userPrompt", "user_prompt", "message", "text"];
    for k in KEYS {
        if let Some(s) = body.get(*k).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    // Nested payload shapes (defensive)
    for path in ["/payload/prompt", "/data/prompt", "/input/prompt"] {
        if let Some(s) = body.pointer(path).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    // Last resort: compact JSON so we still get a bookmark
    let compact = body.to_string();
    if compact.len() > 8 && compact != "{}" {
        format!("(hook) {}", three_word_summary_rules(&compact))
    } else {
        "(empty prompt)".into()
    }
}

async fn on_user_prompt(ctx: &HookCtx, body: &Value) {
    let prompt = extract_prompt(body);

    let session_id = body
        .get("sessionId")
        .or_else(|| body.get("session_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let cwd = {
        let s = ctx.state.lock().await;
        s.project_cwd.clone().or_else(|| {
            body.get("cwd")
                .or_else(|| body.get("workspaceRoot"))
                .or_else(|| body.get("workspace_root"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
    };

    let git_hash = cwd
        .as_ref()
        .and_then(|c| git::try_snapshot_branch(c).ok().flatten());

    let short = three_word_summary_rules(&prompt);
    let cat = categorize_rules(&prompt);

    let bookmark_id = {
        let mut s = ctx.state.lock().await;
        if let Some(ref c) = cwd {
            s.project_cwd = Some(c.clone());
        }
        let id = s.add_bookmark(prompt.clone(), short, cat, false, git_hash, session_id);
        let logged_in = ctx.tokens.load().ok().flatten().is_some();
        if logged_in {
            s.set_status("Bookmark (rules) — SuperGrok enriching…");
        } else {
            s.set_status("Bookmark (rules)");
        }
        maybe_save(&s);
        id
    };

    if ctx.tokens.load().ok().flatten().is_none() {
        return;
    }

    let tokens = ctx.tokens.clone();
    let state = ctx.state.clone();
    tokio::spawn(async move {
        match enrich_with_remote_grok(&prompt, tokens.as_ref()).await {
            Ok(e) => {
                let mut s = state.lock().await;
                if s.apply_enrichment(bookmark_id, e.short_desc, e.category, true) {
                    s.set_status(format!("Bookmark #{bookmark_id} · SuperGrok enrich ✓"));
                    maybe_save(&s);
                }
            }
            Err(err) => {
                tracing::warn!("remote enrich failed: {err:#}");
                let mut s = state.lock().await;
                s.set_status(format!("Bookmark #{bookmark_id} · enrich failed (rules kept)"));
            }
        }
    });
}
