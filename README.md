<div align="center">

# Grok Build Booster

### Mission control for the agent that already rewrote your codebase.

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#install)
[![Status](https://img.shields.io/badge/status-v0.1%20mission--ready-success.svg)](#status)

**A starship-grade sidecar TUI for [Grok Build](https://x.ai/cli)** — always-on timeline, live telemetry, budget guardrails, rewind assist, and optional SuperGrok OAuth enrichment.

[Why this exists](#the-itch-you-already-have) ·
[What you get](#what-you-actually-get) ·
[Install](#install) ·
[Quick start](#quick-start) ·
[Architecture](#architecture) ·
[Star the repo](https://github.com/RatioArtificiosa/Grok_Build_Booster)

</div>

---

## The itch you already have

You open Grok Build. You ship.

Then the loop starts:

- *How full is the context window — really?*
- *How many sub-agents are burning quota right now?*
- *What did I ask three prompts ago that broke auth?*
- *Can I undo this turn without reconstructing history in my head?*
- *What is this session actually costing me?*

So you type `/context`. Then `/usage`. Then scroll. Then lose the thread. Then type again.

That isn’t engineering. That’s **instrumentation anxiety**.

**Grok Build Booster exists so you stop negotiating with slash commands and start flying the cockpit.**

It sits in the pane beside Grok. It does not fork the agent. It does not hijack the TUI. It listens — properly — and turns every prompt into a mission log with gauges that update while you think.

> The best tools don’t add steps. They remove the ones you were ashamed of needing.

---

## Why people will star this

| If you… | Booster gives you… |
|--------|---------------------|
| Obsessively re-check context mid-turn | A **Context Fuel** bar, live from session signals |
| Spawn parallel sub-agents and lose count | **RPM** — active sub-agents, 0–8, always visible |
| Fear surprise bills | **Soft warn + hard budget stop** on the agent’s Stop gate |
| Break something three prompts deep | **Rewind assist** — recovery branch + `/rewind` guidance |
| Want a postmortem of “what just happened” | **Flight recorder** export (Markdown timeline) |
| Want titles that aren’t garbage | **Rules offline**, or **SuperGrok OAuth** for real summaries |

No cloud dashboard. No “sign in to our SaaS.” Localhost. Your machine. Your session.

---

## What you actually get

### Mission Control TUI

```
┌─ GROK BUILD BOOSTER · MISSION CONTROL ─────────────────────────────┐
│  context 59% · sess $ · turns · live signals · hooks: N events     │
├─ TIMELINE ──────────────┬─ INSPECT ────────────────────────────────┤
│  ▶ 06 DB  user query…   │  # chips · HEAD · SuperGrok / rules      │
│    05 SEC fix oauth…    │  full prompt · files touched             │
├─ TELEMETRY ─────────────┴──────────────────────────────────────────┤
│  CONTEXT fuel  [LIVE|EST]                                          │
│  RPM · ODO · TRIP · COST · TEMP · SPD · BURN · SESS                │
└─ ↑↓ nav · R rewind · E export · S save · q quit ───────────────────┘
```

### Smart timeline

Every `UserPromptSubmit` becomes a bookmark:

- Color category (security, db, ui, tests, api, config, refactor, devops, docs…)
- Three-word scan title (rules now; SuperGrok when logged in)
- Full prompt + file touch list
- Git HEAD snapshot at the moment of the prompt

### Atomic-ish recovery (without being reckless)

Press **`R`**:

1. Dry-run awareness (dirty paths)
2. Non-destructive git recovery branch (`_grok_booster_recovery/…`)
3. Clear instructions to use Grok’s native **`/rewind`** / double-Esc

We deliberately do **not** default to `git reset --hard`. Grok already owns file snapshots + conversation truncate. Booster makes that path *operational*.

### Cost & context without slash spam

- Context fuel from `~/.grok/sessions/…/signals.json` when available
- Trip vs session cost estimates
- Soft budget (UI only) vs hard budget (`continue: false` on Stop)
- Error temperature from tool failures / stop failures
- Burn rate ($/hour) when the trip is live

### SuperGrok OAuth (optional, not a local LLM tax)

Summaries and categories can use your **subscription path**:

- Tokens: `auth.x.ai` (PKCE)
- Inference: **`cli-chat-proxy.grok.com`** + CLI headers  
- Never OAuth chat against `api.x.ai` (classic 402 footgun)

Offline rules always work. OAuth is pure upgrade.

---

## The integration that makes it real

Grok Build already has first-class **hooks**. Booster is a **command-hook sidecar**:

```
  grok  ──command hook──►  booster-fwd.cmd
                                │
                                ▼
                    grok-build-booster hook-forward
                                │
                                ▼
                    http://127.0.0.1:8765/hooks
                                │
                    ┌───────────┴───────────┐
                    │  bookmarks · telemetry │
                    │  budget · flight log   │
                    └───────────────────────┘
```

### Windows lesson we paid for so you don’t

If the hook command string **contains spaces**, Grok runs it through **PowerShell**. PowerShell does **not** forward stdin to the child. Hooks appear “enabled” and **never fire**.

Booster installs a **space-free** wrapper:

```text
%USERPROFILE%\.grok\hooks\booster-fwd.cmd
```

That’s why it works on real Windows machines, not just in demos.

---

## Install

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Grok Build](https://x.ai/cli) installed and authenticated
- Git (for recovery branches)
- A dual-pane terminal (Windows Terminal, WezTerm, tmux, …)

### Build

```bash
git clone https://github.com/RatioArtificiosa/Grok_Build_Booster.git
cd Grok_Build_Booster
cargo build --release
```

Binary:

- Windows: `target\release\grok-build-booster.exe`
- Unix: `target/release/grok-build-booster`

---

## Quick start

### 1) (Optional) SuperGrok login for smart titles

```bash
./target/release/grok-build-booster login
./target/release/grok-build-booster auth-status
```

### 2) Start Booster **first**

```bash
./target/release/grok-build-booster run --cwd /path/to/your/project
```

This rewrites hooks under `~/.grok/hooks/` and listens on `http://127.0.0.1:8765`.

### 3) Start Grok and **reload hooks**

```bash
cd /path/to/your/project
grok
```

In Grok:

```
/hooks
```

Press **`r`** (reload). You should see global booster hooks (about 11 event registrations).

### 4) Ship a prompt

Type something real. Watch the timeline grow. Header should show:

```text
hooks: N events · last user_prompt_submit · 0s ago
```

### 5) Keys that matter

| Key | Action |
|-----|--------|
| `↑` `↓` / `j` `k` | Navigate timeline |
| `R` then `Y` | Rewind assist |
| `E` | Export flight recorder |
| `S` | Save state |
| `q` | Quit |

### Doctor (when something feels off)

```bash
./target/release/grok-build-booster doctor
```

Also check: `~/.grok/booster/hook-forward.log`

---

## Configuration

`~/.grok/booster/config.json` (create with `init-config`):

```json
{
  "soft_budget_usd": 1.0,
  "hard_budget_usd": 5.0,
  "soft_context_ratio": 0.85,
  "price_per_mtok_usd": 5.0,
  "default_context_limit": 256000,
  "port": 8765,
  "signals_poll_ms": 1500
}
```

| Knob | Behavior |
|------|----------|
| `soft_budget_usd` | UI warning only — **does not** force another agent round |
| `hard_budget_usd` | Stop hook returns `continue: false` |
| `soft_context_ratio` | Warn when context fuel crosses threshold |
| `price_per_mtok_usd` | Estimate blend until richer usage lands |

CLI overrides: `--soft-budget`, `--hard-budget`, `--port`, `BOOSTER_PORT`.

---

## Architecture (for the skeptical engineer)

| Layer | Responsibility |
|-------|----------------|
| `hooks/` | Install + `hook-forward` + axum event server |
| `state/` | Bookmarks, telemetry, persistence |
| `core/` | Git recovery, session signals discovery, watcher |
| `enrich/` | Rules categorizer + SuperGrok remote labels |
| `oauth/` | PKCE login, token vault, refresh |
| `ui/` | ratatui mission-control theme |
| `export/` | Flight recorder Markdown |

**Design constraints we refused to violate**

1. No patch to official Grok Build core (external PRs aren’t accepted anyway).
2. No default `git reset --hard`.
3. Soft budget must never keep the agent working via Stop feedback abuse.
4. Enrichment must not block UserPromptSubmit (background task; rules first).
5. Localhost only. Your tokens stay under `~/.grok/booster/`.

---

## Status

**v0.1 — mission-ready for daily dual-pane use on Windows**, with hooks, timeline, signals, budgets, export, OAuth path.

Honest gaps (we’d rather earn trust than oversell):

- Token/cost is still partly estimated when Grok doesn’t surface full usage on the wire
- One-key *live* memory rewrite is Grok’s `/rewind` (by design for Tier A sidecar)
- Clicking a bookmark cannot scroll Grok’s own TUI without a local fork

If those matter to you, open an issue — or fork and go Tier B/C.

---

## Why fork it

Because the next coding agent war won’t be won by bigger models alone.

It will be won by **operators** who can see state, reverse mistakes, and control spend without breaking flow.

This repo is a clean Apache-2.0 Rust crate. No mystery binaries. No telemetry to us. Star it if it removes even one anxious `/context` from your day. Fork it if your team needs harder budgets, fleet views, or a control plane.

<div align="center">

### If this cockpit earns a place next to your agent —

**[★ Star Grok Build Booster](https://github.com/RatioArtificiosa/Grok_Build_Booster)**  
**[Fork it](https://github.com/RatioArtificiosa/Grok_Build_Booster/fork)** · **[Open an issue](https://github.com/RatioArtificiosa/Grok_Build_Booster/issues)**

</div>

---

## CLI map

```text
grok-build-booster run [--port] [--cwd] [--soft-budget] [--hard-budget]
grok-build-booster login | login-complete <url> | auth-status | logout
grok-build-booster install-hooks | hook-forward | doctor
grok-build-booster export | init-config | launch
```

---

## License

Apache License 2.0 — see [LICENSE](LICENSE).

**Not affiliated with xAI.** Grok Build is a product of xAI / SpaceXAI. This is a community sidecar that uses public extension points (hooks, session files, documented OAuth surfaces).

---

<div align="center">

*Build with Grok. Command with Booster.*

</div>
