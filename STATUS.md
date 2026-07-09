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
- [x] **UI:** sidebar rows show a live **`next in Xm`** countdown (client-side 15s `now` ticker), a
      transient **`Checking…`** chip (driven by the tick events), an **`error`** chip (tooltip = reason),
      and a **`Last checked H:MM · scanned · new`** status line. The feed empty-state now reflects the
      last check — `Checked N stories, nothing matched yet` / `Last check failed…` / `Checking…` instead
      of a blank pane. Reuses existing design tokens only.
- [x] Built via subagent-driven development (brainstorm → spec → plan → per-unit implement+review →
      whole-branch review). Spec: `docs/superpowers/specs/2026-07-09-tick-observability-design.md`;
      plan: `docs/superpowers/plans/2026-07-09-tick-observability.md`.
- [x] Verified: `cargo test` 19/19, `tsc`/`vite build` clean; live in the native window (create →
      `Checking…` → status line + countdown populate; restart → last-checked stats persist).
- [x] Merged `feat/tick-observability` → `main` (`--no-ff`), pushed; branch kept on origin.

**Known limitation (owned by TODO #4):** the scheduler still sleeps on a **monotonic** timer, so after a
laptop sleep the wall-clock countdown and the real next tick can drift. This feature only exposes the
honest `nextCheckAt`; making the schedule itself wall-clock/catch-up correct is TODO #4.

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
