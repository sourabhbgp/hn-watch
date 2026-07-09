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

## How to run

```bash
npm install
npm run tauri dev     # opens the native HN Watch window
```

Requires Node 20+, Rust stable, and `claude` on the PATH (used from Phase 3 onward).

**Testing:** test against the **real native app window**, never a browser at localhost — see
[`docs/TESTING.md`](docs/TESTING.md) for the verified computer-use test loop (launch → screenshot →
drive). Verified working end-to-end in Session 1.

## Next — Tray + native notifications (Phase 3)

- [ ] Keep running in the system tray with the window closed
- [ ] Fire a native notification when new matches land (hook off the existing `feed-updated` path)

## Backlog (later phases)

- [ ] Dig-deeper swarm: Rust orchestrator spins up several parallel `claude -p` agents,
      live streaming → one compiled brief (currently mock in the UI, reusing the shared `agent` runtime)
- [ ] Polish + design write-up / trade-offs in README

---

_Workflow: each phase is built on its own `feat/*` branch and merged into `main`._
