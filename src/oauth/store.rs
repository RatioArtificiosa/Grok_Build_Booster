use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use super::constants::REFRESH_SKEW_SECS;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix ms when access token expires.
    pub expires_at: i64,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

impl TokenBundle {
    pub fn from_token_response(v: &serde_json::Value) -> Result<Self> {
        let access = v
            .get("access_token")
            .and_then(|x| x.as_str())
            .context("token response missing access_token")?
            .to_string();
        let refresh = v
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let expires_in = v
            .get("expires_in")
            .and_then(|x| x.as_i64())
            .unwrap_or(3600);
        let now = chrono::Utc::now().timestamp_millis();
        Ok(Self {
            access_token: access,
            refresh_token: refresh,
            expires_at: now + expires_in * 1000,
            token_type: v
                .get("token_type")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            scope: v
                .get("scope")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        })
    }

    pub fn needs_refresh(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        now >= self.expires_at - REFRESH_SKEW_SECS * 1000
    }

    pub fn access_last4(&self) -> String {
        let t = self.access_token.as_str();
        if t.len() <= 4 {
            "****".into()
        } else {
            format!("…{}", &t[t.len() - 4..])
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
    pub created_at: i64,
}

pub struct TokenStore {
    path: PathBuf,
    pending_path: PathBuf,
}

impl TokenStore {
    pub fn default_paths() -> Result<Self> {
        let dir = dirs::home_dir()
            .context("no home directory")?
            .join(".grok")
            .join("booster");
        fs::create_dir_all(&dir).context("create ~/.grok/booster")?;
        Ok(Self {
            path: dir.join("oauth_tokens.json"),
            pending_path: dir.join("oauth_pkce.json"),
        })
    }

    pub fn load(&self) -> Result<Option<TokenBundle>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path)?;
        let bundle: TokenBundle = serde_json::from_str(&raw)?;
        Ok(Some(bundle))
    }

    pub fn save(&self, bundle: &TokenBundle) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(bundle)?;
        fs::write(&self.path, raw)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }

    pub fn save_pending(&self, pending: &PendingPkce) -> Result<()> {
        let raw = serde_json::to_string_pretty(pending)?;
        fs::write(&self.pending_path, raw)?;
        Ok(())
    }

    pub fn load_pending(&self) -> Result<Option<PendingPkce>> {
        if !self.pending_path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.pending_path)?;
        Ok(Some(serde_json::from_str(&raw)?))
    }

    pub fn clear_pending(&self) -> Result<()> {
        if self.pending_path.exists() {
            fs::remove_file(&self.pending_path)?;
        }
        Ok(())
    }
}
