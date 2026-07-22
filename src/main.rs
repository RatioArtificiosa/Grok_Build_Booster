use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use grok_build_booster::config::BoosterConfig;
use grok_build_booster::core::watch::spawn_signals_watcher;
use grok_build_booster::export::export_markdown;
use grok_build_booster::hooks::{
    install_hooks, run_forward, spawn_hook_server, SharedConfig, SharedState,
};
use grok_build_booster::oauth::{
    begin_login, complete_from_callback, login_status, TokenStore,
};
use grok_build_booster::state::{load_state, AppState};
use grok_build_booster::ui::run_tui;

#[derive(Parser, Debug)]
#[command(
    name = "grok-build-booster",
    about = "Mission-control sidecar for Grok Build (bookmarks, telemetry, SuperGrok OAuth, budget, flight recorder)",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the TUI + hook server (default)
    Run {
        #[arg(long, default_value_t = 8765, env = "BOOSTER_PORT")]
        port: u16,
        #[arg(long, default_value_t = true)]
        install_hooks: bool,
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Soft budget USD (overrides config)
        #[arg(long, env = "BOOSTER_SOFT_BUDGET")]
        soft_budget: Option<f64>,
        /// Hard budget USD (overrides config)
        #[arg(long, env = "BOOSTER_HARD_BUDGET")]
        hard_budget: Option<f64>,
    },
    /// SuperGrok OAuth login (subscription → cli-chat-proxy.grok.com)
    Login,
    LoginComplete { url: String },
    AuthStatus,
    Logout,
    InstallHooks {
        #[arg(long, default_value_t = 8765)]
        port: u16,
    },
    /// Invoked by Grok command hooks (stdin event → localhost Booster)
    HookForward {
        #[arg(long, default_value_t = 8765, env = "BOOSTER_PORT")]
        port: u16,
    },
    /// Export flight recorder Markdown from saved state
    Export {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Write default config.json if missing / show path
    InitConfig,
    /// Diagnose hooks install, server reachability, and recent hook-forward log
    Doctor {
        #[arg(long, default_value_t = 8765)]
        port: u16,
    },
    Launch {
        #[arg(long, default_value_t = 8765)]
        port: u16,
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // hook-forward should be quiet on stdout (only response JSON)
    let is_forward = std::env::args().any(|a| a == "hook-forward");
    if !is_forward {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("error"))
            .with_writer(std::io::stderr)
            .with_target(false)
            .init();
    }

    let cli = Cli::parse();
    let store = Arc::new(TokenStore::default_paths()?);

    match cli.cmd.unwrap_or(Commands::Run {
        port: 8765,
        install_hooks: true,
        cwd: None,
        soft_budget: None,
        hard_budget: None,
    }) {
        Commands::Run {
            port,
            install_hooks: do_install,
            cwd,
            soft_budget,
            hard_budget,
        } => {
            if do_install {
                let path = install_hooks(port)?;
                tracing::info!("Installed command hooks → {}", path.display());
            }
            run_app(port, cwd, store, soft_budget, hard_budget).await
        }
        Commands::Login => {
            let bundle = begin_login(store.as_ref()).await?;
            println!(
                "Logged in (token {}). Inference: cli-chat-proxy.grok.com — not api.x.ai.",
                bundle.access_last4()
            );
            Ok(())
        }
        Commands::LoginComplete { url } => {
            let bundle = complete_from_callback(store.as_ref(), &url).await?;
            println!("Login complete (token {}).", bundle.access_last4());
            Ok(())
        }
        Commands::AuthStatus => {
            println!("{}", login_status(store.as_ref())?);
            Ok(())
        }
        Commands::Logout => {
            store.clear()?;
            println!("OAuth tokens cleared.");
            Ok(())
        }
        Commands::InstallHooks { port } => {
            let path = install_hooks(port)?;
            println!("Wrote {}", path.display());
            println!(
                "Note: Grok HTTP hooks require HTTPS, so Booster uses command hooks + hook-forward."
            );
            Ok(())
        }
        Commands::HookForward { port } => run_forward(port).await,
        Commands::Export { out } => {
            let mut state = AppState::default();
            let _ = load_state(&mut state)?;
            let path = export_markdown(&state, out.as_deref())?;
            println!("Wrote {}", path.display());
            Ok(())
        }
        Commands::InitConfig => {
            let cfg = BoosterConfig::load_or_default();
            cfg.save()?;
            println!("Config → {}", BoosterConfig::path()?.display());
            println!("{}", serde_json::to_string_pretty(&cfg)?);
            Ok(())
        }
        Commands::Doctor { port } => run_doctor(port).await,
        Commands::Launch { port, cwd } => launch_split(port, cwd, store).await,
    }
}

