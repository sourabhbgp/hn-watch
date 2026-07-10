# STATUS

A running log of what actually works, what's next, and how to run the app.
Updated at the end of every working session.

**Requirement (source of truth):** [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md).

---

## Session 1 — Scaffold + static UI shell

**Done**

- [x] Scaffolded Tauri 2 + React 19 + TypeScript + Vite
- [x] Wired Tailwind CSS v4 (Vite plugin) with the HN Watch design tokens
- [x] Window configured — title "HN Watch", 1160×780, min 900×600
- [x] Project docs seeded: `README.md`, `STATUS.md`, `DECISIONS.md`, `docs/architecture.html`
- [x] Static, non-functional UI shell rendering **mock data**:
  - Left sidebar: list of monitors + "New monitor" button (no-op)
  - Center: Twitter-style feed of match cards
  - Slide-over: "Dig deeper" panel with mock agent lanes + a compiled brief
- [x] Verified live: app compiles & launches; feed renders; monitor filtering and
      the dig-deeper panel work (checked against the Vite dev server)
- [x] Git initialised; baseline on `main`; UI built on `feat/ui-shell`, merged via `--no-ff`

**Not yet (intentionally)** — everything below is UI-only mock; no logic behind it:
buttons are no-ops, data is hard-coded, nothing calls Rust or Claude, no persistence.

## Session 2 — Brand icon + project docs

**Done**

- [x] Custom brand mark — bold white "W" on an HN-orange squircle, one source `assets/brand/icon.svg`
- [x] Regenerated the app bundle icon set (dock / Spotlight); replaced the Tauri placeholder
- [x] Sidebar logo Y → W, sized to match the dock icon; updated the `docs/architecture.html` logo
- [x] `docs/design.md` — design-system reference (brand, color tokens, typography, components)
- [x] Captured the verbatim brief in `docs/REQUIREMENTS.md`; added root `CLAUDE.md` (auto-loaded
      session context); removed `DECISIONS.md`
- [x] Verified live in the native window; merged `feat/brand-icon` → `main`, pushed to origin

## Session 3 — Monitors CRUD + persistence + the real tick loop (Phase 2)

**Done** — the app now has a working core loop behind the UI (no more mock):

- [x] SQLite store in the Rust core (`db.rs`, `rusqlite` bundled): `monitors`, `feed_items`,
      and a `seen` table for dedup. FK cascade so deleting a monitor drops its feed + seen rows.
- [x] Recent HN pulled from the Algolia HN Search API (`hn.rs`).
- [x] `claude -p` agent runtime (`agent.rs`): one call per tick, strict JSON verdict parsing,
      bounded by a shared semaphore so monitor ticks and the future swarm share one runtime.
      Resolves the `claude` binary via PATH + common install dirs (works when launched from Finder).
- [x] Per-tick logic (`tick.rs`) + per-monitor background workers (`scheduler.rs`): each monitor
      ticks immediately on create/startup, then every interval. Dedup against `seen`; matches
      appended to the one feed; `feed-updated` event → UI refresh. A failed tick is logged and skipped.
- [x] Tauri commands (`commands.rs`): `create_monitor` / `list_monitors` / `delete_monitor` /
      `list_feed`; DTOs serialize to the exact `src/types.ts` shapes. Workers respawn on startup.
- [x] Frontend wired to live data (`api.ts`, `App.tsx`, `Sidebar.tsx`): inline create form
      (name + prompt + interval), per-row delete, event-driven feed refresh. Reuses design tokens.
- [x] **Verified live in the native window**: create → immediate tick → real HN matches with
      `claude` summaries/reasons; restart → monitors + feed persist; dedup holds (no duplicate
      cards); delete cascades the feed away. Built via `tauri build` and driven with computer-use.
- [x] Fixes found during verification: resolve the `claude` binary path (Finder-launched app has a
      minimal PATH); timeout the HN fetch + `claude` call and make feed inserts idempotent
      (`UNIQUE` + `INSERT OR IGNORE`); sandbox the judge call with `--safe-mode` + `PWD` override so
      ticks never trigger a macOS file-access prompt or read user files.
