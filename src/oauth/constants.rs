//! Public desktop OAuth client constants (not a secret API key). PKCE replaces a client secret.

pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const ISSUER: &str = "https://auth.x.ai";
pub const AUTHORIZE_URL: &str = "https://auth.x.ai/oauth2/authorize";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
pub const REDIRECT_URI: &str = "http://127.0.0.1:56121/callback";
pub const SCOPES: &str = "openid profile email offline_access grok-cli:access api:access";
pub const REFERRER: &str = "grok-build-booster";

/// Subscription inference host — NOT api.x.ai.
pub const CLI_CHAT_PROXY_BASE: &str = "https://cli-chat-proxy.grok.com/v1";

pub const CLI_USER_AGENT: &str = "grok-shell/0.2.101 (windows; x64)";
pub const CLI_CLIENT_IDENTIFIER: &str = "grok-shell";
pub const CLI_CLIENT_VERSION: &str = "0.2.101";
pub const CLI_CLIENT_MODE: &str = "interactive";
pub const CLI_TOKEN_AUTH: &str = "xai-grok-cli";
pub const CLI_AUTHENTICATE_RESPONSE: &str = "authenticate-response";

/// Default model for short Booster enrichment calls (summaries / categories).
pub const DEFAULT_ENRICH_MODEL: &str = "grok-4.5";

/// Refresh a bit before expiry.
pub const REFRESH_SKEW_SECS: i64 = 90;
