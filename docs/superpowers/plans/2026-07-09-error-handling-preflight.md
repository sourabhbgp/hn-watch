# Error handling + Claude preflight — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Claude failures visible — a startup preflight tells a fresh clone when `claude` is missing or logged out, and every tick failure shows a human-readable reason, with a global "paused" state distinct from per-monitor transient "error".

**Architecture:** Backend gains typed errors (`AgentError`, `TickError`) with stable `code()` + friendly `message()`, a cheap no-token `claude auth status --json` preflight (`ClaudeHealth`), and a shared `Arc<Mutex<ClaudeHealth>>` that preflight seeds and ticks keep live. The monitor DTO maps global health to a `paused` status; the frontend renders a top banner (with Re-check) and a `Paused` chip.

**Tech Stack:** Rust (Tauri 2, rusqlite, tokio, serde_json), React 19 + TypeScript, Tailwind v4.

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-07-09-error-handling-preflight-design.md`. Scope source of truth: `docs/REQUIREMENTS.md`.
- **Stay in #3:** no tray/notifications, no wall-clock scheduling (#4), no watermark/pagination (#2), no monitor pause/resume/edit controls.
- **Non-goal — never make a 0-match or unparseable judge response an error:** `judge` returns `Ok(Vec::new())` for those (guards the "checked N, nothing matched" state shipped in #1).
- **DRY / tokens:** reuse existing design tokens in `src/index.css` (`rust`, `hn-soft`, `hn-border`, `faint`, `soft`, `paper`, `card`) — no new colors or hardcoded values. One sandboxed `claude_command()` helper shared by judge + probe (don't duplicate the sandbox comment block).
- **Keep `--safe-mode`, never `--bare`** (`--bare` strips OAuth/keychain auth; `--safe-mode` preserves it).
- **Status taxonomy:** `active` = last tick OK; `error` = per-monitor transient (`claude_timeout`, `hn_error`); `paused` = global (`claude_missing`, `claude_auth`). Only missing/auth flip global health; success clears it; transient errors leave global health unchanged.
- **Rust tests:** run from `src-tauri/` with `cargo test`. Existing suite is 19 tests, all green — keep it green.
- **Frontend checks:** `npx tsc --noEmit && npm run build` from repo root.
- **Env test affordance:** `HN_WATCH_CLAUDE_BIN` overrides the resolved binary for live verification (point at a fake script). Not unit-tested (env vars are process-global and racy under parallel tests) — validated live in Task 9.

---

## File structure

- `src-tauri/src/agent.rs` — `claude_command()` helper, `claude_present()` + env override, `AgentError`, `is_auth_failure`, typed `judge`, `ClaudeHealth`, `classify_auth`, `preflight()`.
- `src-tauri/src/tick.rs` — `TickError`, `run_tick` typed return.
- `src-tauri/src/scheduler.rs` — health handle in `spawn`, health updates from tick results, friendly error string.
- `src-tauri/src/commands.rs` — `AppState.claude_health`, `ClaudeHealthDto`, DTO status from health, `claude_health` + `recheck_claude` commands, async preflight in `init_state`.
- `src-tauri/src/lib.rs` — register the two new commands.
- `src/types.ts` — `ClaudeHealth` interface (`MonitorStatus` already has `paused`).
- `src/api.ts` — `getClaudeHealth`, `recheckClaude`, `onClaudeHealth`.
- `src/components/ClaudeBanner.tsx` — new banner.
- `src/App.tsx` — health state + banner wiring + column layout.
- `src/components/Sidebar.tsx` — `Paused` chip.

---

## Task 1: Sandboxed `claude_command()` helper + `claude_present()` + env override

Pure refactor + new resolver. No behavior change to a working machine; sets up the seam everything else uses.

**Files:**
- Modify: `src-tauri/src/agent.rs` (the `claude_bin` region + `judge`)

**Interfaces:**
- Produces: `fn claude_command() -> tokio::process::Command`; `pub fn claude_present() -> bool`; `claude_bin()` honoring `HN_WATCH_CLAUDE_BIN`.

- [ ] **Step 1: Add the env override + `claude_present()`.** In `src-tauri/src/agent.rs`, replace the `claude_bin` function (lines ~48–54) with:

```rust
/// Env override for tests / live-verification: point at a fake `claude` script.
const CLAUDE_BIN_ENV: &str = "HN_WATCH_CLAUDE_BIN";

/// Resolved absolute path to the `claude` binary. Honors `HN_WATCH_CLAUDE_BIN`
/// (uncached, for tests), else the cached PATH/common-dir resolution, else the
/// bare name "claude".
fn claude_bin() -> String {
    if let Ok(p) = std::env::var(CLAUDE_BIN_ENV) {
        if !p.is_empty() {
            return p;
        }
    }
    static BIN: OnceLock<String> = OnceLock::new();
    BIN.get_or_init(|| find_claude(claude_candidates()).unwrap_or_else(|| "claude".to_string()))
        .clone()
}

