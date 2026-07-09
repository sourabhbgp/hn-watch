# Lossless Ingestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fixed "newest 30" HN fetch with a per-monitor watermark + paginated delta fetch + chunked, fail-closed `claude` judging, so no story is missed under burst volume and none is analyzed twice.

**Architecture:** Each monitor stores a nullable `watermark` (newest submission time processed). A tick fetches every story since `watermark.unwrap_or(now − 1h)`, paginated; dedupes cross-page repeats; judges the unseen set in 30-item `claude` batches run sequentially; and on a fully clean tick commits in the order insert → mark seen → advance watermark (last, to `max − 5min`). Dedup (`seen` + `UNIQUE`) is untouched.

**Tech Stack:** Rust (Tauri core), `rusqlite`, `reqwest`, `tokio`, `serde`. Tests are Rust `#[cfg(test)]` unit tests on pure functions.

## Global Constraints

- **Scope source of truth:** `docs/REQUIREMENTS.md`; design: `docs/superpowers/specs/2026-07-09-lossless-ingestion-design.md`.
- **Constants (exact values):** `LOOKBACK_SECS = 3600`, `WATERMARK_MARGIN_SECS = 300`, `BATCH_SIZE = 30`, `HITS_PER_PAGE = 100`, `MAX_PAGES = 10`, `CLOCK_SKEW_SECS = 3600`.
- **Reuse existing dedup:** never change `seen` / `UNIQUE(monitor_id, hn_id)` semantics.
- **Pure decision logic is unit-tested** (mirror the existing `parse_verdict` / `find_claude` seam); network + `claude` I/O (`fetch_since`, `run_tick`) is verified live, not unit-tested.
- **Test command:** `cargo test --manifest-path src-tauri/Cargo.toml` — all must pass (27 exist today; this plan adds more).
- **DRY:** use std `slice::chunks(BATCH_SIZE)` for batching — do **not** write a custom chunk helper.
- **Branch:** `feat/lossless-ingestion` (already created; spec already committed).

## File Structure

- `src-tauri/src/hn.rs` — add `HnItem.created_at`; parse `created_at_i`; `numeric_filter` (pure) + `fetch_since` (paginated) replacing `fetch_recent`.
- `src-tauri/src/db.rs` — `Monitor.watermark`; `ensure_column("watermark")`; `list_monitors` selects it; `set_watermark`.
- `src-tauri/src/tick.rs` — constants; `dedupe_by_hn_id` + `advance_watermark` (pure); rewrite `run_tick`.
- `src-tauri/src/commands.rs` — `create_monitor` + test Monitor literal gain `watermark: None` (no behavior change).

---

### Task 1: `hn.rs` — `created_at` on `HnItem` + paginated `fetch_since`

**Files:**
- Modify: `src-tauri/src/hn.rs`
- Modify (literals only): `src-tauri/src/tick.rs` (`item` test helper), `src-tauri/src/agent.rs` (`prompt_contains_prompt_and_ids` test)
- Test: `src-tauri/src/hn.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `HnItem { …, pub created_at: i64 }`; `pub async fn fetch_since(since: i64) -> Result<Vec<HnItem>, String>`; `fn numeric_filter(since: i64) -> String` (pure, private, tested).
- Removes: `pub async fn fetch_recent(limit: usize)` (only caller is `run_tick`, rewritten in Task 4).

- [ ] **Step 1: Extend the `HnItem` struct and Algolia hit, add constants + pure helpers**

In `src-tauri/src/hn.rs`, add `created_at` to `HnItem`:

```rust
#[derive(Debug, Clone)]
pub struct HnItem {
    pub hn_id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub points: i64,
    pub num_comments: i64,
    pub created_at: i64,
}
```

Add `created_at_i` to the deserialized hit:

```rust
#[derive(Deserialize)]
struct AlgoliaHit {
    #[serde(rename = "objectID")]
    object_id: String,
    title: Option<String>,
    url: Option<String>,
    points: Option<i64>,
    num_comments: Option<i64>,
    created_at_i: Option<i64>,
}
```

In `parse_algolia`, set the new field on the built `HnItem` (add this line alongside the others):

```rust
                created_at: h.created_at_i.unwrap_or(0),