async fn run_doctor(port: u16) -> Result<()> {
    use std::fs;
    println!("Grok Build Booster — doctor\n");

    let hooks_dir = dirs::home_dir()
        .context("home")?
        .join(".grok")
        .join("hooks");
    let json = hooks_dir.join("grok-build-booster.json");
    println!("1) Hooks file");
    if json.exists() {
        println!("   OK  {}", json.display());
        let raw = fs::read_to_string(&json)?;
        if raw.contains("hook-forward") {
            println!("   OK  contains hook-forward");
        } else {
            println!("   !!  missing hook-forward command — run: grok-build-booster install-hooks");
        }
        if raw.contains("PreToolUse") {
            println!("   !!  still has PreToolUse (blocking). Re-run install-hooks / run to refresh.");
        } else {
            println!("   OK  no PreToolUse (observe-only set)");
        }
    } else {
        println!("   !!  missing — run: grok-build-booster run   (auto-installs)");
    }

    println!("\n2) Hook server :{port}");
    match reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => println!("   OK  health = {}", r.text().await.unwrap_or_default()),
        Ok(r) => println!("   !!  health HTTP {}", r.status()),
        Err(e) => println!("   !!  not reachable ({e}) — start: grok-build-booster run"),
    }

    println!("\n3) Simulated UserPromptSubmit");
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "hookEventName": "user_prompt_submit",
        "prompt": "doctor test prompt — add a hello world",
        "cwd": std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        "sessionId": "doctor"
    });
    match client
        .post(format!("http://127.0.0.1:{port}/hooks"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) => println!("   OK  POST /hooks → {} {}", r.status(), r.text().await.unwrap_or_default()),
        Err(e) => println!("   !!  POST failed: {e}"),
    }

    let log = dirs::home_dir()
        .unwrap()
        .join(".grok")
        .join("booster")
        .join("hook-forward.log");
    println!("\n4) hook-forward.log");
    if log.exists() {
        let raw = fs::read_to_string(&log).unwrap_or_default();
        let lines: Vec<_> = raw.lines().rev().take(8).collect();
        println!("   OK  {} (last lines)", log.display());
        for l in lines.into_iter().rev() {
            println!("      {l}");
        }
    } else {
        println!("   ·   no log yet — Grok has not invoked hook-forward");
        println!("      After a Grok prompt, this file should gain lines.");
    }

    println!(
        "\n5) Grok side checklist\n\
         · After install-hooks: in Grok /hooks → press  r  (reload), then SEND A NEW PROMPT.\n\
         · Hooks only fire on events (UserPromptSubmit, tools, stop) — not while idle.\n\
         · Command must be a path WITHOUT spaces (PowerShell shell mode breaks stdin).\n\
         · Keep booster run UP (hooks POST to localhost:{port}).\n\
         · Log: %USERPROFILE%\\.grok\\booster\\hook-forward.log  (grows when Grok actually runs hooks)\n"
    );

    // Show installed command
    if json.exists() {
        if let Ok(raw) = fs::read_to_string(&json) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(cmd) = v
                    .pointer("/hooks/UserPromptSubmit/0/hooks/0/command")
                    .and_then(|c| c.as_str())
                {
                    println!("6) Installed UserPromptSubmit command:\n   {cmd}");
                    if cmd.contains(' ') {
                        println!(
                            "   !! CONTAINS SPACES — Grok will use PowerShell and stdin will break.\n\
                             Re-run: grok-build-booster install-hooks"
                        );
                    } else {
                        println!("   OK  no spaces (direct spawn path)");
                    }
                }
            }
        }
    }
    Ok(())
}

