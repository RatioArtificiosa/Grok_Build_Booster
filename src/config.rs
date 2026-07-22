//! Booster runtime config (budget, prices, paths).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BoosterConfig {
    /// Soft warn when trip cost exceeds this (USD). None = disabled.
    pub soft_budget_usd: Option<f64>,
    /// Hard stop agent turn when session/trip lifetime cost estimate exceeds this.
    pub hard_budget_usd: Option<f64>,
    /// Soft warn when context fuel ratio exceeds this (0–1).
    pub soft_context_ratio: f64,
    /// Blended $/MTok for estimates when Grok does not report cost ticks.
    pub price_per_mtok_usd: f64,
    /// Default context window if signals omit it.
    pub default_context_limit: u32,
    /// Hook server port (also used by command-hook forwarder).
    pub port: u16,
    /// Poll interval for ~/.grok/sessions signals.json (ms).
    pub signals_poll_ms: u64,
}

impl Default for BoosterConfig {
    fn default() -> Self {
        Self {
            soft_budget_usd: Some(1.0),
            hard_budget_usd: Some(5.0),
            soft_context_ratio: 0.85,
            price_per_mtok_usd: 5.0,
            default_context_limit: 256_000,
            port: 8765,
            signals_poll_ms: 1500,
        }
    }
}

impl BoosterConfig {
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::home_dir()
            .context("no home dir")?
            .join(".grok")
            .join("booster");
        fs::create_dir_all(&dir)?;
        Ok(dir.join("config.json"))
    }

    pub fn load_or_default() -> Self {
        match Self::path().and_then(|p| {
            if !p.exists() {
                return Ok(Self::default());
            }
            let raw = fs::read_to_string(&p)?;
            Ok(serde_json::from_str(&raw)?)
        }) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("config load failed ({e:#}); using defaults");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn booster_data_dir() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("no home dir")?
        .join(".grok")
        .join("booster");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}
