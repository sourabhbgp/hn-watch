# Tick Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each monitor's activity visible in the UI — a live countdown to the next check, a transient "Checking…" chip, a "last checked · scanned · new" status line, an error chip, and a feed empty-state that reflects the last check.

**Architecture:** Persist per-tick results (`last_checked_at`, counts, error) on the `monitors` table via an idempotent additive migration. The scheduler emits `tick-started`/`tick-finished` events around each tick and records results. Commands expose the raw epoch/count fields (plus a derived `next_check_at`); the React client formats the clock time and counts down locally against a 15s `now` ticker.

**Tech Stack:** Rust (Tauri 2, rusqlite), React 19 + TypeScript, Tailwind v4.

## Global Constraints

- Reuse existing design tokens in `src/index.css` / `docs/design.md` — never hardcode colors, fonts, or spacing (`bg-ok`, `bg-rust`, `text-faint`, `text-soft`, `font-mono`, existing type scale).
- New columns are **nullable**; the migration must be safe to run on every launch against an existing on-disk DB (SQLite has no `ADD COLUMN IF NOT EXISTS`).
- DTO fields cross the boundary as raw epoch seconds / numbers / nullable — the **client** formats time and countdown.
- `"checked N"` = stories **scanned** that tick = `recent.len()` (the HN batch, ~30), not only the unseen ones.
- Scope: `Paused` and `Resumed · catching up` chips are **out** (TODO #3/#4). Only Checking / countdown / error / status line / empty-state.
- Backend tests run via `cd src-tauri && cargo test`. Frontend has no unit-test harness — verify types with `npx tsc --noEmit` and behavior in the native window per `docs/TESTING.md`.

---

### Task 1: DB layer — additive migration, `Monitor` fields, `record_tick`

**Files:**
- Modify: `src-tauri/src/db.rs`

**Interfaces:**
- Produces:
  - `Monitor` gains `last_checked_at: Option<i64>`, `last_checked_count: Option<i64>`, `last_new_count: Option<i64>`, `last_error: Option<String>`.
  - `pub fn record_tick(conn: &Connection, monitor_id: &str, checked: i64, new: i64, error: Option<&str>, now: i64) -> rusqlite::Result<()>`
  - `migrate` adds the four nullable columns idempotently; `list_monitors` selects them.

- [ ] **Step 1: Add the four fields to the `Monitor` struct**

In `src-tauri/src/db.rs`, replace the `Monitor` struct:

```rust
#[derive(Debug, Clone)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub interval_secs: i64,
    pub created_at: i64,
    pub last_checked_at: Option<i64>,
    pub last_checked_count: Option<i64>,
    pub last_new_count: Option<i64>,
    pub last_error: Option<String>,
}
```

- [ ] **Step 2: Add the idempotent column-adder and call it from `migrate`**

Add this helper above `migrate`, and append the four `ensure_column` calls to the end of `migrate` (after the existing `execute_batch`):

```rust
/// Add `column` to `table` only if it isn't already present. SQLite has no
/// `ADD COLUMN IF NOT EXISTS`, and existing on-disk DBs must upgrade safely.
/// table/column/decl are static literals here (never user input).
fn ensure_column(conn: &Connection, table: &str, column: &str, decl: &str) -> rusqlite::Result<()> {
    let present = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|name| name == column);
    if !present {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), [])?;
    }
    Ok(())
}
```

Change `migrate` to run the batch, then the columns:

```rust
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS monitors (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             prompt TEXT NOT NULL,
             interval_secs INTEGER NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS feed_items (
             id TEXT PRIMARY KEY,
             monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
             hn_id TEXT NOT NULL,
             title TEXT NOT NULL,
             url TEXT NOT NULL,
             domain TEXT NOT NULL,
             summary TEXT NOT NULL,
             reason TEXT NOT NULL,
             hn_score INTEGER NOT NULL,
             hn_comments INTEGER NOT NULL,
             created_at INTEGER NOT NULL,
             UNIQUE(monitor_id, hn_id)
         );
         CREATE TABLE IF NOT EXISTS seen (
             monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
             hn_id TEXT NOT NULL,
             PRIMARY KEY (monitor_id, hn_id)
         );",
    )?;
    ensure_column(conn, "monitors", "last_checked_at", "INTEGER")?;
    ensure_column(conn, "monitors", "last_checked_count", "INTEGER")?;
    ensure_column(conn, "monitors", "last_new_count", "INTEGER")?;
    ensure_column(conn, "monitors", "last_error", "TEXT")?;
    Ok(())
}
```

- [ ] **Step 3: Select the new columns in `list_monitors`**

Replace `list_monitors`:

```rust
pub fn list_monitors(conn: &Connection) -> rusqlite::Result<Vec<Monitor>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, prompt, interval_secs, created_at,
                last_checked_at, last_checked_count, last_new_count, last_error
         FROM monitors ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Monitor {
            id: r.get(0)?,
            name: r.get(1)?,
            prompt: r.get(2)?,
            interval_secs: r.get(3)?,
            created_at: r.get(4)?,
            last_checked_at: r.get(5)?,
            last_checked_count: r.get(6)?,
            last_new_count: r.get(7)?,
            last_error: r.get(8)?,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 4: Add `record_tick`**

Add after `mark_seen`:

```rust
/// Record the outcome of one tick. Passing `error: None` clears any prior error.
pub fn record_tick(
    conn: &Connection,
    monitor_id: &str,
    checked: i64,
    new: i64,
    error: Option<&str>,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE monitors
         SET last_checked_at = ?2, last_checked_count = ?3,
             last_new_count = ?4, last_error = ?5
         WHERE id = ?1",
        rusqlite::params![monitor_id, now, checked, new, error],
    )?;
    Ok(())
}
```

- [ ] **Step 5: Fix the test `sample_monitor` helper and write the failing tests**

In the `#[cfg(test)] mod tests`, replace `sample_monitor` so it compiles with the new struct, and add three tests:

