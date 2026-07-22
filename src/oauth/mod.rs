//! SuperGrok / xAI OAuth (Authorization Code + PKCE) for subscription inference.
//!
//! Canonical path (see SuperGrok_OAuth_Integration_Guide.md):
//! - Tokens: `https://auth.x.ai`
//! - Chat: `https://cli-chat-proxy.grok.com/v1` + grok-shell CLI headers
//! - Never send OAuth access tokens to `api.x.ai` for chat (HTTP 402 footgun)

mod constants;
mod flow;
mod pkce;
mod store;

pub use constants::*;
pub use flow::{begin_login, complete_from_callback, ensure_fresh_access_token, login_status};
pub use store::{TokenBundle, TokenStore};