/// True when a real `claude` binary exists (not the bare-name fallback). Drives
/// the preflight "Missing" state before we ever try to spawn.
pub fn claude_present() -> bool {
    if let Ok(p) = std::env::var(CLAUDE_BIN_ENV) {
        if !p.is_empty() {
            return std::path::Path::new(&p).exists();
        }
    }
    find_claude(claude_candidates()).is_some()
}

/// Base command carrying the sandbox that keeps any `claude` call from reading
/// the user's files / triggering a macOS TCC prompt: run from the temp dir, override
/// $PWD (claude walks up from $PWD, not getcwd()), null stdin (else it waits ~3s for
/// piped input), kill on drop. Callers append their own args.
fn claude_command() -> tokio::process::Command {
    let workdir = std::env::temp_dir();
    let mut cmd = tokio::process::Command::new(claude_bin());
    cmd.current_dir(&workdir)
        .env("PWD", &workdir)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    cmd
}
```

- [ ] **Step 2: Route `judge` through the helper.** In `judge` (the `tokio::time::timeout(...)` block, lines ~126–137), replace the inline `tokio::process::Command::new(claude_bin())...current_dir/env/stdin/kill_on_drop` construction with `claude_command()`, and delete the now-duplicated sandbox comment block above it (lines ~112–124). The timeout body becomes:

```rust
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        claude_command().arg("-p").arg("--safe-mode").arg(&prompt).output(),
    )
    .await
```

(Leave the `.await ... .map_err(...)` tail and the rest of `judge` unchanged in this task — the typed errors come in Task 2. Remove the now-unused `let workdir = std::env::temp_dir();` line inside `judge`.)

- [ ] **Step 3: Build + run existing tests to confirm no regression.**

Run: `cd src-tauri && cargo test`
Expected: PASS — all existing tests still green (19), no warnings about unused `workdir`.

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/agent.rs
git commit -m "refactor(agent): share sandboxed claude_command() + add claude_present()

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `AgentError` + typed `judge` + auth-stderr classifier

**Files:**
- Modify: `src-tauri/src/agent.rs`

**Interfaces:**
- Consumes: `claude_command()` (Task 1).
- Produces: `pub enum AgentError { NotFound, NotAuthenticated, Timeout, Failed(String) }` with `pub fn code(&self) -> &'static str` and `pub fn message(&self) -> String`; `fn is_auth_failure(stderr: &str) -> bool`; `pub async fn judge(...) -> Result<Vec<Verdict>, AgentError>`.

- [ ] **Step 1: Write the failing tests.** In `src-tauri/src/agent.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn auth_failure_stderr_detected() {
        assert!(is_auth_failure("Not logged in · Please run /login"));
        assert!(is_auth_failure("Invalid API key"));
        assert!(is_auth_failure("Unauthorized"));
        assert!(!is_auth_failure("network error: connection timed out"));
    }

    #[test]
    fn agent_error_codes_and_messages() {
        assert_eq!(AgentError::NotFound.code(), "claude_missing");
        assert_eq!(AgentError::NotAuthenticated.code(), "claude_auth");
        assert_eq!(AgentError::Timeout.code(), "claude_timeout");
        assert_eq!(AgentError::Failed("x".into()).code(), "claude_error");
        assert!(AgentError::Timeout.message().contains("timed out"));
        assert!(AgentError::NotAuthenticated.message().contains("logged in"));
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri && cargo test agent_error_codes_and_messages`
Expected: FAIL — `AgentError` / `is_auth_failure` not defined (compile error).

- [ ] **Step 3: Add the type + classifier + retype `judge`.** In `agent.rs`, above `judge`, add:

```rust
/// A classified failure of a single `claude` call. `code()` is stable and drives
/// paused-vs-error + global health; `message()` is the friendly UI copy.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    NotFound,
    NotAuthenticated,
    Timeout,
    Failed(String),
}

impl AgentError {
    pub fn code(&self) -> &'static str {
        match self {
            AgentError::NotFound => "claude_missing",
            AgentError::NotAuthenticated => "claude_auth",
            AgentError::Timeout => "claude_timeout",
            AgentError::Failed(_) => "claude_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            AgentError::NotFound => "Claude Code was not found on this machine".into(),
            AgentError::NotAuthenticated => "Claude Code isn't logged in".into(),
            AgentError::Timeout => "Claude timed out".into(),
            AgentError::Failed(s) => format!("Claude failed: {s}"),
        }
    }
}

/// Best-effort: does claude's stderr indicate a login / auth problem?
fn is_auth_failure(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("not logged in")
        || s.contains("/login")
        || s.contains("please run")
        || s.contains("authenticate")
        || s.contains("invalid api key")
        || s.contains("unauthorized")
}
```

Then change `judge`'s signature and error mapping. Replace the signature line and the timeout tail + status check:

```rust
pub async fn judge(user_prompt: &str, items: &[HnItem]) -> Result<Vec<Verdict>, AgentError> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let prompt = build_prompt(user_prompt, items);
    let _permit = agent_sem()
        .acquire()
        .await
        .map_err(|e| AgentError::Failed(format!("semaphore closed: {e}")))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        claude_command().arg("-p").arg("--safe-mode").arg(&prompt).output(),
    )
    .await
    .map_err(|_| AgentError::Timeout)?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AgentError::NotFound
        } else {
            AgentError::Failed(format!("failed to spawn claude: {e}"))
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if is_auth_failure(&stderr) {
            AgentError::NotAuthenticated
        } else {
            AgentError::Failed(format!("claude exited with status {}: {stderr}", output.status))
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_verdict(&text))
}
```