```rust
fn sample_monitor(id: &str) -> Monitor {
    Monitor {
        id: id.into(),
        name: "AI".into(),
        prompt: "ai agents".into(),
        interval_secs: 1800,
        created_at: 100,
        last_checked_at: None,
        last_checked_count: None,
        last_new_count: None,
        last_error: None,
    }
}

#[test]
fn record_tick_stores_and_clears_error() {
    let c = mem();
    insert_monitor(&c, &sample_monitor("m1")).unwrap();
    record_tick(&c, "m1", 10, 0, Some("claude timed out"), 111).unwrap();
    let m = list_monitors(&c).unwrap().pop().unwrap();
    assert_eq!(m.last_checked_at, Some(111));
    assert_eq!(m.last_checked_count, Some(10));
    assert_eq!(m.last_error, Some("claude timed out".into()));
    // a later success clears the error
    record_tick(&c, "m1", 12, 3, None, 222).unwrap();
    let m = list_monitors(&c).unwrap().pop().unwrap();
    assert_eq!(m.last_error, None);
    assert_eq!(m.last_new_count, Some(3));
}

#[test]
fn migrate_is_idempotent() {
    let c = Connection::open_in_memory().unwrap();
    migrate(&c).unwrap();
    migrate(&c).unwrap(); // second run must not error on ADD COLUMN
    insert_monitor(&c, &sample_monitor("m1")).unwrap();
    record_tick(&c, "m1", 30, 2, None, 555).unwrap();
    let m = list_monitors(&c).unwrap().pop().unwrap();
    assert_eq!(m.last_checked_count, Some(30));
}

#[test]
fn migrate_upgrades_preexisting_db_without_new_columns() {
    let c = Connection::open_in_memory().unwrap();
    // simulate an old (pre-observability) schema
    c.execute_batch(
        "CREATE TABLE monitors (
             id TEXT PRIMARY KEY, name TEXT NOT NULL, prompt TEXT NOT NULL,
             interval_secs INTEGER NOT NULL, created_at INTEGER NOT NULL);",
    ).unwrap();
    c.execute("INSERT INTO monitors VALUES ('m1','n','p',1800,100)", []).unwrap();
    migrate(&c).unwrap(); // must ADD the 4 columns, keep the row
    let m = list_monitors(&c).unwrap().pop().unwrap();
    assert_eq!(m.id, "m1");
    assert_eq!(m.last_checked_at, None);
    record_tick(&c, "m1", 5, 1, Some("boom"), 200).unwrap();
    let m = list_monitors(&c).unwrap().pop().unwrap();
    assert_eq!(m.last_error, Some("boom".into()));
}
```

