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

## 4. Sleep/suspend & catch-up scheduling (wall-clock, not monotonic)  ⛔ WON'T DO (Session 7 decision)

> **Decision (Session 7): the monotonic "stopwatch" scheduling is intentional — do not rewrite it.**
> The interval is meant to count **active runtime only**: it advances while the app is running and the
> laptop is awake, and **pauses** (never rushes) across laptop sleep. `tokio::time::sleep` already gives
> exactly this — it freezes during OS sleep and resumes the leftover on wake. On app **start** every
> monitor does a **fresh run**, then ticks every interval; stopwatch progress is deliberately **not**
> persisted across an app close (a close discards the in-flight timer, a relaunch does a fresh run —
> lossless anyway thanks to the watermark). Both the **wall-clock catch-up rewrite** below **and** an
> **"active-time" heartbeat** to survive app-close were considered and **rejected as over-engineering**.
> The only change shipped was cosmetic: the countdown shows a calm `checking soon…` instead of a stuck
> `due now` during the post-wake catch-up window (`Sidebar.tsx`, `fmtCountdown`). See `STATUS.md`
> (Session 7). The original write-up is kept below for history only — **do not implement it.**

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

## 5. Surface the notification-denied state in the UI — no silent muting  ⛔ DESCOPED (Session 9)

> **Descoped (Session 9): not deliverable on desktop with the current plugin.** Built the full feature
> (backend `notification_health` command + pure mapping, DRY shared `Banner` + `NotificationBanner`,
> `App.tsx` focus-recheck wiring) on `feat/notification-permission-banner`, all reviews clean incl.
> whole-branch (opus). **Live E2E on the release build proved it can't work:** with macOS notifications
> truly off, the banner never appeared. **Root cause (primary-source):** `tauri-plugin-notification`
> 2.3.3 (latest) hardcodes desktop `permission_state()` **and** `request_permission()` to
> `Ok(PermissionState::Granted)` (`desktop.rs`) — never queries the OS — so the denied state is
> undetectable. Official Tauri docs are silent on desktop permission behavior (why it passed review);
> the popular fork `Choochmeque/tauri-plugin-notifications` v0.4.6 stubs desktop the same. No
> off-the-shelf fix — detecting desktop notification-denied needs a native `UNUserNotificationCenter`
> query via `objc2` (standard native-app pattern, risky in an ad-hoc-signed build, and stub-able
> incidental plumbing per the brief). **Reverted** the feature (`13e59a1`; code recoverable in history)
> and **removed** the pre-existing dead startup `request_permission()` guard (`72a5825`). Delivery is
> unchanged; macOS still prompts once on first delivery — only *denial detection* is missing. See
> `STATUS.md` (Session 9) and the spec's Outcome section. If revisited, do the native `objc2` spike
> **first** (prove the query works from the bundle AND matches `notify_rust`'s delivery subsystem). The
> rest of this entry is kept for history.

**Problem.** Notification permission is requested once at startup; if the user clicks **Don't
Allow** (or later turns notifications off in System Settings), every `.show()` fails **silently** —
no banner, no in-app indication, no re-prompt. macOS never shows the permission prompt again after a
denial, so the only recovery is System Settings, which the user has no way to discover from the app.

**Why it matters.** On a **different computer / fresh install**, a user who denies the prompt gets a
watchtower that never taps them on the shoulder, with **zero signal that anything is wrong** — the
core "fire a native notification when new items land" requirement quietly doesn't work. (Raised by
the user after Session 8: "will it fail silently on another machine?" — yes, this is that gap.)

**Current behavior.** `lib.rs` `setup` calls `n.request_permission()` only when state isn't
`Granted` and **discards the result**; `scheduler.rs` fires `.show()` best-effort (`let _ = …`).
Nothing reads the permission state back after startup or surfaces it anywhere in the UI.

**Proposed approach (reuse the Claude-health banner pattern from Session 5).**
- Read `notification().permission_state()` at startup and expose it to the frontend (a
  `notification_health` command, or fold a `notificationsBlocked` flag into the existing health DTO)
  — distinguish `Granted` / `Denied` / `NotDetermined`.
- When not `Granted`, show the existing top **banner** (rust / `hn-soft` tokens, same component as
  the Claude banner): *"Notifications are off — enable them in System Settings › Notifications ›
  hn-watch to get alerts when new matches land."* Optional button opens the pane via the already-
  registered opener plugin: `open "x-apple.systempreferences:com.apple.Notifications-Settings.extension"`.