- [x] Merged `feat/monitors-and-tick` → `main` (`--no-ff`), pushed; branch kept on origin.

**Deferred backlog captured in [`docs/TODO.md`](docs/TODO.md)** (pick one per future session):
tick observability + live "next in Xm" countdown/status chips; lossless burst ingestion
(watermark + pagination + chunked `claude` calls); error handling + mandatory Claude Code startup
preflight; sleep/wake catch-up scheduling (wall-clock, not monotonic).

**Not yet (next phases)** — system tray + native notifications; the dig-deeper research swarm
(still mock in the UI). Deliberately not built: monitor edit/pause/status, "Run now", search/filters.

## Session 4 — Tick observability (TODO #1)

**Done** — a monitor is no longer a black box; from the UI alone you can tell what each one is doing:

- [x] **Persisted per-tick results** on the `monitors` table (`last_checked_at`, `last_checked_count`,
      `last_new_count`, `last_error`) via an **idempotent additive migration** (`ensure_column` checks
      `PRAGMA table_info` before `ALTER TABLE` — safe to run every launch, upgrades existing on-disk DBs
      without data loss). New `db::record_tick` writes all four in one `UPDATE`; `None` error clears a
      prior error. Covered by tests (migration idempotency + pre-existing-DB upgrade + error round-trip).
- [x] **Tick lifecycle events** from `scheduler.rs`: `tick-started {monitorId}` and
      `tick-finished {monitorId, checkedCount, newCount, error?}` around each tick; `run_tick` returns a
      `TickOutcome { checked, new }` (`checked` = stories *scanned* that tick, ~30, not just unseen).
      `feed-updated` still fires only when new matches land.
- [x] **DTO exposes** `lastCheckedAt` / `nextCheckAt` (= `last_checked_at + interval`) / counts /
      `lastError` as raw epoch seconds + numbers; the client formats time and the countdown. `status`
      derives `"error"` when the last tick failed, else `"active"`.
- [x] **UI — monitor tiles (redesigned for calm hierarchy):** each monitor is a contained tile
      (`border-line` card, `hn-soft` when selected) on a widened 288px sidebar. It shows: the name, a
      quiet **`next in Xm`** countdown pill (client-side 15s `now` ticker) that goes live as a
      **`Checking…`** pill (pulse) during a tick and **`error`** when the last tick failed (tooltip =
      reason), the prompt, and one calm meta line — **`N matches · N new · checked H:MM`**. `· N new`
      appears only when a tick actually brought new matches, in brand orange (`text-hn`) so fresh
      arrivals catch the eye; the per-tick *scanned* count moved to the meta line's hover tooltip.
      Times render in the **viewer's local timezone** (`toLocaleTimeString`, no fixed zone). The feed
      empty-state now reflects the last check — `Checked N stories, nothing matched yet` /
      `Last check failed…` / `Checking…` instead of a blank pane. All colors are existing design tokens.
- [x] Built via subagent-driven development (brainstorm → spec → plan → per-unit implement+review →
      whole-branch review), then a design pass with the frontend-design skill on the row layout. Spec:
      `docs/superpowers/specs/2026-07-09-tick-observability-design.md` (records the original single
      status-line design; the sidebar was later redesigned into the tiles described above); plan:
      `docs/superpowers/plans/2026-07-09-tick-observability.md`.
- [x] Verified: `cargo test` 19/19, `tsc`/`vite build` clean; verified live in the native window across
      states (fresh migration kept the existing monitor + matches; countdown ticks down; a real tick
      showed `N new` in orange; check-with-0-matches → "Checked N stories, nothing matched yet"; restart
      persists stats). A truncated status line found during review was the reason for the tile redesign.
- [x] Merged `feat/tick-observability` → `main` (`--no-ff`, merge `acb8f04`), pushed; branch kept on
      origin with full step-by-step history. (Note: an earlier premature merge was cleanly reverted —
      `main` was reset to its pre-feature state and re-merged only after review sign-off.)