- [ ] **Step 6: Run tests — expect a COMPILE failure first (other files reference `Monitor`)**

Run: `cd src-tauri && cargo test db::`
Expected: compile errors in `commands.rs` (constructs `Monitor` without the new fields) and possibly `tick.rs`. That is expected — Task 4 fixes `commands.rs`. To test Task 1 in isolation, temporarily also apply the `commands.rs` create_monitor field additions from Task 4 Step 2, or run the full build after Task 4. If you are doing strict task isolation, proceed to Step 7 knowing the db module itself is correct and will compile once callers are updated.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "feat(db): persist per-tick results + idempotent additive migration

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `tick.rs` — return a `TickOutcome`

**Files:**
- Modify: `src-tauri/src/tick.rs`

**Interfaces:**
- Consumes: `db::Monitor` (Task 1).
- Produces: `pub struct TickOutcome { pub checked: usize, pub new: usize }`; `run_tick(...) -> Result<TickOutcome, String>`.

- [ ] **Step 1: Add `TickOutcome` and change `run_tick`**

Add the struct near the top of `src-tauri/src/tick.rs` (below the `use` lines):

```rust
/// What one tick did: how many stories it scanned and how many new matches it inserted.
pub struct TickOutcome {
    pub checked: usize,
    pub new: usize,
}
```

Replace `run_tick`:

```rust
pub async fn run_tick(db: &Arc<Mutex<Connection>>, monitor: &Monitor) -> Result<TickOutcome, String> {
    let recent = hn::fetch_recent(30).await?;
    let checked = recent.len();

    let seen = {
        let conn = db.lock().map_err(|_| "db poisoned".to_string())?;
        db::list_seen(&conn, &monitor.id).map_err(|e| e.to_string())?
    };
    let unseen = select_unseen(recent, &seen);
    if unseen.is_empty() {
        return Ok(TickOutcome { checked, new: 0 });
    }

    let verdicts = agent::judge(&monitor.prompt, &unseen).await?;
    let rows = build_feed_rows(&monitor.id, &unseen, &verdicts, now_secs());

    let conn = db.lock().map_err(|_| "db poisoned".to_string())?;
    for row in &rows {
        db::insert_feed_item(&conn, row).map_err(|e| e.to_string())?;
    }
    for item in &unseen {
        db::mark_seen(&conn, &monitor.id, &item.hn_id).map_err(|e| e.to_string())?;
    }
    Ok(TickOutcome { checked, new: rows.len() })
}
```

(The existing `select_unseen` / `build_feed_rows` tests are unchanged and still pass.)

- [ ] **Step 2: Run the existing tick tests to confirm no regression**