- **Re-check** on window focus / a manual button (mirror the Claude Re-check) so flipping it on in
  System Settings clears the banner without a restart.
- Keep `.show()` best-effort — this is purely about making the off-state **visible + recoverable**,
  not changing delivery.

**Verification gotcha (read before testing).** computer-use screenshots black out the
NotificationCenter banner layer **and** macOS suppresses banners while the app is frontmost — verify
delivery with `screencapture -x` and the app backgrounded. See `STATUS.md` (Session 8) and the
`hn-watch-notification-verify-gotcha` memory.

**Reuses.** The Claude-health banner + Re-check pattern (`feat/error-handling-preflight`, Session 5),
`tauri-plugin-opener` (already registered), existing design tokens.

**Acceptance.** On a machine where notification permission is denied/off, the app shows a clear,
dismissible "notifications are off + how to enable" banner instead of silently never notifying;
enabling it in System Settings and re-checking clears the banner; when permission is granted there is
no banner and notifications deliver as today.

---

## 6. Topic-level (near-duplicate) dedup — same story, different submissions

**Problem.** Today's dedup keys on the exact HN item id (`(monitor_id, hn_id)`), so it only
stops the **identical** story from repeating. Two **different** HN stories about the **same
topic** are not deduped and both land as separate feed cards — e.g. the same launch submitted
twice under different ids, a TechCrunch and a Verge writeup of one announcement, or a next-day
follow-up on the same event. Each has a distinct `hn_id`, so it passes every existing layer,
Claude judges each independently, and both match.

**Why it matters.** For a busy monitor a single news event can produce several near-identical
cards, cluttering the one shared feed and burying genuinely distinct matches. (Raised by the
user: "is there a possibility that two articles cover the same topic?" — yes, this is that gap.)

**Current behavior.** Four dedup layers (`dedupe_by_hn_id`, the `seen` table, the
`UNIQUE(monitor_id, hn_id)` constraint, crash-safe commit ordering) all key on `hn_id` — exact
story identity only. No semantic/topic comparison exists anywhere on the tick path.

**Proposed approach (lightweight first, semantic only if needed).**
- **Cheap pass — normalized-URL / domain grouping.** Dedup on the canonical article URL (strip
  tracking params, normalize host) so re-submissions of the *same link* collapse. Catches exact
  re-posts; misses different articles on the same event. Low cost, no extra `claude` call.
- **Semantic pass (optional, heavier).** Ask `claude` "is this a near-duplicate of these recent
  matches?" against the last N feed titles for the monitor, or embed titles and threshold on
  cosine similarity. Accurate but adds cost/latency per tick and risks wrongly merging two
  genuinely distinct takes — gate it behind a per-monitor opt-in, don't make it default.
- Keep the exact-`hn_id` layers exactly as-is underneath either approach.

**Acceptance.** Feed a monitor two distinct HN stories covering one event: the duplicate is
either collapsed into a single card (or grouped/marked as related) rather than shown twice, and
two genuinely different matches are **not** wrongly merged.

---

## 7. Full-history feed search (backend FTS5) — beyond the client-side cap

**Problem.** The client-side feed search (shipped on `feat/feed-search`, see
`docs/superpowers/specs/2026-07-10-feed-search-design.md`) filters only the **newest 1000 items**
the backend ships (`db::list_feed` `LIMIT 1000`). A query never reaches older matches still in the
`feed_items` table. Fine for a recency-first watchtower, but it means "search everything I've ever
matched" isn't possible.

**Why it matters.** As the feed grows past 1000, older matches become unfindable by search even
though they're persisted. Only relevant once a user has accumulated a large history.

**Current behavior.** Search is a pure frontend filter over the in-memory (capped) feed; no query
reaches SQLite.

**Proposed approach.** A backend search command — either a `LIKE '%term%'` scan over
`feed_items(title, summary, reason)` or an SQLite **FTS5** virtual table kept in sync on insert —
exposed as a `search_feed(query)` Tauri command. The frontend switches to server results when a
query is active (debounced), falling back to the client-side filter for the empty-query feed.
Keep the exact-`hn_id` dedup and the `LIMIT`-capped default feed untouched.

**Acceptance.** A query returns matches from the full persisted history, not just the newest 1000;
the default (no-query) feed and its cap are unchanged.

---

## 8. Persist dig-deeper research — reopen the full prior investigation, or dig again