(This will break `tick.rs`, which still expects `Result<_, String>` — that's fixed in Task 4. `cargo test` in the agent module still compiles the lib as a whole, so run the targeted check below.)

- [ ] **Step 4: Verify the new unit tests pass (module compiles).** Because `tick.rs` now mismatches, do a scoped check first is not possible (one crate). So proceed to Task 4 before a full `cargo test`. For now verify the code you added is syntactically sound:

Run: `cd src-tauri && cargo build 2>&1 | grep -E "error\[|expected .*String" | head`
Expected: the ONLY errors reference `tick.rs` (judge return type) — none inside `agent.rs`.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/agent.rs
git commit -m "feat(agent): typed AgentError + auth-stderr classifier for judge

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `ClaudeHealth` + `classify_auth` + `preflight()`

**Files:**
- Modify: `src-tauri/src/agent.rs`

**Interfaces:**
- Consumes: `claude_present()`, `claude_command()`.
- Produces: `pub enum ClaudeHealth { Ok, Missing, NotAuthenticated }` with `code()`, `message()`, `is_ok()`; `pub fn classify_auth(success: bool, stdout: &str) -> ClaudeHealth`; `pub async fn preflight() -> ClaudeHealth`.

- [ ] **Step 1: Write the failing tests.** In `agent.rs` `mod tests`:

```rust
    #[test]
    fn classify_auth_states() {
        assert_eq!(classify_auth(true, r#"{"loggedIn":true}"#), ClaudeHealth::Ok);
        assert_eq!(classify_auth(true, r#"{"loggedIn":false}"#), ClaudeHealth::NotAuthenticated);
        assert_eq!(classify_auth(false, ""), ClaudeHealth::NotAuthenticated);
        // unparseable stdout on a zero exit must NOT false-alarm
        assert_eq!(classify_auth(true, "not json at all"), ClaudeHealth::Ok);
    }

    #[test]
    fn claude_health_projection() {
        assert_eq!(ClaudeHealth::Ok.code(), "ok");
        assert_eq!(ClaudeHealth::Missing.code(), "missing");
        assert_eq!(ClaudeHealth::NotAuthenticated.code(), "notAuthenticated");
        assert!(ClaudeHealth::Ok.is_ok());
        assert!(!ClaudeHealth::Missing.is_ok());
        assert!(ClaudeHealth::Missing.message().contains("not found"));
        assert!(ClaudeHealth::NotAuthenticated.message().contains("logged in"));
        assert!(ClaudeHealth::Ok.message().is_empty());
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri && cargo test classify_auth_states 2>&1 | grep -E "cannot find|error\[" | head`
Expected: FAIL — `classify_auth` / `ClaudeHealth` not found.

- [ ] **Step 3: Implement.** In `agent.rs` add:

```rust
/// Global Claude availability, seeded at startup and kept live by ticks.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeHealth {
    Ok,
    Missing,
    NotAuthenticated,
}

impl ClaudeHealth {
    pub fn code(&self) -> &'static str {
        match self {
            ClaudeHealth::Ok => "ok",
            ClaudeHealth::Missing => "missing",
            ClaudeHealth::NotAuthenticated => "notAuthenticated",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ClaudeHealth::Ok => String::new(),
            ClaudeHealth::Missing => {
                "Claude Code not found — HN Watch needs it to judge stories. \
                 Install Claude Code, then Re-check."
                    .into()
            }
            ClaudeHealth::NotAuthenticated => {
                "Claude Code isn't logged in — run `claude` in a terminal to log in, \
                 then Re-check."
                    .into()
            }
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, ClaudeHealth::Ok)
    }
}

/// Pure: map `claude auth status --json` output to a health state.
/// non-zero exit → NotAuthenticated (logged-out exits 1); exit 0 with
/// `{"loggedIn": false}` → NotAuthenticated; anything else on exit 0 → Ok
/// (unparseable output must not false-alarm).
pub fn classify_auth(success: bool, stdout: &str) -> ClaudeHealth {
    if !success {
        return ClaudeHealth::NotAuthenticated;
    }
    match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(v) if v.get("loggedIn").and_then(|b| b.as_bool()) == Some(false) => {
            ClaudeHealth::NotAuthenticated
        }
        _ => ClaudeHealth::Ok,
    }
}

/// Startup / re-check probe. Cheap: `claude auth status --json` makes NO model
/// call. Binary absent → Missing without spawning; probe that itself fails to
/// run / times out → Ok (don't false-alarm — real ticks surface genuine errors).
pub async fn preflight() -> ClaudeHealth {
    if !claude_present() {
        return ClaudeHealth::Missing;
    }
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        claude_command().arg("auth").arg("status").arg("--json").output(),
    )
    .await;
    match probe {
        Ok(Ok(output)) => {
            classify_auth(output.status.success(), &String::from_utf8_lossy(&output.stdout))
        }
        _ => ClaudeHealth::Ok,
    }
}
```

- [ ] **Step 4: Verify these tests compile & pass in isolation.** (Full `cargo test` still blocked by `tick.rs` until Task 4.)

Run: `cd src-tauri && cargo build 2>&1 | grep -E "error\[" | grep agent.rs | head`
Expected: no output (no errors originating in `agent.rs`).

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/agent.rs
git commit -m "feat(agent): ClaudeHealth + no-token auth-status preflight probe

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `TickError` + typed `run_tick`

**Files:**
- Modify: `src-tauri/src/tick.rs`

**Interfaces:**
- Consumes: `agent::AgentError`, `agent::judge` (now `Result<_, AgentError>`).
- Produces: `pub enum TickError { Hn(String), Agent(agent::AgentError), Db(String) }` with `code()`/`message()`; `pub async fn run_tick(...) -> Result<TickOutcome, TickError>`.

- [ ] **Step 1: Write the failing test.** In `src-tauri/src/tick.rs` `mod tests`:

```rust
    #[test]
    fn tick_error_projection() {
        assert_eq!(TickError::Hn("boom".into()).code(), "hn_error");
        assert_eq!(TickError::Db("x".into()).code(), "db_error");
        assert_eq!(
            TickError::Agent(agent::AgentError::NotAuthenticated).code(),
            "claude_auth"
        );
        assert!(TickError::Hn("boom".into()).message().contains("Hacker News"));
        assert_eq!(
            TickError::Agent(agent::AgentError::Timeout).message(),
            agent::AgentError::Timeout.message()
        );
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri && cargo test tick_error_projection 2>&1 | grep -E "cannot find|error\[" | head`
Expected: FAIL — `TickError` not defined.

- [ ] **Step 3: Implement `TickError` + retype `run_tick`.** In `tick.rs`, after the `TickOutcome` struct, add:

```rust
/// A classified tick failure. `code()` feeds paused-vs-error + global health;
/// `message()` is the friendly reason stored in `last_error`.
#[derive(Debug)]
pub enum TickError {
    Hn(String),
    Agent(agent::AgentError),
    Db(String),
}

impl TickError {
    pub fn code(&self) -> &'static str {
        match self {
            TickError::Hn(_) => "hn_error",
            TickError::Agent(a) => a.code(),
            TickError::Db(_) => "db_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            TickError::Hn(_) => "Couldn't reach Hacker News".into(),
            TickError::Agent(a) => a.message(),
            TickError::Db(e) => format!("Local database error: {e}"),
        }
    }
}
```

Then change `run_tick` to map every `?` into a `TickError`:

```rust
pub async fn run_tick(
    db: &Arc<Mutex<Connection>>,
    monitor: &Monitor,
) -> Result<TickOutcome, TickError> {
    let recent = hn::fetch_recent(30).await.map_err(TickError::Hn)?;
    let checked = recent.len();

    let seen = {
        let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
        db::list_seen(&conn, &monitor.id).map_err(|e| TickError::Db(e.to_string()))?
    };
    let unseen = select_unseen(recent, &seen);
    if unseen.is_empty() {
        return Ok(TickOutcome { checked, new: 0 });
    }

    let verdicts = agent::judge(&monitor.prompt, &unseen)
        .await
        .map_err(TickError::Agent)?;
    let rows = build_feed_rows(&monitor.id, &unseen, &verdicts, now_secs());

    let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
    for row in &rows {
        db::insert_feed_item(&conn, row).map_err(|e| TickError::Db(e.to_string()))?;
    }
    for item in &unseen {
        db::mark_seen(&conn, &monitor.id, &item.hn_id).map_err(|e| TickError::Db(e.to_string()))?;
    }
    Ok(TickOutcome { checked, new: rows.len() })
}
```

(`scheduler.rs` still calls the old `run_tick` shape — fixed in Task 5. This task leaves the crate not-yet-compiling at the scheduler; that's expected and resolved next.)

- [ ] **Step 4: Confirm the only remaining errors are in `scheduler.rs`.**

Run: `cd src-tauri && cargo build 2>&1 | grep -E "error\[" | grep -oE "src/[a-z]+\.rs" | sort -u`
Expected: only `src/scheduler.rs`.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/tick.rs
git commit -m "feat(tick): typed TickError (hn / agent / db) with friendly messages

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Scheduler threads health + updates it from tick results

**Files:**
- Modify: `src-tauri/src/scheduler.rs`

**Interfaces:**
- Consumes: `tick::run_tick -> Result<TickOutcome, TickError>`, `agent::ClaudeHealth`. This task emits its `claude-health` payload via a **local** `ClaudeHealthPayload` struct (defined here) built from `ClaudeHealth::code()/message()` — no dependency on Task 6's DTO, so task order is safe.
- Produces: `Scheduler::spawn(&self, app, db, health: Arc<Mutex<ClaudeHealth>>, monitor)`.

- [ ] **Step 1: Add the import + a health-payload struct.** At the top of `scheduler.rs`, add ONE new `use` line (do **not** re-import `Arc`/`Mutex` — line 4 `use std::sync::{Arc, Mutex};` already provides them):

```rust
use crate::agent::ClaudeHealth;
```

And add a serialized payload next to the other event structs:

```rust
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeHealthPayload {
    status: String,
    message: String,
}
```

- [ ] **Step 2: Change `spawn` to accept and update health.** Replace the whole `spawn` method with:

```rust
    /// Spawn a long-lived worker: tick immediately, then every `interval_secs`.
    /// `health` is the shared global Claude state — this worker flips it to
    /// Missing/NotAuthenticated on those failures and clears it on success.
    pub fn spawn(
        &self,
        app: AppHandle,
        db: Arc<Mutex<Connection>>,
        health: Arc<Mutex<ClaudeHealth>>,
        monitor: Monitor,
    ) {
        let interval = Duration::from_secs(monitor.interval_secs.max(1) as u64);
        let id = monitor.id.clone();
        let handle = tauri::async_runtime::spawn(async move {
            loop {
                let _ = app.emit("tick-started", TickStarted { monitor_id: monitor.id.clone() });

                let result = tick::run_tick(&db, &monitor).await;
                let now = tick::now_secs();
                let (checked, new, error, code) = match &result {
                    Ok(o) => (o.checked as i64, o.new as i64, None, None),
                    Err(e) => {
                        eprintln!(
                            "[hn-watch] tick failed for {}: {} ({})",
                            monitor.id,
                            e.message(),
                            e.code()
                        );
                        (0i64, 0i64, Some(e.message()), Some(e.code()))
                    }
                };

                match db.lock() {
                    Ok(conn) => {
                        if let Err(e) = db::record_tick(
                            &conn,
                            &monitor.id,
                            checked,
                            new,
                            error.as_deref(),
                            now,
                        ) {
                            eprintln!("[hn-watch] record_tick failed for {}: {e}", monitor.id);
                        }
                    }
                    Err(_) => {
                        eprintln!("[hn-watch] db poisoned; skipped record_tick for {}", monitor.id)
                    }
                }

                // Global Claude health: only claude_missing / claude_auth flip it;
                // success clears it; a transient error leaves it unchanged.
                if let Ok(mut h) = health.lock() {
                    let next = match code {
                        Some("claude_missing") => ClaudeHealth::Missing,
                        Some("claude_auth") => ClaudeHealth::NotAuthenticated,
                        None => ClaudeHealth::Ok,
                        _ => h.clone(),
                    };
                    if *h != next {
                        *h = next.clone();
                        let _ = app.emit(
                            "claude-health",
                            ClaudeHealthPayload {
                                status: next.code().into(),
                                message: next.message(),
                            },
                        );
                    }
                }

                if new > 0 {
                    let _ = app.emit("feed-updated", ());
                }
                let _ = app.emit(
                    "tick-finished",
                    TickFinished {
                        monitor_id: monitor.id.clone(),
                        checked_count: checked,
                        new_count: new,
                        error,
                    },
                );

                tokio::time::sleep(interval).await;
            }
        });
        self.handles.lock().unwrap().insert(id, handle);
    }
```

- [ ] **Step 3: Confirm remaining errors are only in `commands.rs` (spawn call sites).**

Run: `cd src-tauri && cargo build 2>&1 | grep -E "error\[" | grep -oE "src/[a-z]+\.rs" | sort -u`
Expected: only `src/commands.rs`.

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/scheduler.rs
git commit -m "feat(scheduler): thread shared ClaudeHealth; flip paused, clear on success

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Commands — health state, DTO status, new commands, async preflight

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `agent::{ClaudeHealth, preflight}`, `Scheduler::spawn(app, db, health, monitor)`.
- Produces: `AppState.claude_health`, `pub struct ClaudeHealthDto { status, message }` with `from_health`, commands `claude_health` + `recheck_claude`; monitor `status:"paused"` when health down.

- [ ] **Step 1: Write the failing test for DTO status.** In `commands.rs` `mod tests`:

```rust
    #[test]
    fn status_paused_overrides_error_when_claude_down() {
        use crate::db::Monitor;
        let c = Connection::open_in_memory().unwrap();
        db::migrate(&c).unwrap();
        let mut m = Monitor {
            id: "m1".into(), name: "n".into(), prompt: "p".into(),
            interval_secs: 1800, created_at: 1,
            last_checked_at: Some(10), last_checked_count: Some(5),
            last_new_count: Some(0), last_error: None,
        };
        db::insert_monitor(&c, &m).unwrap();
        // Claude down → paused, regardless of last_error
        assert_eq!(to_monitor_dto(&c, &m, false).unwrap().status, "paused");
        // Claude ok, no error → active
        assert_eq!(to_monitor_dto(&c, &m, true).unwrap().status, "active");
        // Claude ok, error set → error
        m.last_error = Some("Claude timed out".into());
        assert_eq!(to_monitor_dto(&c, &m, true).unwrap().status, "error");
    }
```

- [ ] **Step 2: Run to verify failure.**

Run: `cd src-tauri && cargo test status_paused_overrides_error 2>&1 | grep -E "error\[|arguments" | head`
Expected: FAIL — `to_monitor_dto` takes 2 args, not 3.

- [ ] **Step 3: Add imports, the DTO, and retype `to_monitor_dto`.** In `commands.rs`:

At the top, extend the imports:

```rust
use crate::agent::{self, ClaudeHealth};
use crate::db::{self, Monitor};
use crate::scheduler::Scheduler;
use crate::tick::now_secs;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
```

Add the health DTO near `MonitorDto`:

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeHealthDto {
    pub status: String,
    pub message: String,
}

impl ClaudeHealthDto {
    pub fn from_health(h: &ClaudeHealth) -> Self {
        ClaudeHealthDto { status: h.code().into(), message: h.message() }
    }
}
```

Replace `to_monitor_dto` with a version taking `claude_ok`:

```rust
fn to_monitor_dto(conn: &Connection, m: &Monitor, claude_ok: bool) -> rusqlite::Result<MonitorDto> {
    let status = if !claude_ok {
        "paused"
    } else if m.last_error.is_some() {
        "error"
    } else {
        "active"
    };
    Ok(MonitorDto {
        id: m.id.clone(),
        name: m.name.clone(),
        prompt: m.prompt.clone(),
        interval_label: interval_label(m.interval_secs),
        status: status.into(),
        match_count: db::count_matches(conn, &m.id)?,
        last_checked_at: m.last_checked_at,
        next_check_at: next_check_at(m.last_checked_at, m.interval_secs),
        last_checked_count: m.last_checked_count,
        last_new_count: m.last_new_count,
        last_error: m.last_error.clone(),
    })
}
```

- [ ] **Step 4: Add `claude_health` to `AppState` and update `list_monitors` / `create_monitor`.** Change the struct:

```rust
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub scheduler: Scheduler,
    pub claude_health: Arc<Mutex<ClaudeHealth>>,
}
```

Update `list_monitors` (read health first — consistent lock order health→db):

```rust
#[tauri::command]
pub fn list_monitors(state: State<'_, AppState>) -> Result<Vec<MonitorDto>, String> {
    let claude_ok = state
        .claude_health
        .lock()
        .map_err(|_| "health poisoned".to_string())?
        .is_ok();
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    let monitors = db::list_monitors(&conn).map_err(|e| e.to_string())?;
    monitors
        .iter()
        .map(|m| to_monitor_dto(&conn, m, claude_ok).map_err(|e| e.to_string()))
        .collect()
}
```

Update `create_monitor`'s DTO build + spawn call:

```rust
    let claude_ok = state
        .claude_health
        .lock()
        .map_err(|_| "health poisoned".to_string())?
        .is_ok();
    let dto = {
        let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
        db::insert_monitor(&conn, &monitor).map_err(|e| e.to_string())?;
        to_monitor_dto(&conn, &monitor, claude_ok).map_err(|e| e.to_string())?
    };
    state
        .scheduler
        .spawn(app, Arc::clone(&state.db), Arc::clone(&state.claude_health), monitor);
    Ok(dto)
```

- [ ] **Step 5: Add the two commands.** After `delete_monitor`:

```rust
#[tauri::command]
pub fn claude_health(state: State<'_, AppState>) -> Result<ClaudeHealthDto, String> {
    let h = state.claude_health.lock().map_err(|_| "health poisoned".to_string())?;
    Ok(ClaudeHealthDto::from_health(&h))
}

#[tauri::command]
pub async fn recheck_claude(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ClaudeHealthDto, String> {
    let health = agent::preflight().await; // no MutexGuard held across await
    {
        let mut h = state.claude_health.lock().map_err(|_| "health poisoned".to_string())?;
        *h = health.clone();
    }
    let dto = ClaudeHealthDto::from_health(&health);
    let _ = app.emit("claude-health", dto.clone());
    Ok(dto)
}
```

- [ ] **Step 6: Wire health + async preflight into `init_state`.** Replace `init_state`:

```rust
/// Called once at startup: open/create the DB, spawn a worker per monitor, and
/// kick off an async Claude preflight (never blocks the window).
pub fn init_state(app: &AppHandle) -> AppState {
    let dir = app.path().app_data_dir().expect("no app data dir");
    std::fs::create_dir_all(&dir).ok();
    let conn = Connection::open(dir.join("hn-watch.sqlite")).expect("open db");
    db::migrate(&conn).expect("migrate db");
    let db = Arc::new(Mutex::new(conn));
    let scheduler = Scheduler::new();
    let claude_health = Arc::new(Mutex::new(ClaudeHealth::Ok));

    let existing = {
        let conn = db.lock().unwrap();
        db::list_monitors(&conn).unwrap_or_default()
    };
    for m in existing {
        scheduler.spawn(app.clone(), Arc::clone(&db), Arc::clone(&claude_health), m);
    }

    // Startup preflight, async so the window opens immediately.
    {
        let app = app.clone();
        let health = Arc::clone(&claude_health);
        tauri::async_runtime::spawn(async move {
            let result = agent::preflight().await;
            if let Ok(mut h) = health.lock() {
                *h = result.clone();
            }
            let _ = app.emit("claude-health", ClaudeHealthDto::from_health(&result));
        });
    }

    AppState { db, scheduler, claude_health }
}
```

- [ ] **Step 7: Register the commands in `lib.rs`.** In `src-tauri/src/lib.rs`, extend the `generate_handler!` list:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::create_monitor,
            commands::list_monitors,
            commands::delete_monitor,
            commands::list_feed,
            commands::claude_health,
            commands::recheck_claude,
        ])
```

- [ ] **Step 8: Full build + test — the whole crate compiles and all tests pass.**

Run: `cd src-tauri && cargo test`
Expected: PASS — the crate builds clean; the existing 19 tests plus the new ones (`auth_failure_stderr_detected`, `agent_error_codes_and_messages`, `classify_auth_states`, `claude_health_projection`, `tick_error_projection`, `status_paused_overrides_error_when_claude_down`) all green. No warnings.

- [ ] **Step 9: Commit.**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): shared ClaudeHealth, paused status, claude_health + recheck_claude, async preflight

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Frontend types + api

**Files:**
- Modify: `src/types.ts`, `src/api.ts`

**Interfaces:**
- Produces: `ClaudeHealth` type; `getClaudeHealth()`, `recheckClaude()`, `onClaudeHealth(cb)`.

- [ ] **Step 1: Add the `ClaudeHealth` type.** In `src/types.ts`, after the `MonitorStatus` type (which already includes `"paused"`), add:

```ts
export interface ClaudeHealth {
  status: "ok" | "missing" | "notAuthenticated";
  message: string;
}
```

- [ ] **Step 2: Add the api wrappers + listener.** In `src/api.ts`, update the import and append:

```ts
import type { Monitor, FeedItem, ClaudeHealth } from "./types";
```

```ts
// Current Claude availability (drives the top banner + paused status).
export const getClaudeHealth = () => invoke<ClaudeHealth>("claude_health");

// Re-run the startup preflight on demand (banner "Re-check" button).
export const recheckClaude = () => invoke<ClaudeHealth>("recheck_claude");

// Fires when Claude health changes (preflight, recheck, or a tick flip).
export const onClaudeHealth = (cb: (h: ClaudeHealth) => void) =>
  listen<ClaudeHealth>("claude-health", (e) => cb(e.payload));
```

- [ ] **Step 3: Typecheck.**

Run: `npx tsc --noEmit`
Expected: PASS — no errors.

- [ ] **Step 4: Commit.**

```bash
git add src/types.ts src/api.ts
git commit -m "feat(ui): ClaudeHealth type + health api wrappers/listener

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: ClaudeBanner component + App wiring

**Files:**
- Create: `src/components/ClaudeBanner.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `getClaudeHealth`, `recheckClaude`, `onClaudeHealth`, `ClaudeHealth`.
- Produces: `<ClaudeBanner health onRecheck rechecking />`; App renders it above the main row.

- [ ] **Step 1: Create the banner.** Write `src/components/ClaudeBanner.tsx`:

```tsx
import type { ClaudeHealth } from "../types";

export function ClaudeBanner({
  health,
  onRecheck,
  rechecking,
}: {
  health: ClaudeHealth;
  onRecheck: () => void;
  rechecking: boolean;
}) {
  if (health.status === "ok") return null;
  return (
    <div className="flex items-center gap-3 border-b border-hn-border bg-hn-soft px-6 py-2.5">
      <span className="h-2 w-2 shrink-0 rounded-full bg-rust" />
      <p className="min-w-0 flex-1 text-[12.5px] leading-snug text-soft">{health.message}</p>
      <button
        onClick={onRecheck}
        disabled={rechecking}
        className="shrink-0 rounded-md border border-hn-border bg-card px-3 py-1 text-[12px] font-semibold text-rust transition-colors hover:bg-paper disabled:opacity-50"
      >
        {rechecking ? "Checking…" : "Re-check"}
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Wire health state into `App.tsx`.** Add imports:

```tsx
import type { ClaudeHealth, FeedItem, Monitor } from "./types";
import { ClaudeBanner } from "./components/ClaudeBanner";
```

```tsx
import {
  listMonitors,
  listFeed,
  createMonitor,
  deleteMonitor,
  onFeedUpdated,
  onTickStarted,
  onTickFinished,
  getClaudeHealth,
  recheckClaude,
  onClaudeHealth,
} from "./api";
```

Add state (near the other `useState`s):

```tsx
  const [health, setHealth] = useState<ClaudeHealth>({ status: "ok", message: "" });
  const [rechecking, setRechecking] = useState(false);
```

In the mount `useEffect`, seed + subscribe to health (health changes also flip monitor `status`, so refresh monitors when it changes):

```tsx
    getClaudeHealth().then(setHealth);
    const uHealth = onClaudeHealth((h) => {
      setHealth(h);
      listMonitors().then(setMonitors);
    });
```

and add `uHealth.then((f) => f());` to the cleanup returned by the effect.

Add the recheck handler (near `handleCreate`):

```tsx
  const handleRecheck = async () => {
    setRechecking(true);
    try {
      setHealth(await recheckClaude());
      await refresh();
    } finally {
      setRechecking(false);
    }
  };
```

- [ ] **Step 3: Restructure the layout to a column with the banner on top.** Replace the `return (...)` block:

```tsx
  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <ClaudeBanner health={health} onRecheck={handleRecheck} rechecking={rechecking} />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <Sidebar
          monitors={monitors}
          selectedId={selectedMonitorId}
          onSelect={setSelectedMonitorId}
          onCreate={handleCreate}
          onDelete={handleDelete}
          now={now}
          checkingIds={checkingIds}
        />

        <Feed
          items={visibleFeed}
          monitors={monitors}
          activeMonitor={activeMonitor}
          onDigDeeper={setDigItem}
        />

        {digItem && (
          <DigDeeperPanel
            item={digItem}
            brief={digItem.id === BRIEF_F1.itemId ? BRIEF_F1 : null}
            onClose={() => setDigItem(null)}
          />
        )}
      </div>
    </div>
  );