Run: `cd src-tauri && cargo test tick::`
Expected: the three existing tests still PASS (they don't call `run_tick`). Compile of `scheduler.rs` will fail until Task 3 — that's expected under strict isolation.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/tick.rs
git commit -m "feat(tick): return TickOutcome { checked, new } from run_tick

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `scheduler.rs` — emit tick events + record results

**Files:**
- Modify: `src-tauri/src/scheduler.rs`

**Interfaces:**
- Consumes: `tick::run_tick -> Result<TickOutcome, String>` (Task 2), `tick::now_secs`, `db::record_tick` (Task 1).
- Produces: emits `tick-started {monitorId}`, `tick-finished {monitorId, checkedCount, newCount, error?}`, keeps `feed-updated`.

- [ ] **Step 1: Add event payload structs**

At the top of `src-tauri/src/scheduler.rs`, after the existing `use` lines, add:

```rust
use crate::db;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TickStarted {
    monitor_id: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TickFinished {
    monitor_id: String,
    checked_count: i64,
    new_count: i64,
    error: Option<String>,
}
```

- [ ] **Step 2: Rewrite the worker loop in `spawn`**

Replace the `let handle = tauri::async_runtime::spawn(async move { ... });` block with:

```rust
let handle = tauri::async_runtime::spawn(async move {
    loop {
        let _ = app.emit("tick-started", TickStarted { monitor_id: monitor.id.clone() });

        let result = tick::run_tick(&db, &monitor).await;
        // Record at finish time so next_check_at aligns with the sleep(interval) below.
        let now = tick::now_secs();
        let (checked, new, error) = match &result {
            Ok(o) => (o.checked as i64, o.new as i64, None),
            Err(e) => {
                eprintln!("[hn-watch] tick failed for {}: {e}", monitor.id);
                (0i64, 0i64, Some(e.clone()))
            }
        };

        if let Ok(conn) = db.lock() {
            let _ = db::record_tick(&conn, &monitor.id, checked, new, error.as_deref(), now);
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
```

- [ ] **Step 3: Compile the crate**

Run: `cd src-tauri && cargo build`
Expected: fails only in `commands.rs` (Monitor construction) until Task 4; the scheduler/tick/db modules compile.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/scheduler.rs
git commit -m "feat(scheduler): emit tick-started/tick-finished + persist tick results

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `commands.rs` — DTO fields, derived `next_check_at`, status

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `db::Monitor` fields (Task 1).
- Produces: `MonitorDto` gains `lastCheckedAt`, `nextCheckAt`, `lastCheckedCount`, `lastNewCount`, `lastError`. Pure helper `fn next_check_at(last_checked_at: Option<i64>, interval_secs: i64) -> Option<i64>`.

- [ ] **Step 1: Extend `MonitorDto` and add the `next_check_at` helper**

Replace the `MonitorDto` struct:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDto {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub interval_label: String,
    pub status: String,
    pub match_count: i64,
    pub last_checked_at: Option<i64>,
    pub next_check_at: Option<i64>,
    pub last_checked_count: Option<i64>,
    pub last_new_count: Option<i64>,
    pub last_error: Option<String>,
}
```

Add this helper next to `interval_label`:

```rust
fn next_check_at(last_checked_at: Option<i64>, interval_secs: i64) -> Option<i64> {
    last_checked_at.map(|t| t + interval_secs)
}
```

Replace `to_monitor_dto`:

```rust
fn to_monitor_dto(conn: &Connection, m: &Monitor) -> rusqlite::Result<MonitorDto> {
    Ok(MonitorDto {
        id: m.id.clone(),
        name: m.name.clone(),
        prompt: m.prompt.clone(),
        interval_label: interval_label(m.interval_secs),
        status: if m.last_error.is_some() { "error" } else { "active" }.into(),
        match_count: db::count_matches(conn, &m.id)?,
        last_checked_at: m.last_checked_at,
        next_check_at: next_check_at(m.last_checked_at, m.interval_secs),
        last_checked_count: m.last_checked_count,
        last_new_count: m.last_new_count,
        last_error: m.last_error.clone(),
    })
}
```

- [ ] **Step 2: Initialize the new fields in `create_monitor`**

In `create_monitor`, replace the `Monitor { ... }` literal:

```rust
let monitor = Monitor {
    id: Uuid::new_v4().to_string(),
    name: name.trim().to_string(),
    prompt: prompt.trim().to_string(),
    interval_secs: interval_secs.max(60),
    created_at: now_secs(),
    last_checked_at: None,
    last_checked_count: None,
    last_new_count: None,
    last_error: None,
};
```

- [ ] **Step 3: Write the failing test for `next_check_at`**

Add to `#[cfg(test)] mod tests` in `commands.rs`:

```rust
#[test]
fn next_check_at_adds_interval_or_none() {
    assert_eq!(next_check_at(Some(1000), 1800), Some(2800));
    assert_eq!(next_check_at(None, 1800), None);
}
```

- [ ] **Step 4: Run the full backend test suite (Tasks 1–4 now compile together)**

Run: `cd src-tauri && cargo test`
Expected: PASS — including `record_tick_stores_and_clears_error`, `migrate_is_idempotent`, `migrate_upgrades_preexisting_db_without_new_columns`, `next_check_at_adds_interval_or_none`, and all pre-existing tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(commands): expose last-checked stats + derived next_check_at on MonitorDto

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Frontend types + API event listeners

**Files:**
- Modify: `src/types.ts`, `src/api.ts`

**Interfaces:**
- Produces: `Monitor` gains `lastCheckedAt`, `nextCheckAt`, `lastCheckedCount`, `lastNewCount`, `lastError` (all `number | null` except `lastError: string | null`). `onTickStarted(cb)`, `onTickFinished(cb)`.

- [ ] **Step 1: Extend the `Monitor` interface**

In `src/types.ts`, replace the `Monitor` interface:

```ts
export interface Monitor {
  id: string;
  name: string;
  prompt: string;
  intervalLabel: string; // e.g. "every 30m"
  status: MonitorStatus;
  matchCount: number;
  lastCheckedAt: number | null; // epoch seconds
  nextCheckAt: number | null; // epoch seconds
  lastCheckedCount: number | null;
  lastNewCount: number | null;
  lastError: string | null;
}
```

- [ ] **Step 2: Add the tick event listeners**

In `src/api.ts`, add below `onFeedUpdated`:

```ts
export interface TickFinished {
  monitorId: string;
  checkedCount: number;
  newCount: number;
  error: string | null;
}

// Fires when a monitor begins a tick. Returns an unlisten function.
export const onTickStarted = (cb: (monitorId: string) => void) =>
  listen<{ monitorId: string }>("tick-started", (e) => cb(e.payload.monitorId));

// Fires when a monitor finishes a tick (even with 0 new). Returns an unlisten function.
export const onTickFinished = (cb: (p: TickFinished) => void) =>
  listen<TickFinished>("tick-finished", (e) => cb(e.payload));
```

- [ ] **Step 3: Type-check**

Run: `npx tsc --noEmit`
Expected: PASS (no type errors).

- [ ] **Step 4: Commit**

```bash
git add src/types.ts src/api.ts
git commit -m "feat(ui): monitor tick-stat fields + tick event listeners

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `App.tsx` — `now` ticker, `checkingIds`, event wiring

**Files:**
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `onTickStarted`, `onTickFinished`, `listMonitors` (Task 5).
- Produces: passes `now: number` and `checkingIds: Set<string>` to `Sidebar`.

- [ ] **Step 1: Add state, the 15s ticker, and event wiring**

In `src/App.tsx`, update the import line to include the new API functions:

```tsx
import { listMonitors, listFeed, createMonitor, deleteMonitor, onFeedUpdated, onTickStarted, onTickFinished } from "./api";
```

Add state below the existing `useState` calls:

```tsx
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [checkingIds, setCheckingIds] = useState<Set<string>>(new Set());
```

Replace the existing effect that calls `refresh()` / `onFeedUpdated` with:

```tsx
  useEffect(() => {
    refresh();
    const uFeed = onFeedUpdated(() => refresh());
    const uStart = onTickStarted((id) =>
      setCheckingIds((s) => new Set(s).add(id)),
    );
    const uFinish = onTickFinished(({ monitorId }) => {
      setCheckingIds((s) => {
        const n = new Set(s);
        n.delete(monitorId);
        return n;
      });
      // pull the freshly persisted stats for this tick
      listMonitors().then(setMonitors);
    });
    const tick = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 15000);
    return () => {
      uFeed.then((f) => f());
      uStart.then((f) => f());
      uFinish.then((f) => f());
      clearInterval(tick);
    };
  }, []);
