# Lossless ingestion under variable volume — design

**Date:** 2026-07-09
**Ticket:** [`docs/TODO.md`](../../TODO.md) #2
**Branch:** `feat/lossless-ingestion`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

Each tick fetches only the **30 newest** HN stories (`hn::fetch_recent(30)` → Algolia
`search_by_date?hitsPerPage=30`). If more than 30 new stories land between two ticks — a busy
weekday morning, a link-storm — every story beyond the newest 30 is **never fetched, never
judged, permanently missed**. HN's steady-state rate is under 30/30-min so it rarely bites, but
a fixed count gives **no completeness guarantee**: the watchtower silently drops stories under
load.

The requirement's two guarantees:

- **(a) no story missed** — the real gap this ticket closes.
- **(b) no story analyzed twice** — **already solved** and kept as-is: the per-monitor `seen`
  table (SQLite) + `UNIQUE(monitor_id, hn_id)` on `feed_items` mean a story reaches `claude`
  at most once per monitor, ever, across restarts. We do **not** touch dedup; we *rely* on it —
  it's what makes the re-scans below free.

## Goal (acceptance)

Over a high-volume window, **every** story submitted since the last tick appears (paginated
in), each is analyzed **exactly once** (happy path), and no duplicate feed cards — independent
of how many arrived (5 or 500). A failed tick still never kills its worker.

## Core idea: a per-monitor watermark instead of a fixed count