**Known limitation (owned by TODO #4):** the scheduler still sleeps on a **monotonic** timer, so after a
laptop sleep the wall-clock countdown and the real next tick can drift. This feature only exposes the
honest `nextCheckAt`; making the schedule itself wall-clock/catch-up correct is TODO #4.

## Session 5 — Error handling + Claude preflight (TODO #3)

**Done** — Claude failures are no longer silent; a fresh clone learns immediately if `claude` is
missing or logged out, and every tick failure carries a human-readable reason:

- [x] **Typed errors** replace ad-hoc strings on the tick path: `AgentError`
      (`NotFound`/`NotAuthenticated`/`Timeout`/`Failed`) and `TickError` (`Hn`/`Agent`/`Db`), each with a
      stable `code()` (drives paused-vs-error + global health) and a friendly `message()` (stored in
      `last_error`, shown in the monitor tooltip). Classification lives in **pure, unit-tested functions**
      (`is_auth_failure`, `classify_auth`, `next_claude_health`) mirroring the existing `parse_verdict`/`find_claude`
      seam. **Non-goal held:** a 0-match or unparseable judge response stays `Ok([])` — never an error.
- [x] **Startup preflight** (`agent::preflight`, async in `setup` so the window never blocks): binary
      absent → `Missing` without spawning; otherwise a **no-token** `claude auth status --json` probe →
      `Ok`/`NotAuthenticated` (empirically grounded on claude 2.1.205 — `auth status` makes no model call).
      DRY: one `claude_command()` helper carries the temp-dir/`PWD`/stdin-null sandbox, shared by the judge
      call and the probe. Kept `--safe-mode` (never `--bare`, which strips OAuth/keychain auth).
- [x] **Shared `Arc<Mutex<ClaudeHealth>>`** seeded by preflight and kept live by ticks: only
      `claude_missing`/`claude_auth` flip global health; a **real** successful tick (agent actually ran)
      clears it; transient errors and **no-op early-return ticks** leave it unchanged. The DTO maps global
      health → `status:"paused"` (overrides per-monitor `error`). New commands `claude_health` +
      `recheck_claude`; recovery to `Ok` clears stale per-monitor errors so monitors return to `active`.
- [x] **UI:** a persistent top **banner** (rust/`hn-soft` tokens) with a **Re-check** button —
      "Claude Code not found …" / "Claude Code isn't logged in …"; the previously-inert **`Paused`** chip on
      each monitor now lights up when Claude is globally unavailable. No new colors — existing tokens only.
- [x] Built via subagent-driven development: brainstorm → spec
      (`docs/superpowers/specs/2026-07-09-error-handling-preflight-design.md`) → plan
      (`docs/superpowers/plans/2026-07-09-error-handling-preflight.md`) → 2 implementer units (Rust, then
      frontend) each with a task review → whole-branch review (opus).
- [x] **Two Important bugs the whole-branch review + live verification caught, then fixed** (commits
      `00ce131`, `b02727e`): **(A)** a tick where every fetched story was already seen early-returns `Ok`
      without calling the agent — the scheduler used to treat that as "Claude healthy" and wrongly cleared a
      legitimate Missing/logged-out banner; now health only clears when the agent actually ran
      (`TickOutcome.agent_ran` + pure `next_claude_health`). **(B)** preflight/Re-check recovery set health
      `Ok` but left stale `last_error`, so recovered monitors showed a false `error` chip; now recovery
      clears all monitor errors via a shared `apply_claude_health` helper. Also: the scheduler now logs the
      raw tick error (`{e:?}`) so HN/db failures leave a diagnostic trail.
- [x] **Verified:** `cargo test` 27/27, `tsc`/`vite build` clean, zero warnings. **Live-verified in the
      native window** on the fixed release build: missing-binary → "not found" banner + both monitors Paused;
      fake logged-out → "isn't logged in" banner + both Paused (confirming bug A — an all-seen monitor's
      early-return no longer clears the down-state); flip the fake `claude` to healthy + click **Re-check** →
      banner clears, both monitors return to `active` "next in 30m · checked HH:MM" with no stale error.
      (Down states forced with an `HN_WATCH_CLAUDE_BIN` env override pointing at a fake script — no logout
      needed.)
- [x] Merged `feat/error-handling-preflight` → `main` (`--no-ff`), pushed; branch kept on origin with the
      full step-by-step history (spec → plan → 6 Rust commits → 3 frontend commits → 2 fix commits).

**Deferred (Minor, recorded for a later cleanup):** `ClaudeHealthPayload`/`ClaudeHealthDto` are the same
`{status,message}` shape defined twice (plan-authorized for task independence); the scheduler matches on
string `code` literals rather than the enum; the App Re-check does a harmless double state-update. None
affect correctness. **Known:** `recheck_claude` clears every monitor's `last_error` on recovery, including a
stale non-Claude (`hn_error`) one — it re-populates on the next tick if still failing.

## Session 6 — Lossless ingestion under variable volume (TODO #2)

**Done** — a monitor can no longer silently miss stories in a burst; ingestion is complete at any volume:

- [x] **Per-monitor watermark** replaces the fixed "newest 30" fetch. New nullable `monitors.watermark`
      (additive `ensure_column` migration — upgrades existing on-disk DBs to `NULL`). Each tick fetches
      **everything since `watermark.unwrap_or(now − 1h)`** — one unified path, so a fresh *or* migrated
      monitor looks back an hour on its first tick, then carries the watermark forward. `HnItem` now
      carries `created_at` (Algolia `created_at_i`).
- [x] **Paginated delta fetch** (`hn::fetch_since`): `search_by_date?numericFilters=created_at_i>=W`,
      pages of 100 until a short page or a 10-page (1000-hit) safety cap (logged if hit). A burst of 5 or
      500 is pulled in full — nothing beyond 30 is dropped.
- [x] **Chunked, fail-closed judge:** the unseen set is split into `claude` calls of ≤30, run
      **sequentially within a tick** (so one burst can't grab all 4 shared-semaphore permits and stall
      other monitors). Any batch failure returns `Err` **before any DB write** — nothing committed, the
      watermark not advanced, the whole window re-judged next tick.
- [x] **Watermark advance = `max(created_at) − 5min`, monotonic**, ignoring absurd timestamps. The
      trailing 5-min margin is the real correctness fix: Algolia indexes asynchronously, so a story with
      an older timestamp can be indexed *after* newer ones — an exact-max watermark would skip it forever;
      the margin re-scans that tail each tick (free, `seen`-deduped). Commit order **insert → mark seen →
      advance watermark (last)** makes a mid-commit crash safe with no transaction. Dedup (`seen` +
      `UNIQUE`) is untouched — it's what makes the re-scans free.
- [x] Built via subagent-driven development: brainstorm → spec
      (`docs/superpowers/specs/2026-07-09-lossless-ingestion-design.md`) → plan
      (`docs/superpowers/plans/2026-07-09-lossless-ingestion.md`) → 1 implementer unit (4 Rust commits) →
      task review (Approved) → **whole-branch review (opus)**.
- [x] **Critical bug the whole-branch review caught + fixed** (commit `7d47997`): the scheduler reused a
      **frozen `Monitor`** — `run_tick` persisted the advanced watermark to the DB but nothing read it back
      in-session, so `since` never moved for a running worker (at the UI's 1h interval that's a permanent
      per-tick miss window — the exact guarantee this ticket delivers). Fix: `run_tick` returns the new
      watermark, the worker binds `let mut monitor` and adopts it each tick, and the all-seen early-return
      persists it too. (Task-scoped tests were all green — only the broad review saw it.)
- [x] **Verified:** `cargo test` 34/34, `cargo build` zero warnings. **Live-verified in the native app**
      (instrumented dev run, instrumentation reverted): (1) **carry-forward** — `since` advances between
      ticks (`None`→`W1`→`W2`) and the window narrows 30→7; migration upgraded the real pre-existing DB
      non-destructively; (2) **burst** — 167 stories pulled across 2 Algolia pages, 85 unseen judged in
      batches of 30/30/25, no duplicate feed cards; (3) **fail-closed** — a forced judge failure over a
      162-story window committed nothing and left the watermark unadvanced.
- [x] Merged `feat/lossless-ingestion` → `main` (`--no-ff`), pushed; branch kept on origin with the full
      step-by-step history (spec → plan → 4 Rust commits → 1 fix commit).

**Not yet (next phases)** — system tray + native notifications (Phase 3); the dig-deeper research swarm
(still mock in the UI). Remaining backlog refinement: TODO #4 (sleep/wake wall-clock catch-up) — its
catch-up tick will already be lossless now that ingestion is watermark-based.

## Session 7 — Countdown label polish (stopwatch scheduling affirmed)

**Decided (no build):** the monotonic "stopwatch" scheduling is **intentional**, not a bug. Confirmed
the current behavior already matches the desired model with **no backend change**: on app start every
monitor does a **fresh run**, then ticks every interval on a monotonic timer that **freezes during
laptop sleep** (resumes the leftover, never rushes); stopwatch progress is **not** persisted across an
app close (a close discards the in-flight timer, a relaunch does a fresh run). TODO #4's wall-clock
catch-up rewrite and an "active-time" heartbeat were both considered and **rejected as over-engineering**.

**Done** — the one cosmetic seam left in that flow:

- [x] Countdown pill now shows a calm **`checking soon…`** instead of a stuck **`due now`** when a
      monitor is past its wall-clock due time (the window where the monotonic timer, paused across a
      laptop sleep, is still catching up). Pure one-line frontend change in `Sidebar.tsx`
      (`fmtCountdown`); no backend, no new state, no new design tokens. `tsc` + `vite build` clean.

## Session 8 — System tray + native notifications (Phase 3)

**Done** — the watchtower now watches with the window closed and taps you on the shoulder when new
matches land; it no longer only watches while you stare at it:

- [x] **Close-to-tray** (`lib.rs` `WindowEvent::CloseRequested` → `api.prevent_close()` + `window.hide()`):
      the red button / Cmd-W hides the window instead of quitting; monitor workers keep ticking, the Dock
      icon stays (no activation-policy change). The only exit path is the tray Quit item.
- [x] **Tray (menu-bar) icon** — new `src-tauri/src/tray.rs` (`tray-icon` feature on `tauri`): builds a
      status item using the app icon with a two-item menu, **Show HN Watch** / **Quit HN Watch**. Show (and
      a left-click on the icon) `show()`+`unminimize()`+`set_focus()` the window; Quit is `app.exit(0)`.
- [x] **Native notifications** (`tauri-plugin-notification`, fired from Rust in `scheduler.rs` at the
      existing `new > 0` site): **one notification per monitor** that landed matches, title
      `"{name} · {N} new match(es)"` (U+00B7 `·`), body = the top matched story's title (`+N more` when the
      tick brought several), falling back to the monitor prompt. Pure `format_notification` is unit-tested
      (singular/plural/fallback). Additive `TickOutcome.newest_title` carries the title; the storm-coalesce
      guarantee (1 notif/monitor/tick) is preserved. Startup requests notification permission once in `setup`.