```

Add constants + the pure filter helper near the top of the file (below the imports):

```rust
const HITS_PER_PAGE: usize = 100;
const MAX_PAGES: usize = 10;

/// Algolia numericFilters clause: only stories submitted at/after `since`.
fn numeric_filter(since: i64) -> String {
    format!("created_at_i>={since}")
}
```

- [ ] **Step 2: Replace `fetch_recent` with paginated `fetch_since`**

Delete the entire `pub async fn fetch_recent(...)` function and replace it with:

```rust
/// Fetch every `story` submitted at/after `since`, newest-first, paginating until a
/// short page (the last one) or the `MAX_PAGES` safety cap. Cross-page duplicates are
/// possible (new arrivals push items to later pages) and are dropped downstream by
/// `dedupe_by_hn_id` / `seen` / `UNIQUE`; stories are never deleted, so nothing is skipped.
pub async fn fetch_since(since: i64) -> Result<Vec<HnItem>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("hn client build failed: {e}"))?;
    let filter = numeric_filter(since);
    let hpp = HITS_PER_PAGE.to_string();
    let mut all: Vec<HnItem> = Vec::new();
    for page in 0..MAX_PAGES {
        let page_s = page.to_string();
        let body = client
            .get("https://hn.algolia.com/api/v1/search_by_date")
            .query(&[
                ("tags", "story"),
                ("numericFilters", filter.as_str()),
                ("hitsPerPage", hpp.as_str()),
                ("page", page_s.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("hn request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("hn read failed: {e}"))?;
        let items = parse_algolia(&body);
        let got = items.len();
        all.extend(items);
        if got < HITS_PER_PAGE {
            break; // short page → last page for this window
        }
        if page + 1 >= MAX_PAGES {
            eprintln!(
                "[hn-watch] fetch_since hit MAX_PAGES ({MAX_PAGES}) cap; \
                 window may be truncated (since={since})"
            );
        }
    }
    Ok(all)
}
```

- [ ] **Step 3: Update the existing `parse_algolia` test to assert `created_at`, and add a `numeric_filter` test**

Replace the body of the existing `parses_hits_and_derives_domain` test's JSON + assertions so a hit carries `created_at_i` and one omits it, and add a new test. In `src-tauri/src/hn.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn parses_hits_and_derives_domain() {
        let body = r#"{
          "hits": [
            {"objectID":"1","title":"A tool","url":"https://www.example.dev/a","points":10,"num_comments":3,"created_at_i":1700000000},
            {"objectID":"2","title":"Ask HN: something","points":5}
          ]
        }"#;
        let items = parse_algolia(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].hn_id, "1");
        assert_eq!(items[0].domain, "example.dev"); // www. stripped
        assert_eq!(items[0].created_at, 1_700_000_000); // parsed from created_at_i
        assert_eq!(items[1].url, "https://news.ycombinator.com/item?id=2"); // fallback url
        assert_eq!(items[1].created_at, 0); // missing created_at_i defaults to 0
    }

    #[test]
    fn numeric_filter_formats_since() {
        assert_eq!(numeric_filter(1_700_000_000), "created_at_i>=1700000000");
    }
```

- [ ] **Step 4: Fix `HnItem` literals in other modules' tests (build breaks without this)**

In `src-tauri/src/tick.rs`, update the `item` test helper to add the field:

```rust
    fn item(id: &str) -> HnItem {
        HnItem {
            hn_id: id.into(), title: format!("t{id}"), url: "https://x.dev/a".into(),
            domain: "x.dev".into(), points: 10, num_comments: 2, created_at: 1_700_000_000,
        }
    }
```

In `src-tauri/src/agent.rs`, the `prompt_contains_prompt_and_ids` test builds an `HnItem` — add `created_at: 1` to that literal:

```rust
        let items = vec![HnItem {
            hn_id: "42".into(), title: "Rust".into(), url: "u".into(),
            domain: "d".into(), points: 1, num_comments: 1, created_at: 1,
        }];
```

- [ ] **Step 5: Run tests to verify green**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (new `created_at` + `numeric_filter` assertions pass; no references to the removed `fetch_recent` remain — `run_tick` still calls it, so this step will FAIL to compile until Task 4). 

> **Note:** `run_tick` in `tick.rs` still calls `hn::fetch_recent` at this point, so the crate will **not** compile after Task 1 alone. To keep Task 1 independently green, apply the minimal bridge in Step 6.

- [ ] **Step 6: Temporary bridge so the crate compiles after Task 1**

In `src-tauri/src/tick.rs` `run_tick`, change the one fetch line from:

```rust
    let recent = hn::fetch_recent(30).await.map_err(TickError::Hn)?;
```

to (temporary — Task 4 replaces this whole function):

```rust
    let recent = hn::fetch_since(tick::now_secs() - 3600).await.map_err(TickError::Hn)?;
```

Wait — inside `tick.rs` call it as `now_secs() - 3600` (no `tick::` prefix):

```rust
    let recent = hn::fetch_since(now_secs() - 3600).await.map_err(TickError::Hn)?;
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (crate compiles; all tests green).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/hn.rs src-tauri/src/tick.rs src-tauri/src/agent.rs
git commit -m "feat(hn): carry created_at + paginated fetch_since (replaces fetch_recent)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `db.rs` — `watermark` column, `Monitor` field, `set_watermark`

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify (literals only): `src-tauri/src/commands.rs` (`create_monitor` + `status_paused_overrides_error_when_claude_down` test)
- Test: `src-tauri/src/db.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `Monitor { …, pub watermark: Option<i64> }`; `pub fn set_watermark(conn: &Connection, monitor_id: &str, watermark: i64) -> rusqlite::Result<()>`.
- Consumes: nothing from Task 1.

- [ ] **Step 1: Add the field to `Monitor` and select it**

In `src-tauri/src/db.rs`, add to the `Monitor` struct (after `last_error`):

```rust
    pub watermark: Option<i64>,
```

In `list_monitors`, add `watermark` to the SELECT and read it (it becomes column index 9):

```rust
    let mut stmt = conn.prepare(
        "SELECT id, name, prompt, interval_secs, created_at,
                last_checked_at, last_checked_count, last_new_count, last_error, watermark
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
            watermark: r.get(9)?,
        })
    })?;
