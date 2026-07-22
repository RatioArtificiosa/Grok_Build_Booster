//! Background poll of Grok session signals.json

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::config::BoosterConfig;
use crate::core::session::{apply_signals_to_telemetry, find_latest_session_dir, read_signals};
use crate::hooks::SharedState;
use crate::state::maybe_save;

pub fn spawn_signals_watcher(state: SharedState, config: Arc<Mutex<BoosterConfig>>) {
    tokio::spawn(async move {
        let mut save_ticks: u32 = 0;
        loop {
            let poll_ms = {
                let c = config.lock().await;
                c.signals_poll_ms.clamp(500, 60_000)
            };
            tokio::time::sleep(Duration::from_millis(poll_ms)).await;

            let (cwd, pinned) = {
                let s = state.lock().await;
                (s.project_cwd.clone(), s.status_pinned || s.show_confirm_rollback)
            };
            let Some(cwd) = cwd else { continue };

            let session = match find_latest_session_dir(&cwd) {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("session discover: {e:#}");
                    continue;
                }
            };
            let Some(session_dir) = session else { continue };

            match read_signals(&session_dir) {
                Ok(Some(sig)) => {
                    let mut s = state.lock().await;
                    s.session_dir = Some(session_dir.display().to_string());
                    apply_signals_to_telemetry(&sig, &mut s.telemetry);
                    let cfg = {
                        // Release pattern: clone config under lock after state
                        // We already hold state; lock config next (consistent order).
                        config.lock().await.clone()
                    };
                    s.telemetry.price_per_mtok_usd = cfg.price_per_mtok_usd;
                    if s.telemetry.context_limit == 0 {
                        s.telemetry.context_limit = cfg.default_context_limit;
                    }
                    s.telemetry
                        .evaluate_budget(cfg.soft_budget_usd, cfg.hard_budget_usd);

                    if !pinned && !s.show_confirm_rollback && !s.status_pinned {
                        let cost = s.telemetry.session_cost_usd;
                        let hard = s.telemetry.budget_hard_hit;
                        let soft = s.telemetry.budget_soft_hit;
                        let fuel = s.telemetry.fuel_ratio();
                        let turns = s.telemetry.turn_count;
                        let model = s
                            .telemetry
                            .model_id
                            .clone()
                            .unwrap_or_else(|| "?".into());
                        if hard {
                            s.set_status_force(format!("BUDGET HARD — ${cost:.4} (signals)"));
                        } else if soft {
                            s.set_status(format!("Budget soft — ${cost:.4}"));
                        } else if fuel >= cfg.soft_context_ratio {
                            s.set_status(format!(
                                "Context {:.0}% · turns {turns} · {model}",
                                fuel * 100.0
                            ));
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::debug!("read signals: {e:#}"),
            }

            // Persist every ~10 polls to cut disk churn
            save_ticks = save_ticks.wrapping_add(1);
            if save_ticks.is_multiple_of(10) {
                let s = state.lock().await;
                maybe_save(&s);
            }
        }
    });
}
