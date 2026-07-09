# Tick observability — design

**Date:** 2026-07-09
**Ticket:** [`docs/TODO.md`](../../TODO.md) #1
**Branch:** `feat/tick-observability`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

A monitor is a black box. On create there is no loading indicator, no "checking…"
state, no "last checked" time, and no "0 new" message. A tick that finds nothing (or
fails) leaves the feed blank with no explanation — you cannot tell "working, nothing
matched" from "broken." Confirmed live: a "Claude" monitor correctly checked 30 stories
and matched 0, but the UI showed nothing, so it *looked* dead.

## Goal (acceptance)

From the UI alone you can always tell, per monitor: a live countdown to the next check;
whether it is checking right now; when it last checked; how many stories it scanned; how
many were new; and whether the last tick errored. State survives an app restart.

## Scope

**In:** live `next in Xm` countdown, transient `Checking…` chip, `last checked · N · M new`
status line, an `error` chip + reason when the last tick failed, and a feed empty-state
message that reflects the last check.

**Out (owned by other tickets, left as no-op placeholders):**
- `Paused` chip (offline / Claude unavailable) → TODO #3.
- `Resumed · catching up` chip (wall-clock overdue after laptop wake) → TODO #4.
- "Run now" / edit / pause controls → deliberately not built (see STATUS.md).

## Key decision: what "checked N" counts

`checkedCount` = **stories scanned this tick** = `recent.len()` (the HN batch pulled,
typically 30), **not** only the unseen ones. Rationale: the doc's "checked 30 · 0 new"
reassurance is about proving the monitor is actively scanning even when nothing is new. A
steady tick where all 30 were already seen still honestly reports "scanned 30, 0 new."

## Design

### A. Data — persist tick results (survives restart)

Add four **nullable** columns to the `monitors` table via an **idempotent additive
migration**: read `PRAGMA table_info(monitors)`, and `ALTER TABLE monitors ADD COLUMN`
only for columns not already present (SQLite has no `ADD COLUMN IF NOT EXISTS`; existing
on-disk DBs must keep working and the migration must be safe to run every launch).

| column | type | meaning |
| --- | --- | --- |
| `last_checked_at` | INTEGER (epoch s) | when the last tick finished; null = never checked |
| `last_checked_count` | INTEGER | stories scanned in the last tick |
| `last_new_count` | INTEGER | new matches inserted in the last tick |
| `last_error` | TEXT | failure reason from the last tick; null = ok |

New `db::record_tick(conn, monitor_id, checked, new, error: Option<&str>, now)` writes all
four in one `UPDATE`. The `Monitor` struct gains these (optional) fields; `list_monitors`
selects them.

### B. Backend flow

- `tick.rs`: `run_tick` returns `TickOutcome { checked: usize, new: usize }` instead of
  bare `usize`. `checked = recent.len()`. It still early-returns (`new: 0`) when every
  story is already seen — with `checked` set to the fetched count.
- `scheduler.rs` owns the observable side-effects around each tick:
  1. emit **`tick-started { monitorId }`**
  2. run the tick
  3. `db::record_tick(...)` — success counts on `Ok`, the error string on `Err`
  4. emit **`tick-finished { monitorId, checkedCount, newCount, error? }`**
  5. keep the existing `feed-updated` emit, still only when `new > 0`
- A failed tick still never kills the worker (unchanged) — it is now merely *visible*.

### C. DTO / API

`MonitorDto` (and mirror `src/types.ts` `Monitor`) gain:

| field | type | notes |
| --- | --- | --- |
| `lastCheckedAt` | `number \| null` | epoch seconds |
| `nextCheckAt` | `number \| null` | `last_checked_at + interval_secs`; null if never checked |
| `lastCheckedCount` | `number \| null` | |
| `lastNewCount` | `number \| null` | |
| `lastError` | `string \| null` | |

Raw epoch seconds / numbers cross the boundary — **the client formats** the clock time and
the countdown (a server-formatted "3:31 PM" cannot tick down). `status` is derived
server-side: `"error"` if `last_error` is set, else `"active"`. `"checking"` is a
**client-only transient overlay**, never persisted.

New event listeners in `api.ts`: `onTickStarted`, `onTickFinished`.

### D. UI

- **Countdown — client-side timer** (chosen over server-pushed ticks: `nextCheckAt` is a
  known absolute time, so a local clock renders it exactly with zero backend chatter). One
  `now` state in `App`, updated every ~15s via `setInterval`, passed to `Sidebar`.
- `Sidebar` `MonitorRow` renders:
  - chip: `Checking…` when the monitor's id is in a `checkingIds` set; else `next in Xm`
    / `next in <1m` / `due now` computed from `nextCheckAt` and `now`; `error` chip when
    `status === "error"` (title/tooltip carries `lastError`).
  - status line: `Last checked 3:31 PM · 30 · 0 new`, or `Never checked` before the first
    tick completes.
  - the status dot keeps its existing color mapping (`active`/`error`).
- `App` maintains `checkingIds: Set<string>` from `tick-started` (add) / `tick-finished`
  (delete). On `tick-finished` it refreshes `listMonitors()` to pull the newly persisted
  stats; the feed still refreshes off `feed-updated`.
- `Feed` empty state: when a monitor is selected, has `lastCheckedAt != null`, and shows 0
  matches → `Checked N stories, nothing matched yet` (+ last-checked time), instead of the
  generic "No matches yet." The all-monitors view keeps the generic line.

Reuse existing design tokens (`bg-ok`/`bg-rust`/`text-faint`, mono type scale) — no new
colors or hardcoded values.

## Known limitation (documented, owned by TODO #4)

The scheduler still sleeps on a **monotonic** timer, so after a laptop sleep the wall-clock
countdown and the real next tick can drift. #1 only exposes an honest `nextCheckAt`; making
the schedule itself wall-clock/catch-up correct is TODO #4.

## Testing

- **Rust unit tests:**
  - `record_tick` round-trips the four fields (success path) and stores an error string
    (failure path); `last_error` clears to null on a subsequent success.
  - the additive migration is idempotent — running `migrate` twice on the same connection
    does not error on duplicate columns; a pre-existing DB without the columns gains them.
  - `next_check_at` = `last_checked_at + interval_secs`; null when never checked.
- **Live verification** in the native window per [`docs/TESTING.md`](../../TESTING.md):
  create a monitor → observe `Checking…` → status line populates with scanned/new counts →
  countdown ticks down → restart the app and confirm the last-checked stats persist.

## Files touched

- `src-tauri/src/db.rs` — migration, `Monitor` fields, `record_tick`, `list_monitors`
- `src-tauri/src/tick.rs` — `TickOutcome`, `run_tick` return
- `src-tauri/src/scheduler.rs` — event emits + `record_tick` around the tick
- `src-tauri/src/commands.rs` — `MonitorDto` fields, `to_monitor_dto`, `next_check_at`
- `src/types.ts`, `src/api.ts` — mirror fields + tick event listeners
- `src/App.tsx` — `now` ticker, `checkingIds`, tick event wiring
- `src/components/Sidebar.tsx` — chip, countdown, status line
- `src/components/Feed.tsx` — empty-state message
