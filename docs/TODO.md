# HN Watch — TODO (deferred enhancements)

Prioritized backlog of things we deliberately deferred. **Nothing here is started.**
We will pick these up **one at a time** in upcoming sessions. Each item has enough
detail to act on cold. Source of truth for scope remains [`docs/REQUIREMENTS.md`](REQUIREMENTS.md).

---

## 1. Tick observability — the app must show what it's doing  ✅ SHIPPED (Session 4)

> Done on `feat/tick-observability` — live `next in Xm` countdown, `Checking…`/`error` chips, a
> `Last checked H:MM · scanned · new` status line, and a check-aware feed empty-state, backed by
> persisted per-tick results. Paused/catching-up chips remain deferred to #3/#4. The rest of this
> entry is kept for history. See `STATUS.md` (Session 4) and
> `docs/superpowers/specs/2026-07-09-tick-observability-design.md`.

**Problem.** Today a monitor is a black box. When you create one there is no loading
indicator, no "checking…" state, no "last checked" time, and no "0 new" message. If a tick
finds nothing (or fails), the feed is just blank with no explanation — you cannot tell
"working, nothing matched" from "broken." (We confirmed this live: a "Claude" monitor
correctly checked 30 stories and matched 0, but the UI showed nothing, so it *looked* dead.)

**Why it matters.** A watchtower that gives zero feedback feels broken even when it works.
This was the single biggest source of "is it even running?" confusion.

**Current behavior.** A tick runs HN fetch → `claude` → insert matches, and only
`eprintln!`s to stderr on failure. The UI never learns a tick started, finished, or failed.

**Proposed approach.**
- Backend: emit tick-lifecycle events (e.g. `tick-started` / `tick-finished`) carrying
  `{ monitor_id, checked_count, new_count, error? }`. Persist `last_checked_at` **and expose a
  wall-clock `next_check_at`** per monitor (see TODO #4 — the schedule must be wall-clock based
  so the countdown stays correct across sleep/restart).
- UI: per-monitor **live countdown to the next check** — "next in 25m", ticking down
  "24m… 23m…" (a small client-side timer against `next_check_at`), plus a status line —
  "Last checked 3:31 PM · checked 30 · 0 new".
