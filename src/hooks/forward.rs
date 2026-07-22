//! `hook-forward` — invoked by Grok command hooks.
//!
//! Reads the hook event JSON from stdin, POSTs to Booster's localhost server,
//! prints the response body to stdout (so Stop decisions flow back to Grok).
//! Always exits 0 (fail-open) so a dead Booster never blocks the agent.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::PathBuf;

pub async fn run_forward(port: u16) -> Result<()> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("read hook stdin")?;

    // Tiny debug trail so users can prove hooks fire even when the TUI is closed
    append_debug_log(&format!(
        "forward in bytes={} port={port}",
        buf.len()
    ));

    if buf.trim().is_empty() {
        write_stdout("{}");
        return Ok(());
    }

    if buf.len() > 4 * 1024 * 1024 {
        eprintln!("booster hook-forward: stdin too large ({} bytes)", buf.len());
        append_debug_log("stdin too large");
        write_stdout("{}");
        return Ok(());
    }

    let url = format!("http://127.0.0.1:{port}/hooks");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .no_proxy()
        .build()?;

    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(buf)
        .send()
        .await
    {
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_else(|_| "{}".into());
            let out = if text.trim().is_empty() {
                "{}"
            } else {
                text.trim()
            };
            write_stdout(out);
            append_debug_log(&format!("forward ok status={status} out_len={}", out.len()));
            if !status.is_success() {
                eprintln!("booster hook-forward: upstream HTTP {status}");
            }
        }
        Err(e) => {
            eprintln!("booster hook-forward: {e} (is `grok-build-booster run` up on :{port}?)");
            append_debug_log(&format!("forward err: {e}"));
            write_stdout("{}");
        }
    }
    Ok(())
}

fn write_stdout(s: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

fn debug_log_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = home.join(".grok").join("booster");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("hook-forward.log"))
}

fn append_debug_log(msg: &str) {
    let Some(path) = debug_log_path() else {
        return;
    };
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{ts} {msg}");
    }
}