Replace "newest 30" with **"everything since where we left off."** Store a **watermark**
(the newest submission time we've processed) per monitor; each tick pulls the exact delta since
that watermark, paginated, so volume can't overflow a fixed window.

### One unified code path (no special first tick)

The watermark is **nullable**. The tick computes its start time as:

```
since = monitor.watermark.unwrap_or(now - LOOKBACK_SECS)   // LOOKBACK_SECS = 3600 (1 hour)
```

So a brand-new monitor (watermark `NULL`) and a monitor migrated from an older DB (watermark
`NULL` after the additive migration) both **look back 1 hour on their first tick**, then carry
the watermark forward. There is **no separate "baseline" branch** — the first tick runs the
identical "fetch since `since`" logic, it just happens to start one hour back. A watchtower
starts watching *from now* (plus a 1-hour taste so the feed isn't empty on creation); it does
**not** backfill all of HN history. `create_monitor` is therefore unchanged beyond defaulting
the new field to `None`.

## Design

### A. Data model

- **`HnItem` gains `created_at: i64`** (Algolia's `created_at_i`, unix seconds). Needed to
  compute the watermark.
- **`monitors.watermark INTEGER` (nullable)** added via the existing idempotent
  `ensure_column` migration (`PRAGMA table_info` guard before `ALTER TABLE` — safe to run every
  launch, upgrades on-disk DBs to `NULL`). `Monitor` struct gains `watermark: Option<i64>`;
  `list_monitors` selects it; `create_monitor` sets `None`.
- New `db::set_watermark(conn, monitor_id, watermark: i64)` — one `UPDATE`.

### B. Paginated delta fetch — `hn::fetch_since`

```
hn::fetch_since(since: i64) -> Result<Vec<HnItem>, String>
```

Loops Algolia pages, newest-first:

```
search_by_date?tags=story&numericFilters=created_at_i>=SINCE&hitsPerPage=100&page=0,1,2,…
```

- Accumulate hits page by page; **stop** when a page returns fewer than `HITS_PER_PAGE`
  (last page) **or** at `MAX_PAGES` (Algolia caps retrievable hits at 1000; `100 × 10 = 1000`).
- If the `MAX_PAGES` cap is actually hit, **`eprintln!` a warning** — never silently truncate
  (a capped window is the one case where a story *could* still be dropped; it must be visible).
- `>=` (not `>`) plus the margin (§D) guarantees overlap at the boundary — no same-second miss.
- Concurrent inserts mid-pagination only push items to *later* pages (stories are never
  deleted), so we may **re-see** an item across a page boundary but never skip one — the
  duplicate is absorbed by dedupe (§C) / `seen` / `UNIQUE`. No fixed upper bound on the window
  is needed beyond the `MAX_PAGES` safety cap.

`hn::fetch_recent` is **removed** (its only caller was `run_tick`); `parse_algolia` is extended
to read `created_at_i` and keeps its unit test.

### C. Tick flow — chunked judge, fail-closed (`tick::run_tick`)

```
since   = monitor.watermark.unwrap_or(now - LOOKBACK_SECS)
recent  = hn::fetch_since(since).await?              // TickError::Hn on failure
recent  = dedupe_by_hn_id(recent)                    // drop cross-page dups (pure helper)
checked = recent.len()                               // honest "scanned" count (may be ≫30)
unseen  = select_unseen(recent, &seen)               // existing (b) dedup, unchanged
if unseen.is_empty() { return Ok(TickOutcome{checked, new:0, agent_ran:false}) }

matched = []
for batch in chunk(&unseen, BATCH_SIZE) {            // BATCH_SIZE = 30, pure helper
    let verdicts = agent::judge(&monitor.prompt, batch).await?;   // TickError::Agent — FAIL-CLOSED
    matched.extend(verdicts)
}
// all batches succeeded → commit, in this exact order (see §E):
rows = build_feed_rows(&monitor.id, &unseen, &matched, now)       // existing
1. insert each row      (INSERT OR IGNORE)
2. mark every unseen id seen
3. if let Some(wm) = advance_watermark(monitor.watermark, &recent, MARGIN, now) { set_watermark(wm) }  // LAST
Ok(TickOutcome{checked, new: rows.len(), agent_ran:true})
```

- **Chunking:** a burst is never crammed into one giant prompt. `unseen` is split into batches
  of 30 and judged in **separate `claude` calls**, run **sequentially within the tick**. The
  shared `agent_sem` (size 4) still bounds concurrency *across* monitors + the future swarm;
  sequential-within-a-tick keeps a monitor the "one tempo" so one burst can't grab all 4 permits
  and stall other monitors. (This is the deeper reason ~30 is a good number — a sane per-call
  batch size, not a coverage limit.)
- **Fail-closed:** the `?` on `judge` propagates the first batch failure **before any DB
  write** — nothing inserted, nothing marked seen, watermark **not** advanced. The whole window
  is re-judged next tick; already-succeeded items would be filtered by `seen` and the failed
  ones retried. Never-miss holds identically to a partial-commit, and it keeps `run_tick`'s
  clean `Result<TickOutcome, TickError>` signature (no refactor). We accept re-judging the
  earlier batches of the same window on the rare *burst-and-batch-failure* case as the price of
  that simplicity.
- **`agent_ran`** stays `true` only when at least one batch ran, preserving #3's Claude-health
  self-heal semantics (an all-seen early return is *not* evidence Claude works).

### D. Watermark advance — `advance_watermark` (pure)

```
advance_watermark(current: Option<i64>, items: &[HnItem], margin: i64, now: i64) -> Option<i64>
```

- Candidate = **max `created_at`** over `items`, **ignoring absurd values** (`created_at <= 0`
  or `created_at > now + CLOCK_SKEW`) so a poisoned/malformed hit can't rocket the watermark
  into the future (which would then skip everything).
- Returns `Some(max(current.unwrap_or(i64::MIN), candidate - margin))` — **monotonic** (never
  regresses) and set **`MARGIN` (= 300s / 5 min) behind** the newest story.
- If `items` yields no valid timestamp, returns **`None`** and the caller leaves the current
  watermark untouched (nothing to anchor to — including staying `NULL`).

**Why the trailing margin — the real correctness fix.** Algolia's indexing is asynchronous: a
story with an *older* `created_at_i` can be indexed *after* newer ones are already visible. A
watermark set to the exact max would sit *above* such a late-indexed story, which then falls
below the `>=` filter and is **missed forever** — precisely the failure this ticket exists to
kill. Advancing to `max − 5 min` means each tick re-fetches that 5-minute tail; already-handled
stories are recognized by `seen` (a free re-scan, no re-judging) and a late-indexed one is
caught. One subtraction buys robustness that doesn't depend on Algolia's internal ordering.

### E. Commit ordering guarantees crash-safety without a transaction

The commit writes in the order **insert feed → mark seen → advance watermark (last)**. This
ordering alone makes the tick crash-safe:

- Crash **after** mark-seen but **before** set-watermark → watermark unchanged, so next tick
  re-fetches the same window; the now-`seen` items are filtered, unseen empty, no re-judge, and
  the watermark advances on a later tick. **Safe.**
- The dangerous order — advancing the watermark *before* marking items seen — is simply never
  done, so no story can end up both below the watermark and absent from `seen`.

A `rusqlite` transaction around the three steps is an optional hardening (atomic all-or-nothing)
but is **not required** for correctness given the ordering; kept out of scope to match the
existing non-transactional feed+seen pattern.

### F. Constants (one place, top of the relevant module)

| name | value | meaning |
| --- | --- | --- |
| `LOOKBACK_SECS` | `3600` | first-tick look-back when watermark is `NULL` (1 hour) |
| `WATERMARK_MARGIN_SECS` | `300` | trailing margin behind newest story (5 min) |
| `BATCH_SIZE` | `30` | stories per `claude` call |
| `HITS_PER_PAGE` | `100` | Algolia page size |
| `MAX_PAGES` | `10` | safety cap (`100×10` = Algolia's 1000-hit limit); logged if hit |

## Non-goals (staying strictly in #2)

- **No partial-commit / per-batch error surfacing.** Fail-closed (§C); a batch failure discards
  that tick's partial work. "Analyzed exactly once" is guaranteed on the happy path, not on the
  error-retry path.
- **No #4 wall-clock/catch-up scheduling.** The tick still fires on the monotonic
  `tokio::time::sleep` cadence (#4 owns that). This ticket only changes *what a tick fetches*,
  not *when* it fires. (They compose later: a catch-up tick after a long sleep will already be
  lossless.)
- **No system tray / notifications (Phase 3), no dig-deeper swarm.**
- **No backfill of pre-monitor history** — the 1-hour look-back is the deliberate cap; missing
  stories from *before the monitor existed* is correct behavior, not a bug.
- No monitor edit/pause/"Run now".

## Testing

- **Rust unit tests (pure functions — the existing `parse_verdict`/`find_claude` seam style):**
  - `parse_algolia` carries `created_at` (from `created_at_i`); a hit missing it is handled
    (default `0`, which §D then ignores).
  - `dedupe_by_hn_id`: cross-page duplicate ids collapse to one (first kept); order preserved.
  - `chunk`: `[1..=65]`, size 30 → `[30, 30, 5]`; empty → `[]`; exact multiple → no trailing
    empty batch.
  - `advance_watermark`: advances to `max − margin`; **monotonic** (a lower batch max never
    regresses the watermark); ignores absurd `created_at` (`0`, far-future); `None` current +
    valid items → `Some(candidate − margin)`; no valid items → `None` (caller keeps current).
  - `select_unseen` (existing) still filters `seen`.
  - `db::set_watermark` round-trips; migration idempotency + pre-existing-DB upgrade already
    covered — extend one migration test to assert the new `watermark` column defaults to `NULL`
    and round-trips.
- **Live verification** in the native window per [`docs/TESTING.md`](../../TESTING.md):
  - **Normal:** create a monitor → first tick looks back 1 hour, judges, feed populates,
    watermark persists; restart → watermark survives, next tick pulls only the delta (no
    re-judge of old stories, no duplicate cards).
  - **Burst (the headline case):** point `fetch_since` at a low `since` (or a seeded old
    watermark) so a window of **>100 stories** is returned; confirm it **paginates** (multiple
    Algolia requests), judges in **batches of 30**, every story is scanned exactly once, and no
    duplicate feed cards. `checked` on the monitor tile reflects the full window (e.g. 150), not
    ~30 — confirm the #1 UI copy reads correctly at that size.
  - **Fail-closed:** force a batch failure mid-burst (e.g. `HN_WATCH_CLAUDE_BIN` fake that fails)
    → nothing committed that tick, watermark unchanged, error surfaced (via #3's path); on
    recovery the next tick re-judges the window cleanly with no duplicates.

## Files touched

- `src-tauri/src/hn.rs` — `HnItem.created_at`; `parse_algolia` reads `created_at_i`;
  `fetch_since` (paginated) replaces `fetch_recent`.
- `src-tauri/src/tick.rs` — `since = watermark.unwrap_or(now - LOOKBACK)`; `dedupe_by_hn_id`,
  `chunk`, `advance_watermark` pure helpers; chunked fail-closed judge loop; commit order
  insert→seen→watermark; constants.
- `src-tauri/src/db.rs` — `Monitor.watermark`; `ensure_column("watermark")`; `list_monitors`
  selects it; `set_watermark`.
- `src-tauri/src/commands.rs` — `create_monitor` defaults `watermark: None` (no behavior
  change).
- No frontend change required (the tile already renders `checked`; only its magnitude grows).