```

- [ ] **Step 2: Pass `now` and `checkingIds` to `Sidebar`**

In the JSX, update the `<Sidebar .../>` usage to add two props:

```tsx
      <Sidebar
        monitors={monitors}
        selectedId={selectedMonitorId}
        onSelect={setSelectedMonitorId}
        onCreate={handleCreate}
        onDelete={handleDelete}
        now={now}
        checkingIds={checkingIds}
      />
```

- [ ] **Step 3: Type-check (expects Sidebar prop error until Task 7)**

Run: `npx tsc --noEmit`
Expected: a type error that `Sidebar` does not accept `now` / `checkingIds` — resolved in Task 7. If doing strict isolation, implement Task 7 before re-running.

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): 15s now ticker + checkingIds + tick event wiring

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: `Sidebar.tsx` — chip, live countdown, status line

**Files:**
- Modify: `src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: `Monitor` stat fields, `now`, `checkingIds` (Tasks 5–6).

- [ ] **Step 1: Add formatting helpers and accept new props**

At the top of `src/components/Sidebar.tsx` (below the imports), add:

```tsx
function fmtClock(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function fmtCountdown(nextCheckAt: number | null, now: number): string {
  if (nextCheckAt == null) return "scheduling…";
  const rem = nextCheckAt - now;
  if (rem <= 0) return "due now";
  if (rem < 60) return "next in <1m";
  return `next in ${Math.round(rem / 60)}m`;
}

function statusLine(m: Monitor, checking: boolean): string {
  if (checking) return "Checking…";
  if (m.lastError) return "Last tick failed";
  if (m.lastCheckedAt == null) return "Never checked";
  return `Last checked ${fmtClock(m.lastCheckedAt)} · ${m.lastCheckedCount ?? 0} · ${m.lastNewCount ?? 0} new`;
}
```

