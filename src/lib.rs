//! Grok Build Booster — sidecar mission control for the official Grok Build CLI.
//!
//! Integration is **command-hooks-first** (Grok HTTP hooks require HTTPS, so we
//! forward via `hook-forward` over loopback). Optional SuperGrok OAuth uses the
//! subscription CLI proxy (`cli-chat-proxy.grok.com`), never `api.x.ai` with OAuth tokens.

pub mod config;
pub mod core;
pub mod enrich;
pub mod export;
pub mod hooks;
pub mod oauth;
pub mod state;
pub mod ui;

pub use state::{AppState, Bookmark, Telemetry, TopicCategory};
