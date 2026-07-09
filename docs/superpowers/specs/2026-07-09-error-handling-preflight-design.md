# Error handling + Claude preflight — design

**Date:** 2026-07-09
**Ticket:** [`docs/TODO.md`](../../TODO.md) #3
**Branch:** `feat/error-handling-preflight`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

`claude` is the whole engine (relevance judging + summaries). Today, if Claude Code is
not installed, not on the Finder-launched PATH, or not logged in, a tick fails **silently**
(the raw spawn/stderr string lands in `last_error`, shown only as a terse "error" chip) and
a **zero-monitor** fresh clone gets no signal at all. A person who installs the app without a
working, authenticated `claude` sees a broken-looking, unexplained app.

The plumbing half-exists: a failed tick already writes `last_error` → renders as an "error"
chip (shipped in #1). #3 is a **quality upgrade on top of that**, not a rebuild:

1. **Startup preflight** so a fresh clone learns Claude is missing/logged-out *before*
   creating any monitor.
2. **Classify** the raw failure into friendly, actionable copy and into
   **paused-vs-error**.
3. The **binary-missing** case, which today falls back to the bare name `"claude"` and
   produces an ugly spawn error.

## Goal (acceptance)

On a machine without Claude Code (or logged out), the app opens and immediately tells the
user exactly what is wrong and how to fix it — no silent empty feed. Every tick failure mode
shows a human-readable reason in the UI. A failed tick still never kills its worker.

## Empirical grounding (verified locally on claude 2.1.205)

Rather than guess Claude's behavior, we captured it:

- `claude --version` → exit `0` (binary works). Resolved binary lives at `~/.local/bin/claude`
  — **not** on a Finder-launched app's minimal PATH, so *binary-missing* is the most likely
  real failure (the exact issue fought in Session 3).
- Authenticated `claude -p "…" --safe-mode` → exit `0`, result on **stdout**, empty stderr.
- **`claude auth status --json`** → exit `0` and `{"loggedIn": true, "authMethod": …}`.
  This is a **local, no-model-call, structured** auth probe (exit `0`/`1` + `loggedIn` bool).
  Logged-out: exit `1` (per Claude Code docs: stderr `Not logged in · Please run /login`).
- `--safe-mode` **preserves** OAuth/keychain auth (we rely on this); `--bare` strips it — so
  we keep `--safe-mode`, never `--bare`.

Consequence: preflight uses **`claude auth status --json`** (cheap, structured, no tokens),
**not** a real `-p` probe.

## Key decision: status taxonomy (paused vs error)

Three monitor states, chosen by *why* the last tick failed:

| status | when | cause kind |
| --- | --- | --- |
| **active** | last tick OK | — |
| **error** | last tick failed, but Claude itself is fine | `claude_timeout`, `hn_error` (transient/per-monitor) |
| **paused** | ticks can't proceed because Claude is unavailable | `claude_missing`, `claude_auth` (global) |

`paused` is a **global** condition — Claude being down affects every monitor, so it drives a
single top **banner** plus a `Paused` chip on all monitors. `error` is **per-monitor and
transient** and drives the existing per-row chip + tooltip. Only `claude_missing` /
`claude_auth` flip global health; timeouts and HN-fetch failures stay per-monitor `error`.

## Non-goals (staying strictly in #3)

- No system tray / notifications (Phase 3), no #4 wall-clock/catch-up scheduling, no #2
  watermark/pagination.
- **No `BadVerdict` / "empty result = error".** `judge` keeps returning `Ok([])` for a
  no-match *or* unparseable response, so we do **not** regress the "checked N, nothing
  matched yet" state #1 shipped. A tick with 0 matches is success, never an error.
- No monitor pause/resume/edit/"Run now" controls.

## Design

### A. Backend — typed errors, classified by pure functions

Replace the ad-hoc `String` errors on the tick path with small enums that carry a **stable
machine `code`** and a **friendly `message`** (mirroring the existing `parse_verdict` /
`find_claude` test seam — classification is pure and unit-tested).

```
enum AgentError { NotFound, NotAuthenticated, Timeout, Failed(String) }
enum TickError  { Hn(String), Agent(AgentError), Db(String) }
```

- `code()` → e.g. `claude_missing`, `claude_auth`, `claude_timeout`, `claude_error`,
  `hn_error`, `db_error`. Used to decide paused-vs-error and to flip global health.
- `message()` → human copy stored in `last_error` and shown in the meta-line tooltip, e.g.
  "Claude Code isn't logged in", "Claude timed out", "Couldn't reach Hacker News".

`agent::judge` returns `Result<Vec<Verdict>, AgentError>`:
- spawn error with `ErrorKind::NotFound` → `NotFound`;
- timeout → `Timeout`;
- non-zero exit whose stderr matches a login/auth signal → `NotAuthenticated`, else
  `Failed(stderr)`.

`tick::run_tick` returns `Result<TickOutcome, TickError>` (HN failure → `Hn`, judge failure →
`Agent`, db failure → `Db`). The classification of the exit/stderr into `AgentError` and the
auth-status output into health are **pure functions** with unit tests.

### B. Backend — preflight probe + shared health

- `enum ClaudeHealth { Ok, Missing, NotAuthenticated }` with a `code`/`message` projection
  for the UI.
- `agent::preflight() -> ClaudeHealth`: if the binary isn't found (a new
  `claude_present()` beside the existing `claude_bin()`), return `Missing` **without
  spawning**; otherwise run `claude auth status --json` (sandboxed — see DRY helper below),
  feed its `(exit, stdout)` to a pure `classify_auth(...)` → `Ok` / `NotAuthenticated`. If the
  probe itself errors or times out, default to `Ok` (don't false-alarm; real ticks will
  surface genuine failures).
