//! Integration tests: mock hook payloads against the Booster HTTP server.

use grok_build_booster::config::BoosterConfig;
use grok_build_booster::hooks::spawn_hook_server;
use grok_build_booster::oauth::TokenStore;
use grok_build_booster::state::AppState;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

async fn spawn_test_server() -> (u16, Arc<Mutex<AppState>>) {
    let state = Arc::new(Mutex::new(AppState::default()));
    let tokens = Arc::new(TokenStore::default_paths().expect("token store paths"));
    let config = Arc::new(Mutex::new(BoosterConfig {
        hard_budget_usd: Some(0.0001),
        soft_budget_usd: Some(0.00001),
        price_per_mtok_usd: 1000.0,
        ..BoosterConfig::default()
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let st = state.clone();
    let tk = tokens;
    let cfg = config;
    tokio::spawn(async move {
        let _ = spawn_hook_server(port, st, tk, cfg).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    (port, state)
}

async fn post_hook(port: u16, body: serde_json::Value) -> (reqwest::StatusCode, String) {
    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://127.0.0.1:{port}/hooks"))
        .json(&body)
        .send()
        .await
        .expect("post");
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    (status, text)
}

#[tokio::test]
async fn user_prompt_creates_bookmark() {
    let (port, state) = spawn_test_server().await;
    let (status, body) = post_hook(
        port,
        json!({
            "hookEventName": "user_prompt_submit",
            "prompt": "add unit tests for the login form",
            "cwd": "E:/fake/proj",
            "sessionId": "sess-1"
        }),
    )
    .await;
    assert!(status.is_success());
    // Must return quickly with empty JSON (enrich is backgrounded)
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(json!({}));
    assert!(v.as_object().map(|o| o.is_empty()).unwrap_or(false) || body.trim() == "{}");

    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        if !state.lock().await.bookmarks.is_empty() {
            break;
        }
    }
    let s = state.lock().await;
    assert!(
        !s.bookmarks.is_empty(),
        "expected bookmark after UserPromptSubmit"
    );
    assert_eq!(s.project_cwd.as_deref(), Some("E:/fake/proj"));
    // Rules path: should not need OAuth
    assert!(!s.bookmarks.back().unwrap().short_desc.is_empty());
}

#[tokio::test]
async fn stop_soft_budget_does_not_force_continue() {
    // Soft budget must NOT emit hookSpecificOutput (that keeps agent working)
    let state = Arc::new(Mutex::new(AppState::default()));
    let tokens = Arc::new(TokenStore::default_paths().expect("token store paths"));
    let config = Arc::new(Mutex::new(BoosterConfig {
        hard_budget_usd: Some(1000.0),
        soft_budget_usd: Some(0.00001),
        price_per_mtok_usd: 1000.0,
        ..BoosterConfig::default()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let st = state.clone();
    tokio::spawn(async move {
        let _ = spawn_hook_server(port, st, tokens, config).await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;

    {
        let mut s = state.lock().await;
        s.telemetry.session_cost_usd = 1.0;
        s.telemetry.total_tokens_lifetime = 5_000;
    }
    let (status, body) = post_hook(port, json!({"hookEventName": "stop", "reason": "end_turn"})).await;
    assert!(status.is_success());
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        v.get("hookSpecificOutput").is_none(),
        "soft budget must not force another agent round: {body}"
    );
    assert_ne!(v.get("continue").and_then(|c| c.as_bool()), Some(false));
}

#[tokio::test]
async fn subagent_rpm_and_tool_events() {
    let (port, state) = spawn_test_server().await;
    let _ = post_hook(port, json!({"hookEventName": "subagent_start"})).await;
    let _ = post_hook(port, json!({"hookEventName": "subagent_start"})).await;
    let _ = post_hook(
        port,
        json!({
            "hookEventName": "post_tool_use",
            "toolName": "read_file"
        }),
    )
    .await;
    let _ = post_hook(port, json!({"hookEventName": "post_tool_use_failure"})).await;
    let _ = post_hook(port, json!({"hookEventName": "subagent_stop"})).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    let s = state.lock().await;
    assert_eq!(s.telemetry.active_subagents, 1);
    assert!(s.telemetry.tool_ok >= 1);
    assert!(s.telemetry.tool_fail >= 1);
}

#[tokio::test]
async fn stop_hard_budget_returns_continue_false() {
    let (port, state) = spawn_test_server().await;
    {
        let mut s = state.lock().await;
        s.telemetry.total_tokens_lifetime = 10_000;
        s.telemetry.session_cost_usd = 50.0;
        s.telemetry.budget_hard_hit = true;
    }
    let (status, body) = post_hook(
        port,
        json!({
            "hookEventName": "stop",
            "reason": "end_turn"
        }),
    )
    .await;
    assert!(status.is_success());
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v.get("continue").and_then(|c| c.as_bool()), Some(false));
    assert!(v.get("stopReason").is_some());
}

#[tokio::test]
async fn health_ok() {
    let (port, _) = spawn_test_server().await;
    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
}