```

`insert_monitor` is unchanged — it inserts a fixed column list, so `watermark` defaults to `NULL`.

- [ ] **Step 2: Add the migration column and `set_watermark`**

In `migrate`, add alongside the other `ensure_column` calls:

```rust
    ensure_column(conn, "monitors", "watermark", "INTEGER")?;
```

Add the setter (near `record_tick`):

```rust
/// Advance a monitor's ingestion watermark (newest submission time processed).
pub fn set_watermark(conn: &Connection, monitor_id: &str, watermark: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE monitors SET watermark = ?2 WHERE id = ?1",
        rusqlite::params![monitor_id, watermark],
    )?;
    Ok(())
}
```

- [ ] **Step 3: Fix `Monitor` literals + add `watermark` to `sample_monitor`**

In `src-tauri/src/db.rs`, add the field to the `sample_monitor` test helper:

```rust
            last_error: None,
            watermark: None,
```

In `src-tauri/src/commands.rs`, `create_monitor` builds a `Monitor` — add `watermark: None` after `last_error: None`:

```rust
        last_error: None,
        watermark: None,
```

In `src-tauri/src/commands.rs`, the `status_paused_overrides_error_when_claude_down` test builds a `Monitor` — add `watermark: None` after `last_new_count: Some(0), last_error: None,`:

```rust
            last_new_count: Some(0), last_error: None, watermark: None,
```

- [ ] **Step 4: Write the `set_watermark` + migration tests**

Add to `src-tauri/src/db.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn set_watermark_round_trips() {
        let c = mem();
        insert_monitor(&c, &sample_monitor("m1")).unwrap();
        // fresh monitor: NULL watermark
        assert_eq!(list_monitors(&c).unwrap().pop().unwrap().watermark, None);
        set_watermark(&c, "m1", 1_700_000_000).unwrap();
        assert_eq!(list_monitors(&c).unwrap().pop().unwrap().watermark, Some(1_700_000_000));
    }
