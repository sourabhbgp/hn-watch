# Monitors + real tick loop — design

**Date:** 2026-07-08
**Branch:** `feat/monitors-and-tick`
**Requirement:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md) — this slice covers persistence,
monitor create/list/delete, and the long-lived background worker that ticks a monitor through
`claude -p` into the feed. Tray/notifications and the dig-deeper swarm are **later slices**.

## Goal (this slice)

Turn the static mock UI into a working core loop:

1. Create / list / delete monitors, persisted in SQLite (survive restart).
2. Each monitor runs as a long-lived background worker in Rust: on every tick it pulls recent
   Hacker News items, sends the unseen ones + the monitor's prompt to `claude -p` in one call,
   and appends the matches to the single feed.
3. Results are deduplicated against what's already been seen and persisted locally.

To keep it testable without extra UI, **each worker ticks immediately on create/startup, then
repeats on its interval** — real matches appear within seconds of creating a monitor.

## Decisions

- **Schedule** = interval presets (e.g. 15m / 30m / 1h / 6h), stored as `interval_secs`.
  No cron, no free-form input.
- **HN source** = Algolia HN Search API (`search_by_date?tags=story`) — one request returns
  recent stories with `objectID`, `title`, `url`, `points`, `num_comments`, `created_at`.
  No per-item N+1 fetch.
- **Runtime** = `claude -p --output-format json`, **one call per tick**, bounded by a global
  semaphore so monitor ticks (one at a time) and the future dig-deeper swarm (many at once)
  share a single agent runtime — the "same runtime at two tempos" the brief asks about.
- **Persistence** = `rusqlite` (bundled) with one connection held in Tauri state, so the
  background workers can write to it directly.

## Architecture — Rust core (`src-tauri/src/`)

Small, single-purpose modules:

| Module | One job |
| --- | --- |
| `db.rs` | Open SQLite, run schema migration, expose typed read/write helpers. |
| `hn.rs` | Fetch recent stories from Algolia → `Vec<HnItem>`. |
| `agent.rs` | The `claude -p` runtime: build the judging prompt, spawn the process, parse the JSON verdict. Bounded by a shared semaphore. Reused later by the swarm. |
| `scheduler.rs` | Spawn one async task per monitor (tick now → sleep(interval) → repeat); add on create, cancel on delete. |
| `commands.rs` | Tauri commands: `create_monitor`, `list_monitors`, `delete_monitor`, `list_feed`. |
| `lib.rs` | Wire state + plugins; on startup load monitors from DB and spawn their workers. |

## Data model (SQLite)

```sql
CREATE TABLE monitors (
  id           TEXT PRIMARY KEY,   -- uuid
  name         TEXT NOT NULL,
  prompt       TEXT NOT NULL,
  interval_secs INTEGER NOT NULL,
  created_at   INTEGER NOT NULL    -- unix seconds
);

CREATE TABLE feed_items (
  id           TEXT PRIMARY KEY,   -- uuid
  monitor_id   TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  hn_id        TEXT NOT NULL,
  title        TEXT NOT NULL,
  url          TEXT NOT NULL,
  domain       TEXT NOT NULL,
  summary      TEXT NOT NULL,
  reason       TEXT NOT NULL,      -- why the prompt matched
  hn_score     INTEGER NOT NULL,
  hn_comments  INTEGER NOT NULL,
  created_at   INTEGER NOT NULL
);

-- dedup: every HN item already judged for a monitor, match or not
CREATE TABLE seen (
  monitor_id   TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
  hn_id        TEXT NOT NULL,
  PRIMARY KEY (monitor_id, hn_id)
);
```

`seen` is the dedup key: it stops re-adding the same story to a feed **and** stops re-sending
already-judged items to `claude` (cost control). Both `feed_items` and `seen` cascade-delete
with their monitor.

## One tick (per monitor)

1. Fetch recent HN stories (`hn.rs`).
2. Drop any whose `hn_id` is already in `seen` for this monitor.
3. If nothing new → done. Otherwise send the unseen items + the monitor's prompt to `claude -p`
   in one call (`agent.rs`, semaphore-bounded).
4. Parse the JSON verdict: for each item, `{ hn_id, match: bool, summary, reason }`.
5. Insert matches into `feed_items`; insert **all** judged `hn_id`s into `seen`.
6. Emit a `feed-updated` Tauri event → the UI prepends the new cards.

Item cap per tick: a small bound (~30 most-recent unseen) to keep each `claude` call cheap.

## Frontend (`src/`) — reuse existing components + design tokens

- `App.tsx`: replace mock arrays with `invoke('list_monitors')` / `invoke('list_feed')` on mount,
  and a listener for `feed-updated` that refreshes the feed.
- `Sidebar.tsx`: "New monitor" opens a small inline form (name + prompt + interval `<select>`)
  → `invoke('create_monitor')`. Delete affordance → `invoke('delete_monitor')`.
- No visual redesign; existing `FeedCard` / `Feed` / `DigDeeperPanel` stay as-is (dig-deeper
  keeps its mock data this slice).

## Error handling

- HN fetch fails, `claude` errors/times out, or returns unparseable JSON → log and skip that
  tick; the worker survives and ticks again next interval.
- DB errors surface to the UI as a rejected command.
- No monitor "error" status UI in this slice — a failed tick is just logged.

## Out of scope

- **Later slices (roadmap):** system tray + native notifications; the dig-deeper research swarm.
- **Not built (over-building):** monitor edit/pause/status management, a manual "Run now" button,
  feed search/filter, settings screens.

## Testing

Native window per [`docs/TESTING.md`](../../TESTING.md): create a monitor → confirm real matches
land within seconds (immediate first tick) → restart the app → confirm monitors + feed persist.