async fn run_app(
    port: u16,
    cwd: Option<PathBuf>,
    store: Arc<TokenStore>,
    soft_budget: Option<f64>,
    hard_budget: Option<f64>,
) -> Result<()> {
    let mut cfg = BoosterConfig::load_or_default();
    cfg.port = port;
    if let Some(s) = soft_budget {
        cfg.soft_budget_usd = Some(s);
    }
    if let Some(h) = hard_budget {
        cfg.hard_budget_usd = Some(h);
    }
    let _ = cfg.save();

    let state: SharedState = Arc::new(Mutex::new(AppState::default()));
    {
        let mut s = state.lock().await;
        let _ = load_state(&mut s);
        s.telemetry.price_per_mtok_usd = cfg.price_per_mtok_usd;
        s.telemetry.context_limit = cfg.default_context_limit;
        if let Some(c) = cwd {
            s.project_cwd = Some(c.display().to_string());
        }
    }

    let config: SharedConfig = Arc::new(Mutex::new(cfg.clone()));

    let state_srv = state.clone();
    let store_srv = store.clone();
    let cfg_srv = config.clone();
    tokio::spawn(async move {
        if let Err(e) = spawn_hook_server(port, state_srv, store_srv, cfg_srv).await {
            tracing::error!("hook server exited: {e:#}");
        }
    });

    spawn_signals_watcher(state.clone(), config.clone());

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    println!(
        "Grok Build Booster\n\
         · Hook server: http://127.0.0.1:{port}/hooks\n\
         · Hooks re-installed → %USERPROFILE%\\.grok\\hooks\\grok-build-booster.json\n\
         · IMPORTANT: in Grok run  /hooks  then press  r  (reload), or restart grok\n\
         · Header must show  hooks: N events  after you send a prompt (not only restored state)\n\
         · OAuth: {}\n\
         · Budget soft={:?} hard={:?}\n\
         · Keys: ↑/↓ · R · E · S · q    ·  doctor: grok-build-booster doctor\n",
        login_status(store.as_ref()).unwrap_or_else(|_| "unknown".into()),
        cfg.soft_budget_usd,
        cfg.hard_budget_usd,
    );

    run_tui(state, store).await
}

async fn launch_split(port: u16, cwd: Option<PathBuf>, store: Arc<TokenStore>) -> Result<()> {
    let _ = install_hooks(port)?;
    let booster_exe = std::env::current_exe().context("current_exe")?;
    let work = cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let booster_cmd = format!(
        "\"{}\" run --port {port} --cwd \"{}\"",
        booster_exe.display(),
        work.display()
    );

    #[cfg(windows)]
    {
        use std::process::Command;
        let status = Command::new("wt")
            .args([
                "-d",
                work.to_str().unwrap_or("."),
                "grok",
                ";",
                "split-pane",
                "-H",
                "-d",
                work.to_str().unwrap_or("."),
                "cmd",
                "/c",
                &booster_cmd,
            ])
            .status();

        match status {
            Ok(s) if s.success() => {
                println!("Launched Windows Terminal split (grok | booster).");
                return Ok(());
            }
            Ok(s) => tracing::warn!("wt exited with {s}; falling back"),
            Err(e) => tracing::warn!("wt not available ({e}); falling back"),
        }
    }

    let _ = store;
    println!(
        "Manual split:\n  A) grok\n  B) {} run --port {port}\n",
        booster_exe.display()
    );
    run_app(port, Some(work), store, None, None).await
}