```

- [ ] **Step 4: Typecheck + build.**

Run: `npx tsc --noEmit && npm run build`
Expected: PASS — clean typecheck and Vite build.

- [ ] **Step 5: Commit.**

```bash
git add src/components/ClaudeBanner.tsx src/App.tsx
git commit -m "feat(ui): Claude health banner with Re-check, wired into App

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Sidebar `Paused` chip + live verification

**Files:**
- Modify: `src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `Monitor.status` (now emits `"paused"`).

- [ ] **Step 1: Add the `Paused` chip branch.** In `src/components/Sidebar.tsx`, in `MonitorRow`, extend the `chip` chain — insert a `paused` branch between `checking` and `error`:

```tsx
  const chip = checking ? (
    <span className="flex shrink-0 items-center gap-1 rounded-full bg-hn-soft px-2 py-0.5 font-mono text-[10px] text-rust">
      <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-rust" />
      Checking…
    </span>
  ) : monitor.status === "paused" ? (
    <span className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[10px] text-faint">
      Paused
    </span>
  ) : monitor.status === "error" ? (
    <span
      title={monitor.lastError ?? ""}
      className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[10px] text-rust"
    >
      error
    </span>
  ) : (
    <span className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[10px] text-faint">
      {fmtCountdown(monitor.nextCheckAt, now)}
    </span>
  );
