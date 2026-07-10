# Persist dig-deeper research — reopen the prior investigation, or dig again

**Date:** 2026-07-11
**TODO:** #8 (`docs/TODO.md`)
**Status:** approved — ready for implementation plan

## Problem

A completed dig-deeper run lives only in `DigDeeperPanel`'s React state. Closing the drawer
(`App.tsx` sets `digItem = null`, unmounting the panel) discards **everything** — the compiled
brief, the angles used, and each angle's findings/status. Reopening "Dig deeper" on the same
story re-runs the whole swarm from scratch (planner + parallel workers + synthesis), so the
research is gone **and** we pay real Sonnet usage again to regenerate it — the exact "same
runtime, mind the cost" concern the brief calls out.

## Goal / acceptance

Run dig-deeper on a story, close the drawer, reopen it → the previous **brief and every angle
used (with findings + status)** appear **instantly**, with **no** new `claude` processes
spawned. A **"Dig deeper again"** action starts a fresh run on demand. The saved research
survives an app restart (persisted in SQLite).

## Non-goals / scope guardrails

- **Latest-wins only.** One saved run per story; a re-run overwrites it. No per-story history.
- **Live view unchanged.** The running swarm still shows streamed progress lines, not per-angle
  findings — findings are new UI in the *saved* view only. Do **not** retrofit findings into the
  live lanes for "consistency."
- **Delivery/dedup/cancellation/degraded paths untouched.** This feature only adds persistence +
  a load-on-open branch.
- The verbatim requirement mandates persistence for **monitors + feed**, not for dig-deeper
  research; this is a deliberate enhancement (caching so the runtime never double-pays for the
  same story), not a requirement gap.

## Design

### 1. Data model — single `research` table (JSON columns), latest-wins

Chosen over the TODO's original two-table (`research` + `research_angles`) split: angles are only
ever read/written as one whole set together with the brief, and there is no cross-story angle
query, so a normalized join buys nothing. One row per story keeps save/load atomic and simple.

```sql
CREATE TABLE IF NOT EXISTS research (
    feed_item_id TEXT PRIMARY KEY REFERENCES feed_items(id) ON DELETE CASCADE,
    summary      TEXT NOT NULL,     -- brief overview text
    sections     TEXT NOT NULL,     -- JSON: [{heading, body}, …]  (BriefSection[])
    angles       TEXT NOT NULL,     -- JSON: SavedAngle[] (see below)
    created_at   INTEGER NOT NULL   -- epoch secs, drives "researched Xh ago"
);
```

- Added additively in `db::migrate()` via `CREATE TABLE IF NOT EXISTS` (matches the existing
  `monitors`/`feed_items`/`seen` pattern). `ON DELETE CASCADE` on `feed_item_id` means a saved run
  is dropped automatically when its monitor (and thus its feed item) is deleted, under the
  existing `PRAGMA foreign_keys = ON`.
- **SavedAngle** (serialized in the `angles` JSON array):
  `{ id, icon, label, focus, status: "done" | "failed", findings: string | null, error: string | null }`
  — `findings` set for a `done` angle, `error` set (with the reason) for a `failed` angle.

### 2. Backend — save on completion (never on start)

- **Retain per-angle errors.** `run_swarm`'s `JoinSet` result type widens from
  `(PlannedAngle, Option<String>)` to `(PlannedAngle, Result<String, String>)` so the per-angle
  **error text** (currently only emitted to the UI in `swarm-angle-done`, then dropped) is kept
  for saving. `swarm-angle-done` still emits the same `{output, error}` payload; the
  degraded-vs-failed check becomes "all results are `Err`"; `synthesize` still receives the
  `Option<String>` findings it needs (map `Result::ok()` at the call site — `build_synthesis_prompt`
  is unchanged).
- **Upsert on success.** After `synthesize` returns `Ok(brief)` and around the existing
  `swarm-brief-ready` emit, call a new
  `db::save_research(&conn, feed_item_id, &brief, &angle_results, now)` that
  `INSERT … ON CONFLICT(feed_item_id) DO UPDATE SET …`. Per-angle saved `status` =
  `done` when `Ok(findings)`, `failed` (with the message) when `Err(msg)`.
- **Save nothing otherwise.** A **cancelled** run (panel closed → task aborted before synthesis)
  and an **all-angles-failed** run (`swarm-failed`, no brief) write nothing. Consequence: clicking
  "Dig deeper again" and then cancelling — or the re-run fully failing — leaves the **prior**
  saved research intact. This is the reason save happens on completion, not on start.