- Shared state: `AppState` gains `claude_health: Arc<Mutex<ClaudeHealth>>`.
  - `init_state` kicks off preflight **async in `setup`** (window never blocks on it); the
    result is written to the shared state and a **`claude-health`** event is emitted.
  - **Ticks keep it live:** on each tick result, a `claude_missing`/`claude_auth` failure sets
    health accordingly; any success sets `Ok`. So once the user logs in, the next successful
    tick clears the banner automatically (self-healing). This threads the health handle into
    `scheduler.spawn(...)` — a real signature change.
- `to_monitor_dto` consults health: when global health ≠ `Ok`, every monitor's `status` is
  `"paused"`; otherwise it derives `"error"`/`"active"` from `last_error` as today.
- New commands: `claude_health()` (banner reads current health) and `recheck_claude()`
  (banner's "Re-check" button re-runs `preflight()`, updates state, emits `claude-health` —
  cheap because `auth status` is instant).

### C. DRY — one sandboxed command builder

Extract `agent::claude_command() -> tokio::process::Command` that applies the existing
sandbox (run from `std::env::temp_dir()`, override `PWD`, `stdin(null)`, `kill_on_drop`) so
the judge call and the auth probe share it — instead of duplicating that comment block
(CLAUDE.md DRY rule). Callers append their own args (`-p --safe-mode <prompt>` for the judge,
`auth status --json` for the probe). The `--safe-mode` flag stays on the judge call only.

### D. DTO / API

- `MonitorDto.status` already exists; its value set now includes `"paused"` (the
  `src/types.ts` `MonitorStatus` already declares it).
- New serialized health shape crossing the boundary:

  | field | type | notes |
  | --- | --- | --- |
  | `status` | `"ok" \| "missing" \| "notAuthenticated"` | current Claude health |
  | `message` | `string` | friendly, actionable copy for the banner |

- `api.ts`: `getClaudeHealth()` command wrapper, `recheckClaude()` wrapper, and an
  `onClaudeHealth(cb)` event listener.

### E. Frontend — banner + Paused chip

- A new top **banner** component, shown only when health ≠ `ok`, persistent while the problem
  exists (non-dismissible — it is blocking-but-friendly). Copy:
  - **missing** → "Claude Code not found — HN Watch needs it to judge stories. Install it,
    then Re-check."
  - **notAuthenticated** → "Claude Code isn't logged in — run `claude` in a terminal to log
    in, then Re-check."
  - A **Re-check** button calls `recheckClaude()`.
  - Styled with existing `rust` / `hn-soft` / `hn-border` tokens — no new colors.
- `App` holds `health` state, seeded from `getClaudeHealth()` and updated via
  `onClaudeHealth`; renders the banner above the main row.
- `Sidebar` `MonitorRow`: the `Paused` status is already wired for the dot (`bg-faint`); now
  it also renders a **`Paused`** countdown pill (in place of `next in Xm`) when
  `status === "paused"`. The per-tick friendly `lastError` continues to flow into the existing
  meta-line tooltip — better strings, no new plumbing.

## Testing

- **Rust unit tests (pure functions):**
  - `classify_auth`: exit `0` + `loggedIn:true` → `Ok`; exit `0` + `loggedIn:false` →
    `NotAuthenticated`; non-zero exit → `NotAuthenticated`; unparseable stdout on exit `0` →
    `Ok` (don't false-alarm).
  - `AgentError` / `TickError` → `code()` + `message()` mappings; the judge exit/stderr →
    `AgentError` classifier (spawn-not-found, timeout, auth-signal stderr, other).
  - `to_monitor_dto`: global health ≠ `Ok` forces `status:"paused"`; health `Ok` derives
    `error`/`active` from `last_error`.
  - No-match / unparseable judge output stays `Ok([])` (guards the non-goal).
- **Live verification** in the native window per [`docs/TESTING.md`](../../TESTING.md),
  without logging out: an env override (e.g. `HN_WATCH_CLAUDE_BIN`) points `claude_bin()` /
  `claude_present()` at a fake script that mimics **missing** (nonexistent path) and
  **not-logged-in** (`auth status` exits `1`) shapes. Verify: Missing banner + Paused chips;
  Not-logged-in banner + Paused chips; **Re-check** clears the banner once the real binary is
  restored; a healthy run shows no banner and normal `active` countdowns; a forced transient
  failure (e.g. HN unreachable) shows a per-monitor `error` chip but **no** global banner.

## Files touched

- `src-tauri/src/agent.rs` — `claude_command()` helper, `claude_present()`, `AgentError`,
  judge returns typed error, `ClaudeHealth`, `classify_auth`, `preflight()`.
- `src-tauri/src/tick.rs` — `TickError`, `run_tick` return type.
- `src-tauri/src/scheduler.rs` — accept the health handle; update health from tick results;
  persist `error.message()`.
- `src-tauri/src/commands.rs` — `AppState.claude_health`; `to_monitor_dto` reads health;
  `claude_health` + `recheck_claude` commands; async preflight in `init_state`.
- `src-tauri/src/lib.rs` — register the two new commands.
- `src/types.ts` — `ClaudeHealth` shape (`MonitorStatus` already has `paused`).
- `src/api.ts` — `getClaudeHealth`, `recheckClaude`, `onClaudeHealth`.
- `src/App.tsx` — health state + banner wiring.
- `src/components/ClaudeBanner.tsx` — new banner component.
- `src/components/Sidebar.tsx` — `Paused` countdown pill.