**Problem.** A completed dig-deeper run lives only in the panel's React state. Closing the drawer
(`App.tsx` sets `digItem = null`, unmounting `DigDeeperPanel`) discards **everything** — the
compiled brief, the angles that were used, and each angle's findings/lanes. Reopening "Dig deeper"
on the same story re-runs the whole swarm from scratch — a fresh planner + parallel workers +
synthesis — so the research is gone **and** you pay real Sonnet usage again to regenerate it.

**Why it matters.** UX: you can't step away and come back to your research. Cost: the shared
runtime pays twice (or N times) for the same story — the exact "same runtime, mind the cost"
concern the brief calls out. Raised by the user after Session 14 live verification.

**Desired behavior (user, Session 14).** Once a story has been dug into, opening it again should
show the **previous research in full** — the combined brief *and every angle that was used* (with
each angle's findings and its done/failed status) — **and** offer a clear "**Dig deeper again**"
action to run a fresh investigation on demand. Viewing costs nothing; re-running is an explicit,
visible choice.

**Scope note.** The verbatim requirement mandates local persistence for **monitors + feed**
("survive an app restart"), but **not** for the dig-deeper research — the swarm ask is only
"compile into one combined brief," and the brief invites stubbing incidental plumbing. So this is
a deliberate enhancement, not a requirement gap, and a strong design point to discuss (caching so
the runtime never double-pays for the same story).

**Current behavior.** `swarm.rs` reads the story via `db::get_feed_item` and only **emits** the
brief (`swarm-brief-ready`) and per-angle progress/outcomes as transient events — nothing is
written back. The DB has only `monitors`, `feed_items`, `seen`; there is no research/brief table.

**Proposed approach.**
- **Persist the whole run, not just the brief.** New tables keyed by `feed_item_id`
  (one saved run per story, latest wins):
  - `research` — the compiled brief (summary + sections as JSON or raw Markdown) + `created_at`.
  - `research_angles` — each angle used: `label`, `focus`, `icon`, final `status`
    (done/failed), and its findings text. So the reopened view can show all the angles, not
    just the final brief.
- **Write on completion** — `run_swarm` saves the brief **and** the per-angle results right
  before/after emitting `swarm-brief-ready` (only a completed run is saved; a cancelled run isn't).
- **Load on open** — a `get_research(feed_item_id)` command; `DigDeeperPanel`'s mount effect
  checks for a saved run first. If present, render the **saved brief + all saved angle lanes**
  directly (skip planner/workers) instead of calling `startDigDeeper`.
- **"Dig deeper again"** button in the saved-research view — intentionally starts a fresh swarm
  (the normal plan → confirm → run flow), overwriting the saved run on completion. So a re-run is
  always a deliberate, visible cost, never accidental on reopen.
- Keep cancellation/degraded behavior unchanged.
- *Optional extension:* keep a short history of past runs per story instead of latest-wins, so you
  can compare how a story's research evolved over time.

**Acceptance.** Run dig-deeper on a story, close the drawer, reopen it → the previous **brief and
every angle used (with findings + status)** appear instantly, with **no** new `claude` processes
spawned; a "Dig deeper again" action starts a fresh run on demand; the saved research survives an
app restart (persisted in SQLite).

---

_Order to tackle: **#1 (observability) done (Session 4); #3 (error handling / preflight) done
(Session 5); #2 (lossless ingestion) done (Session 6).** **#4 (sleep/wake catch-up) — WON'T DO**
(Session 7: monotonic stopwatch scheduling is intentional; wall-clock rewrite + active-time
persistence both rejected as over-engineering). Phase 3 (tray + native notifications) shipped
(Session 8). **#5 (surface the notification-denied state) — DESCOPED (Session 9):** unbuildable on
desktop; `tauri-plugin-notification` never reads real permission state (stub always returns
`Granted`). The **dig-deeper research swarm — the last core requirement — shipped (Session 14)**
on `feat/dig-deeper-swarm`, live-verified end-to-end (see `STATUS.md`); **Session 15** then made
the **planning phase cancellable** (`feat/cancellable-planning`) so closing the panel stops the
planner too, not just the workers. All core requirements are now complete; remaining items
(**#6** topic dedup, **#7** full-history search, **#8** persist dig-deeper research) are optional
enhancements. **Next up: #8 (persist dig-deeper research — reopen the full prior investigation,
or dig again).**_