- [ ] **Step 2: Extend `MonitorRow` props and render the chip + status line**

Replace the `MonitorRow` component with (adds `now` + `checking`, replaces the bottom `intervalLabel` line with a status/chip row):

```tsx
function MonitorRow({
  monitor,
  selected,
  now,
  checking,
  onSelect,
  onDelete,
}: {
  monitor: Monitor;
  selected: boolean;
  now: number;
  checking: boolean;
  onSelect: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      className={`group w-full rounded-lg px-3 py-2.5 transition-colors border ${
        selected ? "bg-hn-soft border-hn-border" : "bg-transparent border-transparent hover:bg-card"
      }`}
    >
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 shrink-0 rounded-full ${STATUS_DOT[monitor.status]}`} />
        <button onClick={onSelect} className="truncate text-left text-[13.5px] font-semibold text-ink">
          {monitor.name}
        </button>
        <span className="ml-auto shrink-0 rounded-full bg-paper px-1.5 py-0.5 font-mono text-[10px] text-faint">
          {monitor.matchCount}
        </span>
        <button
          onClick={onDelete}
          title="Delete monitor"
          className="shrink-0 text-faint opacity-0 transition-opacity hover:text-rust group-hover:opacity-100"
        >
          ×
        </button>
      </div>
      <button onClick={onSelect} className="block w-full text-left">
        <p className="mt-1 line-clamp-2 pl-4 text-[11.5px] leading-snug text-faint">{monitor.prompt}</p>
        <div className="mt-1.5 flex items-center gap-2 pl-4">
          {checking ? (
            <span className="shrink-0 rounded-full bg-hn-soft px-1.5 py-0.5 font-mono text-[10px] text-rust">
              <span className="mr-1 inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-rust align-middle" />
              Checking…
            </span>
          ) : monitor.status === "error" ? (
            <span
              title={monitor.lastError ?? ""}
              className="shrink-0 rounded-full bg-paper px-1.5 py-0.5 font-mono text-[10px] text-rust"
            >
              error
            </span>
          ) : (
            <span className="shrink-0 rounded-full bg-paper px-1.5 py-0.5 font-mono text-[10px] text-faint">
              {fmtCountdown(monitor.nextCheckAt, now)}
            </span>
          )}
          <span className="truncate font-mono text-[10.5px] text-faint">{statusLine(monitor, checking)}</span>
        </div>
      </button>
    </div>
  );
}
```

- [ ] **Step 3: Thread `now` / `checkingIds` through `Sidebar` into each row**

Update the `Sidebar` function signature to accept the two new props, and pass them to each `MonitorRow`.

Change the destructured props and type:

```tsx
export function Sidebar({
  monitors,
  selectedId,
  onSelect,
  onCreate,
  onDelete,
  now,
  checkingIds,
}: {
  monitors: Monitor[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onCreate: (name: string, prompt: string, intervalSecs: number) => void;
  onDelete: (id: string) => void;
  now: number;
  checkingIds: Set<string>;
}) {
```

Update the `.map` that renders rows:

```tsx
        {monitors.map((m) => (
          <MonitorRow
            key={m.id}
            monitor={m}
            selected={selectedId === m.id}
            now={now}
            checking={checkingIds.has(m.id)}
            onSelect={() => onSelect(m.id)}
            onDelete={() => onDelete(m.id)}
          />
        ))}
```

- [ ] **Step 4: Type-check**

Run: `npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/Sidebar.tsx
git commit -m "feat(ui): monitor status chip, live countdown, and last-checked status line

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: `Feed.tsx` — check-aware empty state

**Files:**
- Modify: `src/components/Feed.tsx`

**Interfaces:**
- Consumes: `activeMonitor: Monitor | null` (already a prop) with its stat fields.

- [ ] **Step 1: Add the empty-message helper and use it**

At the top of `src/components/Feed.tsx` (below the imports), add:

```tsx
function emptyMessage(m: Monitor | null): string {
  if (m && m.lastCheckedAt != null) {
    return `Checked ${m.lastCheckedCount ?? 0} stories, nothing matched yet.`;
  }
  if (m) return "Checking…";
  return "No matches yet.";
}
```

Replace the empty-state block in the JSX:

```tsx
          {items.length === 0 ? (
            <div className="mt-20 text-center text-[13px] text-faint">
              {emptyMessage(activeMonitor)}
            </div>
          ) : (
```

- [ ] **Step 2: Type-check**

Run: `npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/Feed.tsx
git commit -m "feat(ui): feed empty-state reflects the monitor's last check

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Full-build gate, live verification, STATUS + merge

**Files:**
- Modify: `STATUS.md`

- [ ] **Step 1: Backend + frontend build gates**

Run: `cd src-tauri && cargo test && cargo build`
Expected: all tests PASS, build succeeds.

Run (from repo root): `npm run build`
Expected: `tsc` + `vite build` succeed with no errors.

- [ ] **Step 2: Live verification in the native window** (per `docs/TESTING.md`)

Launch the native app (`npm run tauri dev`), then via computer-use:
1. Create a monitor → the row shows a **Checking…** chip immediately.
2. When the first tick finishes, the chip becomes **next in Xm** and the status line reads **Last checked H:MM · N · M new**.
3. Wait/observe the countdown decrement across the 15s ticker.
4. Select a monitor that matched 0 → the feed pane reads **Checked N stories, nothing matched yet.** (not blank).
5. Quit and relaunch → the last-checked status line persists (loaded from SQLite).

Capture a screenshot as evidence.

- [ ] **Step 3: Update `STATUS.md`**

Add a new session section summarizing: persisted per-tick results + idempotent additive migration; `tick-started`/`tick-finished` events; DTO stat fields + derived `next_check_at`; sidebar chip/countdown/status line; check-aware feed empty state. Note the known limitation (monotonic scheduler → countdown may drift after sleep; owned by TODO #4). Move TODO #1 out of the "pick one" backlog note.

- [ ] **Step 4: Commit STATUS**

```bash
git add STATUS.md
git commit -m "docs: STATUS — tick observability (TODO #1) shipped

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 5: Push the branch and merge to main (per CLAUDE.md workflow)**

```bash
git push -u origin feat/tick-observability
git checkout main
git merge --no-ff feat/tick-observability -m "Merge feat/tick-observability: tick observability (TODO #1)"
git push origin main
```

(Keep the `feat/tick-observability` branch on origin — never delete after merge.)

---

## Self-Review

**Spec coverage:**
- Persist tick results / survives restart → Task 1 (columns, `record_tick`, `list_monitors`) + Task 9 Step 2.5.
- Idempotent additive migration → Task 1 Step 2 + two migration tests.
- `checked = recent.len()` semantics → Task 2.
- `tick-started`/`tick-finished` + keep `feed-updated` + record on error → Task 3.
- DTO fields + `next_check_at` + status derivation → Task 4.
- Client formats time/countdown → Tasks 5–7.
- `Checking…` chip / countdown / error chip / status line → Tasks 6–7.
- Check-aware feed empty state → Task 8.
- Known limitation documented → Task 9 Step 3.
- Tests: `record_tick` (success + clear error), migration idempotent + preexisting-DB upgrade, `next_check_at` → Tasks 1, 4. Live verification → Task 9.

**Placeholder scan:** none — every code step shows full code; commands have expected output.

**Type consistency:** `record_tick(conn, monitor_id, checked, new, error: Option<&str>, now)` consistent across Tasks 1/3. `TickOutcome { checked, new }` consistent Tasks 2/3. DTO camelCase fields (`lastCheckedAt`, `nextCheckAt`, `lastCheckedCount`, `lastNewCount`, `lastError`) match `serde(rename_all="camelCase")` and `types.ts`. `onTickStarted(id)` / `onTickFinished(payload)` consistent Tasks 5/6. `Sidebar` gains `now`/`checkingIds` in Task 6 (passed) and Task 7 (accepted).