- [x] Built via subagent-driven development: spec
      (`docs/superpowers/specs/2026-07-09-tray-and-notifications-design.md`) → plan
      (`docs/superpowers/plans/2026-07-09-tray-and-notifications.md`) → 2 implementer units (tray, then
      notifications) each task-reviewed clean → whole-branch review (opus; no code defects, only two
      verification gaps to close live).
- [x] **Verified:** `cargo test` 37/37, `cargo build` zero warnings. **Live-verified in the native release
      app** (computer-use + AppleScript/System Events): close→hide keeps the process alive; tray menu copy
      exact; **Show** restores the hidden window; **Quit** exits the process cleanly; first-launch window
      opens without the permission prompt hanging it (whole-branch review's I2).
- [x] **Notification delivery proven (whole-branch review's I1).** Permission is granted (System Settings →
      Notifications → hn-watch: Allow ON, Desktop/Notification Centre/Lock Screen checked, Alert Style
      Temporary). A real-screen `screencapture -x` showed a live hn-watch banner (app badge, Apple-
      Intelligence-summarised body). **Gotcha for future sessions:** computer-use screenshots apply native
      app-filtering that **blacks out the NotificationCenter banner layer** — banners are invisible in those
      captures even though they fire; use `screencapture -x` (real pixels) to see them. The off-main-thread
      `.show()` (I1's concern) is **not** a bug — `UNUserNotificationCenter.add` is thread-safe; no code fix.
- [x] Cleaned up 4 throwaway test monitors created during verification (DB backed up); the two real
      monitors are untouched. Merged `feat/tray-notifications` → `main` (`--no-ff`), pushed; branch kept.
- [x] **Follow-up fix `fix/dock-reopen`** (merged `a996411`): close-to-tray only hid the window and
      nothing handled the macOS reopen event, so clicking the app's **Dock icon did nothing** — only the
      tray "Show HN Watch" restored it. Now `RunEvent::Reopen` in the run loop calls the (now-public)
      `tray::show_main_window`, so a Dock-icon click re-shows the window (Spotify-style). Verified live:
      launch → close (hide, process alive) → Dock reopen → window restored, visible + frontmost.

**Not yet (last phase)** — the dig-deeper research swarm (still mock in the UI).

## Session 9 — Notification-denied banner (TODO #5) — ⛔ DESCOPED (plugin limitation)

**Attempted, then descoped after live end-to-end testing proved it unbuildable on desktop.** The goal
was TODO #5: when macOS notifications are off, show a banner (reusing the Session-5 Claude-health
banner + Re-check pattern) with an "Open Settings" deep-link that self-clears on window focus.

- [x] Full subagent-driven build, all reviews clean: spec + plan
      (`docs/superpowers/specs/2026-07-09-notification-permission-banner-design.md`, `…/plans/…`); a
      synchronous `notification_health` command + pure unit-tested `granted`/`denied`/`default`
      mapping (Task 1); a DRY shared `Banner` + `NotificationBanner` + `App.tsx` focus-recheck wiring
      (Task 2). A whole-branch review (opus) found **no code defects** — merge-ready *pending live E2E*.
- [x] **Live E2E on the release build caught a false premise that static review could not.** With
      macOS notifications toggled truly **off** (System Settings → Notifications → hn-watch → Allow
      off), the banner **never appeared** — neither on focus-recheck nor on a cold relaunch.
- [x] **Root cause (primary-source):** `tauri-plugin-notification` **2.3.3 (latest)** hardcodes the
      desktop permission API — `desktop.rs` returns `Ok(PermissionState::Granted)` from **both**
      `permission_state()` and `request_permission()`, never querying the OS. So `notification_health`
      always read `granted`; the `denied` banner could never fire. Our mapping/banner/wiring were
      correct — fed a constant. Cross-checked: official Tauri docs are **silent** on desktop
      permission behavior (the reason it slipped design + code review); the popular fork
      `Choochmeque/tauri-plugin-notifications` (v0.4.6) **stubs desktop identically**. No off-the-shelf
      fix — detecting desktop notification-denied needs a native `UNUserNotificationCenter` query via
      `objc2`, which the weekend brief allows leaving as stubbed incidental plumbing.
- [x] **Decision:** descope + document; don't ship a banner that can never fire. **Reverted** the
      feature (`13e59a1`; code recoverable in history — `7e9061a`/`6227ffc`/`8d8f2d5`). **Cleaned up**
      the pre-existing **dead** startup `request_permission()` block in `lib.rs` (`72a5825`) — it was
      inert on desktop (guard never true against the always-`Granted` stub; the call itself a no-op).
      Notification **delivery is unchanged** and still works; macOS still prompts once on the first
      delivered notification (via `notify_rust`) — only *denial detection* is unavailable.
- [x] `cargo test` 37/37, `cargo build` 0 warnings, `npm run build` clean after the revert + cleanup.

**Takeaway:** the live-in-the-real-app test rule earned its keep — three passing review gates validated
correct code against a wrong assumption; only running it on the release build surfaced the truth.

## Session 10 — Feed honesty + performance (cosmetic-chrome cleanup + virtualization)

Two small units, both prompted by inspecting the live feed at ~500 items.

**Unit 1 — remove non-functional feed chrome (`feat/remove-cosmetic-feed-chrome`, merged `d5b6a28`).**

- [x] Dropped the header **`● live`** indicator — it was hardcoded/always-pulsing, bound to no state
      (the honest live signals already live in the sidebar chips: `Checking…`/`error`/`Paused`).
- [x] Dropped the per-card **`▲ score / 💬 comments`** labels — real HN data but frozen at ingest
      (fresh stories ≈ `▲1 · 💬0`, never refreshed) and non-interactive with no link to the thread. The
      footer now holds only **Dig deeper**. Backend `hn_score/hn_comments` plumbing left intact (harmless,
      no schema change). Verified live in the release build; merged `--no-ff`, pushed, branch kept.

**Unit 2 — feed performance (`feat/feed-virtualization`).** The feed rendered **every** match into the DOM
at once (unbounded, ~500 and climbing) and re-fetched/re-rendered the whole list on every tick. Fixed with:

- [x] **Backend query cap** — `db::list_feed` now `LIMIT 1000` (recency-first). Bounds the IPC payload and
      the in-memory JS array regardless of table size. Per-monitor totals stay exact via `count_matches`
      (a `COUNT`, not this list), so the sidebar can read `1200 matches` while the feed ships the newest
      1000 — the deliberate, documented cap tradeoff (option (i): generous global cap, keeps instant
      client-side monitor filtering; server-side per-monitor pagination was the rejected heavier option).
- [x] **Virtualized list** — `@tanstack/react-virtual` in `Feed.tsx` with dynamic per-row measurement
      (variable card heights), `gap: 12`, `overscan: 6`, `getItemKey` = item id. Only cards in/near the
      viewport mount, so the DOM stays constant at any feed size. Empty-state path unchanged.
- [x] **`React.memo(FeedCard)`** so unchanged cards skip re-render. The `LIMIT` makes the existing
      full-reload-on-tick cheap, so no incremental-merge complexity was added.
- [x] **Advisor caught the verification gap** that mattered: "renders/scrolls fine" passes whether or not
      virtualization works at 500 items. **Proven** instead by a temporary `DEBUG mounted` readout against
      an injected **1200-row** dataset: header showed `1000 matches` (cap working) while the sidebar showed
      `1200 matches`, and the mounted `<article>` count held at **9 (top) → 16 (deep scroll)** — the DOM is
      windowed, not full. Debug line removed before finalizing. `tsc`/`vite build` clean, `cargo test` 37/37.

## Session 11 — Feed search + matched-term highlighting (`feat/feed-search`)

**Done** — you can now find a topic in the feed instead of scrolling it by eye. Frontend-only; no
Rust, no schema, no new dependency.

- [x] **Client-side search** — a `Search this feed…` box in the feed header filters the visible
      cards **live** as you type. Matches **title + AI summary + reason**, case-insensitive,
      multi-word **AND** (every whitespace-separated term must appear). Pure `matchesQuery`
      (`src/lib/search.ts`, with a shared `parseTerms`); the feed array is already in memory, so
      search is one more `useMemo` filter composed **on top of** the existing monitor filter — with
      a monitor selected it searches only that monitor's matches; on "All matches" it searches
      everything loaded. The query **clears when the monitor selection changes** so each view starts
      fresh.
- [x] **`X of Y` count + clear + empty state** — the header count reads `12 of 340 matches` while a
      query is active (falling back to the plain `N matches` otherwise); a `×` clears the box and
      restores the full feed; a query with no hits shows `No matches for "…"`.
- [x] **Matched-term highlighting** (added mid-session at user request; was originally a non-goal) —
      every matched term in a card's title, summary, and reason is wrapped in a `<mark>` with a
      subtle on-brand orange (`bg-hn-soft` token), so you can see *why* a card matched. Pure
      `highlight` helper (`src/lib/highlight.tsx`): case-insensitive, multi-term via one alternation
      regex, regex-metachars escaped (so a term like `c++` can't throw), empty query is a no-op.
      `FeedCard` gained a `query` prop; `React.memo` still skips unchanged cards while scrolling.
- [x] **Known limit (documented):** search covers the **newest 1000** items the backend ships
      (`db::list_feed LIMIT 1000`), not the full history — the same recency cap the feed already
      uses. Full-history backend (FTS5) search is filed as **TODO #7**, not built here.
- [x] Built via subagent-driven development: spec
      (`docs/superpowers/specs/2026-07-10-feed-search-design.md`) → plan
      (`docs/superpowers/plans/2026-07-10-feed-search.md`) → 4 implementer units (matcher → wiring →
      *(live verify)* → highlighting) each task-reviewed clean, then a whole-branch review.
- [x] **Verified live in the native window** (release build driven with computer-use): search box
      renders; typing `agents` → `250 of 1000` and the term highlighted orange in titles;
      case-insensitive (lowercase query matched capitalized text); with **Load Test** selected,
      `monitored` → scoped `499 of 998` with the term highlighted in the reason boxes; `×` restores
      the full feed; switching monitors clears the query; a nonsense query → `No matches for "…"`.
      `tsc`/`vite build` clean, `cargo test` untouched (no Rust changed). **Gotcha logged:** the
      computer-use launcher always opens the *release* `.app`, so a stale Session-10 debug bundle
      masked the branch until a fresh `tauri build` — always rebuild before driving.

## Session 12 — Pin the tick model to Sonnet 5 (`feat/pin-sonnet-5-model`)

**Done** — the per-tick `claude -p` filter agent now runs on a **known** model instead of
whatever the CLI default happens to be on the host.

- [x] **Pinned model** — `judge()` (`src-tauri/src/agent.rs`) now passes `--model claude-sonnet-5`
      to `claude -p --safe-mode`. Previously no `--model` flag was set, so each tick used the host's
      default Claude Code model (unspecified, could drift per machine/account).
- [x] **Verified** — smoke-tested the CLI accepts `--model claude-sonnet-5` (`claude -p … → OK`,
      exit 0); ran the exact judge-prompt shape the code builds and confirmed Sonnet 5 returns a
      clean JSON array with the right keys, correctly filtering matches (kept the on-topic story,
      dropped the off-topic one) — i.e. `parse_verdict` will parse it. `cargo build` clean.
- [x] **Note:** this hardcodes the version. To always track the newest Sonnet instead, use the
      `sonnet` alias; kept the explicit `claude-sonnet-5` per request.

## Session 13 — Cap per-tick fetch at 500 stories (`feat/cap-fetch-500`)

**Done** — bounds the fetch window after a long gap (laptop closed for a day/week), so a
stale watermark can't request an enormous window.

- [x] **Lowered the fetch safety cap** — `MAX_PAGES` in `src-tauri/src/hn.rs` from 10 → 5
      (5 pages × 100 = **500 stories max per tick**, down from 1000). After a long gap the tick
      judges at most the newest 500 stories; the watermark then self-heals to ~now on the next
      tick, and older stories in the gap are intentionally skipped.
- [x] **Behavior unchanged for the normal case** — a fresh monitor still looks back 1 hour on
      its first tick; steady-state ticks fetch a small window well under the cap.
- [x] **Note:** this is a story-*count* cap, not a time clamp — `since` is unchanged, so
      truncation is bounded by volume, not age. An explicit "never look back more than X" time
      clamp remains a separate future option if wanted.
- [x] **Verified** — `hn::` unit tests pass, `cargo build` clean. Merged `feat/cap-fetch-500` →
      `main` (`--no-ff`), branch pushed to origin and kept.

## How to run

```bash
npm install
npm run tauri dev     # opens the native HN Watch window
```

Requires Node 20+, Rust stable, and `claude` on the PATH (used from Phase 3 onward).

**Testing:** test against the **real native app window**, never a browser at localhost — see
[`docs/TESTING.md`](docs/TESTING.md) for the verified computer-use test loop (launch → screenshot →
drive). Verified working end-to-end in Session 1.

## Next — Dig-deeper research swarm (last phase)

- [ ] Rust orchestrator spins up several parallel `claude -p` agents, live streaming → one compiled
      brief (currently mock in the UI, reusing the shared `agent` runtime)

## Backlog (later phases)

- [ ] Polish + design write-up / trade-offs in README

---

_Workflow: each phase is built on its own `feat/*` branch and merged into `main`._
