//! Remote enrichment via SuperGrok OAuth → cli-chat-proxy.grok.com (never api.x.ai).

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::oauth::{
    ensure_fresh_access_token, TokenStore, CLI_AUTHENTICATE_RESPONSE, CLI_CHAT_PROXY_BASE,
    CLI_CLIENT_IDENTIFIER, CLI_CLIENT_MODE, CLI_CLIENT_VERSION, CLI_TOKEN_AUTH, CLI_USER_AGENT,
    DEFAULT_ENRICH_MODEL,
};
use crate::state::TopicCategory;

use super::rules::categorize_rules;

#[derive(Debug, Clone)]
pub struct Enrichment {
    pub short_desc: String,
    pub category: TopicCategory,
}

/// Ask remote Grok for a 3-word summary + category. Uses subscription quota path.
pub async fn enrich_with_remote_grok(prompt: &str, store: &TokenStore) -> Result<Enrichment> {
    let access = ensure_fresh_access_token(store).await?;
    let conv_id = Uuid::new_v4().to_string();

    let system = "You label coding-agent user prompts. Reply with ONLY one JSON object, no markdown:\n\
{\"summary\":\"exactly three words\",\"category\":\"security|database|ui|tests|api|config|refactor|devops|docs|other\"}\n\
summary must be exactly three English words, Title Case preferred. category must be one of the enum values.";

    let user = format!(
        "Prompt to label (truncate if huge):\n{}",
        truncate(prompt, 2000)
    );

    let body = json!({
        "model": DEFAULT_ENRICH_MODEL,
        "input": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "temperature": 0.2,
        "store": false
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .user_agent(CLI_USER_AGENT)
        .build()?;

    let url = format!("{CLI_CHAT_PROXY_BASE}/responses");
    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {access}"))
        .header("Content-Type", "application/json")
        .header("x-grok-client-identifier", CLI_CLIENT_IDENTIFIER)
        .header("x-grok-client-version", CLI_CLIENT_VERSION)
        .header("x-grok-client-mode", CLI_CLIENT_MODE)
        .header("x-xai-token-auth", CLI_TOKEN_AUTH)
        .header("x-authenticateresponse", CLI_AUTHENTICATE_RESPONSE)
        .header("x-grok-model-override", DEFAULT_ENRICH_MODEL)
        .header("x-grok-conv-id", &conv_id)
        .header("x-grok-source", "grok-build-booster")
        .json(&body)
        .send()
        .await
        .context("CLI proxy /responses request failed")?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();

    if status.as_u16() == 402 {
        bail!(
            "HTTP 402 from CLI proxy — if you pointed at api.x.ai by mistake, switch to cli-chat-proxy.grok.com. Body: {}",
            truncate(&text, 200)
        );
    }

    // Fallback to chat/completions on same host if responses is unavailable
    let text = if status.as_u16() == 400
        || status.as_u16() == 404
        || status.as_u16() == 405
        || !status.is_success()
    {
        tracing::debug!("/responses status {status}, trying /chat/completions");
        let chat_body = json!({
            "model": DEFAULT_ENRICH_MODEL,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": 0.2
        });
        let res2 = client
            .post(format!("{CLI_CHAT_PROXY_BASE}/chat/completions"))
            .header("Authorization", format!("Bearer {access}"))
            .header("Content-Type", "application/json")
            .header("x-grok-client-identifier", CLI_CLIENT_IDENTIFIER)
            .header("x-grok-client-version", CLI_CLIENT_VERSION)
            .header("x-grok-client-mode", CLI_CLIENT_MODE)
            .header("x-xai-token-auth", CLI_TOKEN_AUTH)
            .header("x-authenticateresponse", CLI_AUTHENTICATE_RESPONSE)
            .header("x-grok-model-override", DEFAULT_ENRICH_MODEL)
            .header("x-grok-conv-id", &conv_id)
            .header("x-grok-source", "grok-build-booster")
            .json(&chat_body)
            .send()
            .await
            .context("CLI proxy /chat/completions failed")?;
        let st2 = res2.status();
        let t2 = res2.text().await.unwrap_or_default();
        if !st2.is_success() {
            bail!("enrich HTTP {st2}: {}", truncate(&t2, 300));
        }
        t2
    } else {
        text
    };

    let v: Value = serde_json::from_str(&text).context("enrich response not JSON")?;
    let content = extract_output_text(&v).context("could not parse model output text")?;
    parse_enrichment_json(&content, prompt)
}

fn extract_output_text(v: &Value) -> Option<String> {
    if let Some(s) = v.get("output_text").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    if let Some(arr) = v.get("output").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for part in content {
                    if let Some(t) = part
                        .get("text")
                        .or_else(|| part.get("output_text"))
                        .and_then(|x| x.as_str())
                    {
                        return Some(t.to_string());
                    }
                }
            }
            if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                return Some(t.to_string());
            }
        }
    }
    // Chat completions shape
    v.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

fn parse_enrichment_json(content: &str, original_prompt: &str) -> Result<Enrichment> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        bail!("empty model output");
    }
    // Strip ```json fences if present; extract first {...} object
    let json_str = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if end >= start {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    let v: Value = serde_json::from_str(json_str).with_context(|| {
        format!(
            "model did not return JSON: {}",
            truncate(trimmed, 120)
        )
    })?;

    let summary = v
        .get("summary")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");

    let cat_raw = v
        .get("category")
        .and_then(|x| x.as_str())
        .unwrap_or("other")
        .to_lowercase();

    let category = TopicCategory::from_label(&cat_raw).unwrap_or_else(|| categorize_rules(original_prompt));

    let short_desc = if summary.is_empty() {
        super::rules::three_word_summary_rules(original_prompt)
    } else {
        summary
    };

    Ok(Enrichment {
        short_desc,
        category,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