### 3. Backend — load command

- `db::get_research(&conn, feed_item_id) -> rusqlite::Result<Option<SavedResearch>>` reads the row
  and deserializes the `sections` + `angles` JSON. `SavedResearch` is a plain DTO
  (`summary`, `sections: Vec<BriefSection>`, `angles: Vec<SavedAngle>`, `created_at`) built
  directly from the row — no need to deserialize into `agent::Brief` (which is `Serialize`-only).
  `BriefSection` already derives `Deserialize` (agent.rs).
- `#[tauri::command] get_research(item_id) -> Result<Option<SavedResearchDto>, String>`, exposed in
  `api.ts` as `getResearch(itemId)` returning `SavedResearch | null`. camelCase DTO mirroring the
  existing command style.

### 4. Frontend — the reopen flow (the crux of the feature)

`DigDeeperPanel`'s mount effect is restructured to be **saved-first**:

```
mount (keyed by item.id) → getResearch(item.id)
  ├─ found  → phase = "saved": render saved brief + saved angle lanes instantly.
  │           NO startDigDeeper, NO swarm → zero claude processes on reopen.
  └─ none   → startDigDeeper(item.id)   (today's planner → confirm → running flow, unchanged)
```

- New phase value **`"saved"`** joins `planning | confirm | running`. The `getResearch` call
  precedes any `startDigDeeper`; the planner is reached **only** on the `none` branch. This is the
  single most important correctness boundary — if both fired, we'd spawn a planner even when saved
  research exists, violating the "no new claude processes on reopen" acceptance criterion.
- The swarm event subscriptions still mount (harmless in the saved branch — no events arrive), so a
  later "Dig deeper again" streams live exactly as today. Unmount still `cancelDigDeeper(item.id)`
  (a no-op when nothing is running).

### 5. Frontend — saved view UI

- **Angle lanes reuse `AngleLane`.** Each saved lane shows the angle's **findings** prose + its
  `done`/`failed` chip, and for a failed angle the **error reason**. `AngleLane` gains an optional
  `findings` field rendered as a prose body block; the live view keeps rendering streamed `lines`
  (the two are mutually exclusive per lane). Existing `error` rendering covers the failed reason.
- **Brief block** is the existing render (summary + sections), with a quiet **`researched Xh ago`**
  meta line derived from `created_at` (reuses the feed's relative-time style + existing tokens;
  a small shared client-side formatter).
- **`Dig deeper again`** button in the saved view resets panel state to the normal planner flow
  (`phase = "planning"`, then `startDigDeeper` → confirm → running). On completion the backend
  upserts, overwriting the saved run. Re-running is always an explicit, visible click; viewing
  costs nothing.
- All colors/spacing from existing design tokens (`docs/design.md` / `index.css`) — no new tokens.

### 6. Data flow summary

```
First run:   panel → planner → confirm → run_swarm → synthesize Ok
                                                   → db.save_research (upsert)  ← NEW
                                                   → emit swarm-brief-ready
Reopen:      panel → getResearch → Some → render saved (0 claude)              ← NEW
Dig again:   saved view → "Dig deeper again" → planner → … → run_swarm → upsert (overwrite)
```

## Testing

- **Rust unit tests (db.rs):**
  - `save_research` + `get_research` round-trip: brief summary + sections + a mix of `done`
    (with findings) and `failed` (with error text) angles; `created_at` preserved.
  - `get_research` returns `None` for an unknown feed-item id.
  - Upsert is latest-wins: saving twice for the same `feed_item_id` overwrites, one row remains.
  - Cascade: deleting the monitor drops the research row (via `feed_items` cascade).
- **Live native-window proof (Session 10 lesson — "rendered" ≠ "didn't re-run"):** reopen a
  researched story and **prove no `claude` process spawns** (instrument the planner entry or watch
  the process count), then verify the saved brief + angles (incl. a failed angle's reason) render;
  verify "Dig deeper again" starts a fresh run and overwrites; verify a cancelled re-run leaves the
  prior saved research intact; restart the app and confirm the saved research still loads.
- `cargo test` green, `cargo build` zero warnings, `tsc` + `vite build` clean.

## Open decisions (resolved)

- **Schema:** single `research` table with JSON columns (not two normalized tables). ✅
- **"researched Xh ago" timestamp in the saved view:** yes. ✅
- **Failed angle in the saved view:** show `failed` **+ the error reason** (requires widening the
  `JoinSet` result to `Result<String, String>`). ✅
- **History vs latest-wins:** latest-wins. ✅