```

(The status dot already maps `paused` → `bg-faint` via the existing `STATUS_DOT` map — no change needed there.)

- [ ] **Step 2: Typecheck + build.**

Run: `npx tsc --noEmit && npm run build`
Expected: PASS.

- [ ] **Step 3: Commit.**

```bash
git add src/components/Sidebar.tsx
git commit -m "feat(ui): Paused chip on monitors when Claude is unavailable

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: Live verification in the native window** (per `docs/TESTING.md` — build/launch the native app, drive with computer-use; never localhost). Use the `HN_WATCH_CLAUDE_BIN` env override to force each state without logging out:

  1. **Missing:** launch with `HN_WATCH_CLAUDE_BIN=/no/such/claude`. Expect: red-dot banner "Claude Code not found …", every monitor shows a `Paused` chip + grey dot.
  2. **Not logged in:** create a fake script that makes `auth status` exit 1, e.g. write `/tmp/fake-claude` containing:
     ```sh
     #!/bin/sh
     if [ "$1" = "auth" ]; then echo "Not logged in · Please run /login" >&2; exit 1; fi
     exit 1
     ```
     `chmod +x /tmp/fake-claude`, launch with `HN_WATCH_CLAUDE_BIN=/tmp/fake-claude`. Expect: "isn't logged in …" banner + `Paused` chips.
  3. **Re-check clears it:** with the app open in a failed state, quit/relaunch with the real binary (no env override) and click **Re-check** on the banner → banner disappears, monitors return to `active` countdowns.
  4. **Healthy run:** normal launch (real `claude`, logged in) → no banner; a created monitor ticks, shows `Checking…` then `active` + matches.
  5. **Transient error stays per-monitor:** (optional) confirm an HN-only failure would show a per-row `error` chip but **no** global banner — code-review this path if not reproducible live.

  Capture a screenshot of the Missing/Not-logged-in banner state for the session log.

- [ ] **Step 5: Update `STATUS.md` and `docs/TODO.md`.** Add a "Session 5 — Error handling + preflight (TODO #3)" entry to `STATUS.md` summarizing what shipped and the live-verification results; mark TODO #3 ✅ SHIPPED in `docs/TODO.md` (mirror the #1 style). Commit:

```bash
git add STATUS.md docs/TODO.md
git commit -m "docs: STATUS + TODO for Session 5 (error handling + preflight)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final integration gate (after all tasks)

- [ ] `cd src-tauri && cargo test` → all green.
- [ ] `npx tsc --noEmit && npm run build` → clean.
- [ ] Live states 1–4 verified in the native window (Task 9 Step 4).
- [ ] Whole-branch review (superpowers:requesting-code-review) before merging `feat/error-handling-preflight` → `main` (`--no-ff`), push, keep the branch on origin.