```

Extend the existing `migrate_upgrades_preexisting_db_without_new_columns` test: after the `migrate(&c).unwrap();` line and the existing assertions, add an assertion that the new column defaulted to `NULL` and round-trips:

```rust
        assert_eq!(m.watermark, None); // new column added as NULL on upgrade
        set_watermark(&c, "m1", 999).unwrap();
        assert_eq!(list_monitors(&c).unwrap().pop().unwrap().watermark, Some(999));
```

- [ ] **Step 5: Run tests to verify green**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (all Monitor literals updated, new tests pass).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db.rs src-tauri/src/commands.rs
git commit -m "feat(db): per-monitor watermark column + set_watermark

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `tick.rs` — constants + pure helpers (`dedupe_by_hn_id`, `advance_watermark`)

**Files:**
- Modify: `src-tauri/src/tick.rs`
- Test: `src-tauri/src/tick.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `HnItem.created_at` (Task 1).
- Produces: `pub const LOOKBACK_SECS: i64`, `pub const WATERMARK_MARGIN_SECS: i64`, `pub const BATCH_SIZE: usize`; `pub fn dedupe_by_hn_id(items: Vec<HnItem>) -> Vec<HnItem>`; `pub fn advance_watermark(current: Option<i64>, items: &[HnItem], margin: i64, now: i64) -> Option<i64>`.

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/src/tick.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn dedupe_keeps_first_occurrence_in_order() {
        let items = vec![item("1"), item("2"), item("1"), item("3")];
        let out = dedupe_by_hn_id(items);
        let ids: Vec<&str> = out.iter().map(|i| i.hn_id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "3"]); // dup "1" dropped, order preserved
    }

    fn item_at(id: &str, created_at: i64) -> HnItem {
        HnItem {
            hn_id: id.into(), title: "t".into(), url: "u".into(),
            domain: "d".into(), points: 1, num_comments: 1, created_at,
        }
    }

    #[test]
    fn advance_watermark_sets_max_minus_margin() {
        let items = vec![item_at("a", 100), item_at("b", 500), item_at("c", 300)];
        // now large so nothing is "far future"; margin 5 -> 500 - 5
        assert_eq!(advance_watermark(None, &items, 5, 10_000), Some(495));
    }

    #[test]
    fn advance_watermark_is_monotonic() {
        let items = vec![item_at("a", 500)];
        // current already ahead of (max - margin) -> unchanged
        assert_eq!(advance_watermark(Some(1000), &items, 5, 10_000), Some(1000));
    }

    #[test]
    fn advance_watermark_ignores_absurd_timestamps() {
        let now = 10_000;
        // 0 and far-future dropped; only 500 counts -> 500 - 5
        let items = vec![item_at("a", 0), item_at("b", now + 999_999), item_at("c", 500)];
        assert_eq!(advance_watermark(None, &items, 5, now), Some(495));
        // all absurd -> None (nothing to anchor to)
        let junk = vec![item_at("a", 0), item_at("b", now + 999_999)];
        assert_eq!(advance_watermark(Some(42), &junk, 5, now), None);
        assert_eq!(advance_watermark(None, &junk, 5, now), None);
    }

    #[test]
    fn advance_watermark_empty_is_none() {
        assert_eq!(advance_watermark(Some(42), &[], 5, 10_000), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — `cannot find function dedupe_by_hn_id` / `advance_watermark` (not yet defined).

- [ ] **Step 3: Implement the constants + helpers**

In `src-tauri/src/tick.rs`, add near the top (after the `use` lines):

```rust
/// First-tick look-back when a monitor has no watermark yet (1 hour).
pub const LOOKBACK_SECS: i64 = 3600;
/// Trailing margin behind the newest story, guarding against Algolia's async indexing (5 min).
pub const WATERMARK_MARGIN_SECS: i64 = 300;
/// Stories per `claude` call.
pub const BATCH_SIZE: usize = 30;
/// Tolerance for clock skew when rejecting far-future timestamps (1 hour).
const CLOCK_SKEW_SECS: i64 = 3600;
```

Add the two pure helpers (below `select_unseen`):

```rust
/// Drop cross-page duplicate ids (pagination can re-see an item), keeping the first
/// occurrence and preserving order.
pub fn dedupe_by_hn_id(items: Vec<HnItem>) -> Vec<HnItem> {
    let mut seen: HashSet<String> = HashSet::new();
    items.into_iter().filter(|i| seen.insert(i.hn_id.clone())).collect()
}

/// Compute the next watermark: `MARGIN` behind the newest valid `created_at`, monotonic
/// (never regresses below `current`). Ignores absurd timestamps (<= 0 or far future) so a
/// malformed hit can't rocket the watermark forward. Returns `None` when there is no valid
/// timestamp to anchor to (caller keeps the current watermark, including `NULL`).
pub fn advance_watermark(
    current: Option<i64>,
    items: &[HnItem],
    margin: i64,
    now: i64,
) -> Option<i64> {
    let candidate = items
        .iter()
        .map(|i| i.created_at)
        .filter(|&t| t > 0 && t <= now + CLOCK_SKEW_SECS)
        .max()?;
    let proposed = candidate - margin;
    Some(current.map_or(proposed, |c| c.max(proposed)))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (all five new tests + existing suite green).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/tick.rs
git commit -m "feat(tick): watermark constants + dedupe_by_hn_id + advance_watermark (pure)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `tick.rs` — rewrite `run_tick` (watermark + chunked fail-closed judge + commit ordering)

**Files:**
- Modify: `src-tauri/src/tick.rs` (`run_tick` body; remove the Task 1 temporary bridge)
- Test: none new (I/O path — verified live). Deliverable = crate compiles, full suite green.

**Interfaces:**
- Consumes: `hn::fetch_since` (Task 1), `Monitor.watermark` + `db::set_watermark` (Task 2), `LOOKBACK_SECS` / `WATERMARK_MARGIN_SECS` / `BATCH_SIZE` / `dedupe_by_hn_id` / `advance_watermark` (Task 3), existing `select_unseen` / `build_feed_rows` / `agent::judge` / `db::{list_seen, insert_feed_item, mark_seen}`.
- Produces: unchanged `run_tick` signature `Result<TickOutcome, TickError>` — scheduler untouched.

- [ ] **Step 1: Replace the whole `run_tick` body**

In `src-tauri/src/tick.rs`, replace the entire `pub async fn run_tick(...)` (including the Task 1 temporary `fetch_since(now_secs() - 3600)` bridge) with:

```rust
/// One tick: fetch every HN story since this monitor's watermark (or the last hour on the
/// first tick), drop already-seen items, judge the rest with `claude` in `BATCH_SIZE` chunks
/// (sequential within the tick, bounded by the shared agent semaphore), persist matches, mark
/// every judged id seen, and advance the watermark — in that order so a crash before the
/// watermark write is safe (the window is re-fetched and `seen` dedupes it). Fail-closed: any
/// batch failure returns Err before any DB write, and the whole window is re-judged next tick.
pub async fn run_tick(
    db: &Arc<Mutex<Connection>>,
    monitor: &Monitor,
) -> Result<TickOutcome, TickError> {
    let now = now_secs();
    let since = monitor.watermark.unwrap_or(now - LOOKBACK_SECS);

    let recent = dedupe_by_hn_id(hn::fetch_since(since).await.map_err(TickError::Hn)?);
    let checked = recent.len();
    // Compute the next watermark from the full fetched window before `select_unseen` consumes it.
    let new_watermark = advance_watermark(monitor.watermark, &recent, WATERMARK_MARGIN_SECS, now);

    let seen = {
        let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
        db::list_seen(&conn, &monitor.id).map_err(|e| TickError::Db(e.to_string()))?
    };
    let unseen = select_unseen(recent, &seen);
    if unseen.is_empty() {
        return Ok(TickOutcome { checked, new: 0, agent_ran: false });
    }

    // Judge in chunks; fail-closed — the first batch error aborts before any DB write.
    let mut verdicts: Vec<Verdict> = Vec::new();
    for batch in unseen.chunks(BATCH_SIZE) {
        let batch_verdicts = agent::judge(&monitor.prompt, batch)
            .await
            .map_err(TickError::Agent)?;
        verdicts.extend(batch_verdicts);
    }
    let rows = build_feed_rows(&monitor.id, &unseen, &verdicts, now);

    // Commit order: insert -> mark seen -> advance watermark (LAST). See doc comment.
    let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
    for row in &rows {
        db::insert_feed_item(&conn, row).map_err(|e| TickError::Db(e.to_string()))?;
    }
    for item in &unseen {
        db::mark_seen(&conn, &monitor.id, &item.hn_id).map_err(|e| TickError::Db(e.to_string()))?;
    }
    if let Some(wm) = new_watermark {
        db::set_watermark(&conn, &monitor.id, wm).map_err(|e| TickError::Db(e.to_string()))?;
    }
    Ok(TickOutcome { checked, new: rows.len(), agent_ran: true })
}
```

- [ ] **Step 2: Run the full suite + build to verify green**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — the whole suite (including Tasks 1–3 tests) is green and the crate compiles with no references to `fetch_recent` and no unused-import warnings.

Then confirm a clean release build compiles:

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds with no errors, no warnings.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/tick.rs
git commit -m "feat(tick): lossless run_tick — watermark fetch + chunked fail-closed judge

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Live verification in the native window

**Files:** none (verification only — per `docs/TESTING.md`, real native window, not localhost).

- [ ] **Step 1: Build and launch the native app**

Run: `npm run tauri build` then launch the bundled app (or `npm run tauri dev`), and drive it with computer-use per `docs/TESTING.md`.

- [ ] **Step 2: Normal path**

Create a monitor. Confirm: the first tick looks back ~1 hour (feed populates or "checked N, nothing matched yet"); the monitor tile's `checked` count is sane. Restart the app; confirm monitors + feed persist and the next tick does **not** re-emit duplicate feed cards (watermark carried forward, `seen` dedup holds).

- [ ] **Step 3: Burst path (the headline case)**

Temporarily force a large window by seeding an old watermark for a monitor (e.g. via the sqlite file: `UPDATE monitors SET watermark = <now − 24h>`), or add a throwaway `eprintln!` of page count in `fetch_since`. Trigger a tick and confirm from logs/behavior: multiple Algolia pages fetched, judged in batches of 30, `checked` on the tile reflects the full window (e.g. > 30), and **no duplicate feed cards**. Remove any throwaway logging afterward.

- [ ] **Step 4: Fail-closed path**

Point `HN_WATCH_CLAUDE_BIN` at a fake script that exits non-zero (reuse the Session 5 technique). Trigger a tick during a non-empty window; confirm nothing is committed that tick (no new feed cards, watermark unchanged in the DB), the monitor shows an error, and after restoring a healthy `claude` the next tick judges the window cleanly with no duplicates.

- [ ] **Step 5: Push the branch (keep it on origin per CLAUDE.md)**

```bash
git push -u origin feat/lossless-ingestion
```

---

## Notes for the implementer

- **Do not** touch `seen` / `UNIQUE` dedup or the scheduler — `run_tick`'s signature is unchanged on purpose.
- **`select_unseen` consumes `recent`** (takes `Vec`), so `advance_watermark(&recent, …)` and `checked = recent.len()` must run **before** the `select_unseen` call — the rewritten body already orders them correctly; keep that order.
- The Task 1 temporary bridge (`fetch_since(now_secs() - 3600)`) exists only so Task 1 compiles standalone; Task 4 removes it. If executing Tasks 1–4 in one pass, you may skip the bridge and go straight to the Task 4 body — but then Task 1 alone won't compile, so commit Tasks 1–4 together in that case.
- Batching uses std `slice::chunks(BATCH_SIZE)` — no custom helper.
