use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use url::Url;

use super::constants::*;
use super::pkce::{generate_pkce, random_state};
use super::store::{PendingPkce, TokenBundle, TokenStore};

pub fn login_status(store: &TokenStore) -> Result<String> {
    match store.load()? {
        Some(b) if !b.needs_refresh() => Ok(format!("logged in (token {})", b.access_last4())),
        Some(b) => Ok(format!(
            "logged in, refresh needed (token {})",
            b.access_last4()
        )),
        None => Ok("not logged in".into()),
    }
}

/// Start PKCE login: open browser, listen on loopback callback, exchange code.
pub async fn begin_login(store: &TokenStore) -> Result<TokenBundle> {
    let pkce = generate_pkce();
    let state = random_state();
    let nonce = random_state();

    store.save_pending(&PendingPkce {
        verifier: pkce.verifier.clone(),
        challenge: pkce.challenge.clone(),
        state: state.clone(),
        created_at: chrono::Utc::now().timestamp_millis(),
    })?;

    let mut auth = Url::parse(AUTHORIZE_URL)?;
    {
        let mut q = auth.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", CLIENT_ID);
        q.append_pair("redirect_uri", REDIRECT_URI);
        q.append_pair("scope", SCOPES);
        q.append_pair("code_challenge", &pkce.challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("state", &state);
        q.append_pair("nonce", &nonce);
        q.append_pair("plan", "generic");
        q.append_pair("referrer", REFERRER);
    }

    let auth_url = auth.to_string();
    tracing::info!("Opening browser for SuperGrok OAuth…");
    if let Err(e) = open::that(&auth_url) {
        tracing::warn!("Could not open browser ({e}). Open this URL manually:\n{auth_url}");
        eprintln!("Open this URL to login:\n{auth_url}\n");
    }

    let code = match capture_loopback_code(&state).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Loopback capture failed ({e}). Paste the callback URL when prompted.");
            eprintln!(
                "If the browser showed a connection error, copy the full URL from the address bar\n\
                 (it still contains code=…) and paste it here, then press Enter:"
            );
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            parse_code_from_paste(line.trim(), &state)?
        }
    };

    let pending = store
        .load_pending()?
        .context("missing PKCE pending state; run login again")?;
    if pending.state != state {
        bail!("OAuth state mismatch");
    }

    let bundle = exchange_code(&code, &pending.verifier, &pending.challenge).await?;
    store.save(&bundle)?;
    store.clear_pending()?;
    Ok(bundle)
}

/// Complete login from a pasted callback URL (SPA-style fallback).
pub async fn complete_from_callback(store: &TokenStore, pasted: &str) -> Result<TokenBundle> {
    let pending = store
        .load_pending()?
        .context("no pending OAuth; run `grok-build-booster login` first")?;
    let code = parse_code_from_paste(pasted, &pending.state)?;
    let bundle = exchange_code(&code, &pending.verifier, &pending.challenge).await?;
    store.save(&bundle)?;
    store.clear_pending()?;
    Ok(bundle)
}

fn parse_code_from_paste(pasted: &str, expected_state: &str) -> Result<String> {
    let pasted = pasted.trim();
    if pasted.is_empty() {
        bail!("empty paste");
    }
    let (code, state) = if pasted.contains("code=") || pasted.starts_with("http") {
        let url = if pasted.starts_with("http") {
            Url::parse(pasted)?
        } else {
            Url::parse(&format!("http://local/?{pasted}"))?
        };
        if let Some(err) = url.query_pairs().find(|(k, _)| k == "error") {
            let desc = url
                .query_pairs()
                .find(|(k, _)| k == "error_description")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            bail!("OAuth error: {} {}", err.1, desc);
        }
        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string())
            .context("no code= in paste")?;
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.to_string());
        (code, state)
    } else {
        (pasted.to_string(), None)
    };

    if let Some(s) = state {
        if s != expected_state {
            bail!("state mismatch (possible CSRF or stale paste)");
        }
    }
    Ok(code)
}

