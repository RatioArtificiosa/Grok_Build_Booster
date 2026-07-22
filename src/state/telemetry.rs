use std::time::Instant;

/// Live dashboard metrics (hooks + session signals).
#[derive(Debug, Clone)]
pub struct Telemetry {
    pub context_limit: u32,
    pub context_used: u32,
    pub total_tokens_lifetime: u64,
    pub trip_tokens: u32,
    pub trip_cost_usd: f64,
    /// Accumulated estimated session cost (survives trips).
    pub session_cost_usd: f64,
    pub active_subagents: u8,
    pub error_temperature: f32,
    pub tokens_per_second: f32,
    pub tool_ok: u64,
    pub tool_fail: u64,
    pub stop_failures: u64,
    pub price_per_mtok_usd: f64,
    pub model_id: Option<String>,
    pub compaction_count: u32,
    pub turn_count: u32,
    /// True when last update came from signals.json
    pub signals_source: bool,
    pub budget_soft_hit: bool,
    pub budget_hard_hit: bool,
    trip_started: Option<Instant>,
    tokens_at_trip_start: u64,
    last_lifetime_for_delta: u64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            context_limit: 256_000,
            context_used: 0,
            total_tokens_lifetime: 0,
            trip_tokens: 0,
            trip_cost_usd: 0.0,
            session_cost_usd: 0.0,
            active_subagents: 0,
            error_temperature: 0.0,
            tokens_per_second: 0.0,
            tool_ok: 0,
            tool_fail: 0,
            stop_failures: 0,
            price_per_mtok_usd: 5.0,
            model_id: None,
            compaction_count: 0,
            turn_count: 0,
            signals_source: false,
            budget_soft_hit: false,
            budget_hard_hit: false,
            trip_started: None,
            tokens_at_trip_start: 0,
            last_lifetime_for_delta: 0,
        }
    }
}

impl Telemetry {
    pub fn on_new_prompt(&mut self) {
        self.trip_tokens = 0;
        self.trip_cost_usd = 0.0;
        self.trip_started = Some(Instant::now());
        self.tokens_at_trip_start = self.total_tokens_lifetime;
        self.tokens_per_second = 0.0;
        // Soft re-evaluated on next evaluate_budget; hard stays latched until cleared
        // by evaluate_budget when under limit again.
    }

    pub fn record_tokens(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        self.total_tokens_lifetime = self.total_tokens_lifetime.saturating_add(n as u64);
        self.trip_tokens = self.trip_tokens.saturating_add(n);
        // Only bump context estimate if signals are not authoritative
        if !self.signals_source {
            self.context_used = self
                .context_used
                .saturating_add(n)
                .min(self.context_limit.max(1));
        }
        self.recompute_costs();
        self.recompute_speed();
        self.recompute_temperature_pub();
    }

    /// Absolute lifetime token counter (e.g. from external source). Applies delta to trip.
    pub fn set_lifetime_tokens(&mut self, absolute: u64) {
        if absolute >= self.last_lifetime_for_delta {
            let delta = absolute - self.last_lifetime_for_delta;
            // Ignore absurd jumps (corrupt file / reset)
            if delta > 0 && delta < 50_000_000 {
                self.trip_tokens = self.trip_tokens.saturating_add(delta.min(u64::from(u32::MAX)) as u32);
            }
        }
        self.total_tokens_lifetime = absolute.max(self.total_tokens_lifetime);
        self.last_lifetime_for_delta = absolute;
        self.recompute_costs();
        self.recompute_speed();
    }

    pub fn set_context_used(&mut self, used: u32) {
        if used > self.context_limit {
            // Expand limit rather than clamp into a lie
            self.context_limit = used;
        }
        self.context_used = used;
    }

    pub fn set_context_limit(&mut self, limit: u32) {
        if limit == 0 {
            return;
        }
        self.context_limit = limit;
        if self.context_used > limit {
            self.context_used = limit;
        }
    }

    pub fn subagent_start(&mut self) {
        self.active_subagents = self.active_subagents.saturating_add(1).min(8);
    }

    pub fn subagent_stop(&mut self) {
        self.active_subagents = self.active_subagents.saturating_sub(1);
    }

    pub fn tool_success(&mut self) {
        self.tool_ok = self.tool_ok.saturating_add(1);
        self.recompute_temperature_pub();
    }

    pub fn tool_failure(&mut self) {
        self.tool_fail = self.tool_fail.saturating_add(1);
        self.recompute_temperature_pub();
    }

    pub fn stop_failure(&mut self) {
        self.stop_failures = self.stop_failures.saturating_add(1);
        self.recompute_temperature_pub();
    }

    pub fn recompute_temperature_pub(&mut self) {
        let total = (self.tool_ok + self.tool_fail + self.stop_failures).max(1) as f32;
        let bad = (self.tool_fail * 2 + self.stop_failures * 3) as f32;
        self.error_temperature = ((bad / total) * 100.0).clamp(0.0, 100.0);
    }

    fn recompute_costs(&mut self) {
        let ppm = self.price_per_mtok_usd.max(0.0);
        self.trip_cost_usd = (self.trip_tokens as f64 / 1_000_000.0) * ppm;
        self.session_cost_usd = (self.total_tokens_lifetime as f64 / 1_000_000.0) * ppm;
    }

    fn recompute_speed(&mut self) {
        if let Some(start) = self.trip_started {
            let secs = start.elapsed().as_secs_f32().max(0.001);
            let delta = self
                .total_tokens_lifetime
                .saturating_sub(self.tokens_at_trip_start) as f32;
            self.tokens_per_second = delta / secs;
        }
    }

    pub fn fuel_ratio(&self) -> f64 {
        if self.context_limit == 0 {
            0.0
        } else {
            (self.context_used as f64 / self.context_limit as f64).clamp(0.0, 1.0)
        }
    }

    /// Evaluate soft/hard budget from config thresholds.
    /// Hard latch clears when cost falls back under the limit (e.g. config raised).
    pub fn evaluate_budget(&mut self, soft: Option<f64>, hard: Option<f64>) {
        self.budget_soft_hit = match soft {
            Some(s) if s > 0.0 => {
                self.session_cost_usd >= s || self.trip_cost_usd >= s
            }
            _ => false,
        };
        self.budget_hard_hit = match hard {
            Some(h) if h > 0.0 => self.session_cost_usd >= h,
            _ => false,
        };
    }

    pub fn burn_rate_usd_per_hour(&self) -> f64 {
        let Some(start) = self.trip_started else {
            return 0.0;
        };
        let hours = start.elapsed().as_secs_f64() / 3600.0;
        if hours < 1.0 / 3600.0 {
            // < 1 second of trip — don't explode burn rate
            return 0.0;
        }
        self.trip_cost_usd / hours
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_clears_when_under_limit() {
        let mut t = Telemetry::default();
        t.session_cost_usd = 10.0;
        t.evaluate_budget(Some(1.0), Some(5.0));
        assert!(t.budget_hard_hit);
        t.session_cost_usd = 0.5;
        t.evaluate_budget(Some(1.0), Some(5.0));
        assert!(!t.budget_hard_hit);
        assert!(!t.budget_soft_hit);
    }

    #[test]
    fn set_context_used_expands_limit() {
        let mut t = Telemetry::default();
        t.context_limit = 100;
        t.set_context_used(150);
        assert_eq!(t.context_used, 150);
        assert_eq!(t.context_limit, 150);
    }
}