- **Status chips** per monitor:
  - **Checking…** while a tick runs.
  - **Paused** when the app is open but ticks can't proceed (offline / Claude unavailable —
    ties into TODO #3).
  - **Resumed · catching up** transient state right after a laptop wake pushed a monitor
    overdue (ties into TODO #4), then back to the normal countdown.
- Empty-feed message: "Checked N stories, nothing matched yet" instead of a blank pane.

**Acceptance.** From the UI alone you can always tell: a live countdown to the next check; is it
checking now; when it last checked; how many stories it looked at; how many were new; whether the
last tick errored; and whether it's paused or catching up after a wake.

---

## 2. Lossless ingestion under variable volume — never miss, never re-analyze  ✅ SHIPPED (Session 6)

> Done on `feat/lossless-ingestion`. Per-monitor nullable `watermark` (additive migration) replaces the
> fixed newest-30 fetch; each tick pulls everything since `watermark.unwrap_or(now − 1h)` via paginated
> `hn::fetch_since` (`created_at_i>=W`, 100/page, 10-page/1000-hit cap logged if hit). Unseen set judged in
> fail-closed `claude` batches of ≤30 (sequential within a tick). Watermark advances to `max(created_at) −
> 5min` (monotonic, absurd-ts-guarded); the 5-min margin covers Algolia's async indexing. Commit order
> insert → seen → watermark (last) = crash-safe without a transaction. Dedup (`seen`+`UNIQUE`) unchanged. A
> **Critical** whole-branch-review find was fixed (`7d47997`): the scheduler reused a frozen `Monitor` so
> the persisted watermark was never read back in-session (permanent miss window at 1h intervals) — `run_tick`
> now returns the watermark and the worker carries it forward. Live-verified: carry-forward (`since`
> advances between ticks), burst (167 stories / 2 pages / 30-30-25 batches / no dup cards), fail-closed (162
> fetched, judge fails → nothing committed, watermark unadvanced). See `STATUS.md` (Session 6),
> `docs/superpowers/specs/2026-07-09-lossless-ingestion-design.md`, and the plan. The rest of this entry is
> kept for history.

**Problem.** Between two 30-min ticks the number of new HN stories varies a lot — sometimes
5, sometimes 200, sometimes 100. We must guarantee **(a) no story is missed** and
**(b) no story is analyzed twice**, regardless of that volume.

**Current behavior.**
- **Duplication (b): already solved.** The per-monitor `seen` table (persisted in SQLite) +
  `UNIQUE(monitor_id, hn_id)` on `feed_items` mean a story is sent to `claude` at most once
  per monitor, ever — even across restarts. This part is robust; keep it.
- **Missing (a): a real gap.** Each tick fetches only the **30 newest** stories
  (`hn::fetch_recent(30)` → Algolia `search_by_date`). If a burst of >30 new stories lands
  between ticks, stories beyond the newest 30 are never fetched → never seen → permanently
  missed. In practice HN's story rate is well under 30/30-min, so it rarely bites, but the
  fixed cap does **not guarantee completeness**.

**Proposed approach (make it bulletproof at any volume).**
- **Watermark, not a fixed count.** Store the newest processed HN submission timestamp per
  monitor. Each tick, request everything submitted since that watermark
  (`numericFilters=created_at_i>watermark`) and **paginate** (Algolia `page=0,1,2…`) until you
  reach already-seen / pre-watermark stories. This pulls the exact new window — 5 or 5,000 —
  with nothing skipped.
- **Chunk the `claude` calls.** Never jam a burst into one giant prompt. Split the unseen set
  into batches of ~30 and run them as separate `claude` calls, bounded by the existing shared
  semaphore. (This is the deeper reason ~30 is a good number — it's a sane per-call batch size,
  not a coverage limit.)
- Keep `seen` + `UNIQUE` exactly as-is for dedup.

**Acceptance.** Simulate/observe a high-volume window: every story submitted since the last
tick appears (paginated in), each is analyzed exactly once, and no duplicate feed cards —
independent of how many arrived.

---

## 3. Robust error handling + mandatory Claude Code startup check  ✅ SHIPPED (Session 5)

> Done on `feat/error-handling-preflight`. Startup preflight via a no-token `claude auth status --json`
> probe (binary-absent → Missing without spawning); typed `AgentError`/`TickError` with stable `code()` +
> friendly `message()`; a shared `Arc<Mutex<ClaudeHealth>>` seeded by preflight and kept live by ticks;
> DTO `status:"paused"` (global) distinct from per-monitor transient `error`; a top banner with a
> **Re-check** button + the `Paused` chip. Two Important health-transition bugs found by the whole-branch
> review + live verification were fixed (early-return tick no longer clears a down-state; recovery clears
> stale per-monitor errors). Live-verified across missing / logged-out / Re-check-recovery. `Resumed ·
> catching up` chip remains deferred to #4. See `STATUS.md` (Session 5),
> `docs/superpowers/specs/2026-07-09-error-handling-preflight-design.md`, and the plan. The rest of this
> entry is kept for history.

**Problem.** `claude` is mandatory — it's the whole engine (relevance judging + summaries).
Right now, if the user hasn't installed Claude Code, or isn't logged in, or `claude` errors,
the tick fails silently (stderr only) and the feed is just empty with no explanation.

**Why it matters.** A person who clones/installs the app without a working, authenticated
`claude` gets a silent, broken-looking app. The app should tell them clearly.

**Proposed approach.**
- **Startup preflight (first thing on launch).** Before/while spawning workers, check:
  1. Is the `claude` binary present? (reuse the existing `claude_bin()` resolver.)
  2. Is it authenticated / usable? (e.g. a quick probe call, or detect the
     "Not logged in · Please run /login" response.)
  If either fails, surface a clear, blocking-but-friendly banner: "Claude Code not found —
  install it and run `claude` to log in" / "Claude Code is not logged in — run `claude`".
- **Per-tick error surfacing.** Distinguish and display the failure modes instead of silence:
  `claude` missing, `claude` not authenticated, `claude` timed out, HN fetch failed, bad/empty
  verdict. Show the reason on the monitor (ties into TODO #1's status line) rather than only
  logging to stderr.
- Keep the current graceful behavior (a failed tick never kills the worker) — just make it
  *visible*.

**Acceptance.** On a machine without Claude Code (or logged out), the app opens and immediately
tells the user exactly what's wrong and how to fix it — no silent empty feed. Every tick failure
mode shows a human-readable reason in the UI.

---

## 4. Sleep/suspend & catch-up scheduling (wall-clock, not monotonic)

**Problem.** The current scheduler sleeps on a monotonic timer (`tokio::time::sleep`). On macOS
`Instant` uses `CLOCK_UPTIME_RAW`, which **does not advance while the machine is asleep**, and
Tokio's timers are **paused during OS sleep** (verified: tokio issue #2784). So if you start an
"every 30m" monitor, close the lid 5 min in, and reopen 45 min later, the 45 minutes of sleep
don't count — the next tick fires ~25 min *after reopening*, not on wake and not during sleep.
The schedule silently stretches by however long the laptop was asleep, and after a long sleep
the fixed-30 fetch (TODO #2) can also miss stories.

Inconsistency to fix too: **quitting** the app → each monitor ticks immediately on relaunch
(catch-up), but **suspend→wake** does not catch up. Both should behave the same.

**Proposed approach (unify normal / restart / wake under one rule: "tick anything overdue").**
- Schedule off **persisted wall-clock time**, not a monotonic sleep: store `last_checked_at`
  (`SystemTime`) per monitor; the next due time is `last_checked_at + interval`. Wall-clock moves
  forward across suspend, so overdue-ness is computed correctly after a wake.
- On **app start** and on **resume-from-sleep** (macOS `NSWorkspace` didWake via Tauri, or detect
  a large gap on the next wake), re-evaluate all monitors and run a catch-up tick for any overdue.
- Guard the wall-clock delta against absurd negative/huge jumps (NTP/manual clock changes).
- Consider `tokio::time::interval` + `MissedTickBehavior::Skip` for the in-app cadence (drift-free,
  explicit no-overlap) — see the scheduling research note; but the durable fix is the wall-clock
  `last_checked` model above, which also subsumes restart behavior.
- Feeds directly into TODO #1's UI: expose `next_check_at` so the UI shows a live countdown, and a
  transient "Resumed · catching up" state after a wake.
- Pair with TODO #2 (watermark + pagination) so a catch-up tick after a long sleep doesn't drop
  stories.

**Acceptance.** Close the lid mid-interval, reopen after > one interval → the monitor checks
promptly on wake (catch-up), the UI countdown reflects reality immediately, no duplicate analysis,
and (with #2) no stories missed. Suspend→wake and quit→relaunch behave identically.

---

_Order to tackle: **#1 (observability) done (Session 4); #3 (error handling / preflight) done (Session 5);
#2 (lossless ingestion) done (Session 6).** Remaining: **#4 (sleep/wake catch-up)** — makes the schedule
trustworthy on a laptop, shares plumbing with #1, adds the `Resumed · catching up` chip; its catch-up tick is
already lossless now that ingestion is watermark-based. Do them one per session._