async fn capture_loopback_code(expected_state: &str) -> Result<String> {
    let (tx, rx) = oneshot::channel::<Result<String>>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let expected = expected_state.to_string();

    // Signal to shut down the temporary server after we get a code
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown_tx = Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx)));

    let app = axum::Router::new().route(
        "/callback",
        axum::routing::get({
            let tx = tx.clone();
            let shutdown_tx = shutdown_tx.clone();
            move |axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>| {
                let tx = tx.clone();
                let shutdown_tx = shutdown_tx.clone();
                let expected = expected.clone();
                async move {
                    let outcome: Result<String> = (|| {
                        if let Some(err) = q.get("error") {
                            bail!(
                                "OAuth error: {} {}",
                                err,
                                q.get("error_description").map(|s| s.as_str()).unwrap_or("")
                            );
                        }
                        let code = q
                            .get("code")
                            .cloned()
                            .context("missing code in callback")?;
                        let state = q.get("state").cloned().unwrap_or_default();
                        if !state.is_empty() && state != expected {
                            bail!("state mismatch");
                        }
                        Ok(code)
                    })();

                    let html = if outcome.is_ok() {
                        "<!DOCTYPE html><html><head><meta charset='utf-8'><title>Booster</title></head>\
                         <body style='font-family:system-ui,sans-serif;max-width:32rem;margin:3rem auto;padding:1rem'>\
                         <h1>Grok Build Booster</h1>\
                         <p>Login complete. You can close this tab and return to the terminal.</p>\
                         </body></html>"
                    } else {
                        "<!DOCTYPE html><html><head><meta charset='utf-8'><title>Booster</title></head>\
                         <body style='font-family:system-ui,sans-serif;max-width:32rem;margin:3rem auto;padding:1rem'>\
                         <h1>Login failed</h1>\
                         <p>Return to the terminal for details.</p></body></html>"
                    };

                    if let Some(sender) = tx.lock().await.take() {
                        let _ = sender.send(outcome);
                    }
                    if let Some(sd) = shutdown_tx.lock().await.take() {
                        let _ = sd.send(());
                    }
                    axum::response::Html(html.to_string())
                }
            }
        }),
    );

    let addr = SocketAddr::from(([127, 0, 0, 1], 56121));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("bind OAuth callback on 127.0.0.1:56121 (is another app using it?)")?;

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
        // Brief delay so the HTML response can flush
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    });

    let server_handle = tokio::spawn(async move {
        let _ = server.await;
    });

    let code_result = tokio::time::timeout(std::time::Duration::from_secs(300), rx).await;

    // Ensure server task ends even on timeout
    if let Some(sd) = shutdown_tx.lock().await.take() {
        let _ = sd.send(());
    }
    let _ = server_handle.await;

    let code = code_result
        .context("timed out waiting for OAuth callback (5 min)")?
        .context("callback channel closed")??;

    Ok(code)
}

async fn exchange_code(code: &str, verifier: &str, challenge: &str) -> Result<TokenBundle> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", code);
    form.insert("redirect_uri", REDIRECT_URI);
    form.insert("client_id", CLIENT_ID);
    form.insert("code_verifier", verifier);
    form.insert("code_challenge", challenge);
    form.insert("code_challenge_method", "S256");

    let res = client
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .context("token exchange request failed")?;

    let status = res.status();
    let body: Value = res.json().await.context("token response not JSON")?;
    if !status.is_success() {
        let msg = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("token exchange failed");
        bail!("token exchange HTTP {status}: {msg}");
    }
    TokenBundle::from_token_response(&body)
}

async fn refresh_tokens(store: &TokenStore, refresh_token: &str) -> Result<TokenBundle> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut form = HashMap::new();
    form.insert("grant_type", "refresh_token");
    form.insert("client_id", CLIENT_ID);
    form.insert("refresh_token", refresh_token);

    let res = client
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .context("refresh request failed")?;

    let status = res.status();
    let body: Value = res.json().await.context("refresh response not JSON")?;
    if !status.is_success() {
        let msg = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("refresh failed");
        if status.as_u16() >= 400 && status.as_u16() < 500 {
            let _ = store.clear();
        }
        bail!("refresh HTTP {status}: {msg}");
    }

    let mut bundle = TokenBundle::from_token_response(&body)?;
    if bundle.refresh_token.is_none() {
        bundle.refresh_token = Some(refresh_token.to_string());
    }
    store.save(&bundle)?;
    Ok(bundle)
}

/// Return a valid access token, refreshing if needed.
pub async fn ensure_fresh_access_token(store: &TokenStore) -> Result<String> {
    let bundle = store
        .load()?
        .context("not logged in — run `grok-build-booster login`")?;

    if !bundle.needs_refresh() {
        return Ok(bundle.access_token);
    }

    let refresh = bundle
        .refresh_token
        .as_deref()
        .context("access expired and no refresh_token; run login again")?;
    let fresh = refresh_tokens(store, refresh).await?;
    Ok(fresh.access_token)
}
