# Monitors + Real Tick Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the static mock UI with a working core loop — create/list/delete monitors persisted in SQLite, each running as a long-lived Rust background worker that ticks recent Hacker News through `claude -p` and appends deduplicated matches to the feed.

**Architecture:** A Rust core split into focused modules (`db`, `hn`, `agent`, `tick`, `scheduler`, `commands`) behind Tauri commands + events. One SQLite connection lives in Tauri state; one async worker per monitor ticks immediately then on its interval. `claude -p` is called once per tick, bounded by a shared semaphore. The React UI calls the commands and listens for a `feed-updated` event.

**Tech Stack:** Tauri 2, Rust (rusqlite bundled, reqwest, tokio process, uuid, serde), React 19 + TypeScript, `claude -p` as the agent runtime.

## Global Constraints

- Reuse existing design tokens in `src/index.css` / `docs/design.md`; never hardcode colors, fonts, spacing.
- Reuse existing UI components (`Feed`, `FeedCard`, `Sidebar`, `DigDeeperPanel`); no visual redesign.
- UI type contract is `src/types.ts` — Rust DTOs must serialize to exactly those shapes (camelCase).
- `claude` binary is on PATH; the agent runtime is `claude -p` (one call per tick this slice).
- HN source is the Algolia HN Search API (`https://hn.algolia.com/api/v1/search_by_date?tags=story`).
- Dedup is per `(monitor_id, hn_id)` via the `seen` table; all judged ids are recorded, matched or not.
- A failed tick (HN error, `claude` error, bad JSON) is logged and skipped; the worker must survive.
- Out of scope this slice: system tray, native notifications, the dig-deeper swarm, monitor edit/pause/status management, a manual "Run now" button. Do not build them.
- Branch: `feat/monitors-and-tick`. Commit after every task.

---

### Task 1: Rust dependencies + SQLite store (`db.rs`)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lib.rs` (declare `mod db;`)

**Interfaces:**
- Produces:
  - `pub struct Monitor { pub id: String, pub name: String, pub prompt: String, pub interval_secs: i64, pub created_at: i64 }`
  - `pub struct FeedRow { pub id: String, pub monitor_id: String, pub hn_id: String, pub title: String, pub url: String, pub domain: String, pub summary: String, pub reason: String, pub hn_score: i64, pub hn_comments: i64, pub created_at: i64 }`
  - `pub fn migrate(conn: &Connection) -> rusqlite::Result<()>`
  - `pub fn insert_monitor(conn: &Connection, m: &Monitor) -> rusqlite::Result<()>`
  - `pub fn list_monitors(conn: &Connection) -> rusqlite::Result<Vec<Monitor>>`
  - `pub fn delete_monitor(conn: &Connection, id: &str) -> rusqlite::Result<()>`
  - `pub fn insert_feed_item(conn: &Connection, f: &FeedRow) -> rusqlite::Result<()>`
  - `pub fn list_feed(conn: &Connection) -> rusqlite::Result<Vec<(FeedRow, String)>>` (row + monitor name, newest first)
  - `pub fn count_matches(conn: &Connection, monitor_id: &str) -> rusqlite::Result<i64>`
  - `pub fn list_seen(conn: &Connection, monitor_id: &str) -> rusqlite::Result<HashSet<String>>`
  - `pub fn mark_seen(conn: &Connection, monitor_id: &str, hn_id: &str) -> rusqlite::Result<()>`

- [ ] **Step 1: Add dependencies to `src-tauri/Cargo.toml`**

Replace the `[dependencies]` block with:

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-opener = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
tokio = { version = "1", features = ["process", "time", "sync", "rt"] }
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 2: Write `src-tauri/src/db.rs` with the schema, types, helpers, and a failing test**

```rust
use rusqlite::Connection;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub interval_secs: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct FeedRow {
    pub id: String,
    pub monitor_id: String,
    pub hn_id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub summary: String,
    pub reason: String,
    pub hn_score: i64,
    pub hn_comments: i64,
    pub created_at: i64,
}

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
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS seen (
             monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
             hn_id TEXT NOT NULL,
             PRIMARY KEY (monitor_id, hn_id)
         );",
    )
}

pub fn insert_monitor(conn: &Connection, m: &Monitor) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO monitors (id, name, prompt, interval_secs, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![m.id, m.name, m.prompt, m.interval_secs, m.created_at],
    )?;
    Ok(())
}

pub fn list_monitors(conn: &Connection) -> rusqlite::Result<Vec<Monitor>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, prompt, interval_secs, created_at FROM monitors ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Monitor {
            id: r.get(0)?,
            name: r.get(1)?,
            prompt: r.get(2)?,
            interval_secs: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn delete_monitor(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    conn.execute("DELETE FROM monitors WHERE id = ?1", [id])?;
    Ok(())
}

pub fn insert_feed_item(conn: &Connection, f: &FeedRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO feed_items
         (id, monitor_id, hn_id, title, url, domain, summary, reason, hn_score, hn_comments, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        rusqlite::params![
            f.id, f.monitor_id, f.hn_id, f.title, f.url, f.domain,
            f.summary, f.reason, f.hn_score, f.hn_comments, f.created_at
        ],
    )?;
    Ok(())
}

pub fn list_feed(conn: &Connection) -> rusqlite::Result<Vec<(FeedRow, String)>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.monitor_id, f.hn_id, f.title, f.url, f.domain, f.summary, f.reason,
                f.hn_score, f.hn_comments, f.created_at, m.name
         FROM feed_items f JOIN monitors m ON m.id = f.monitor_id
         ORDER BY f.created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            FeedRow {
                id: r.get(0)?,
                monitor_id: r.get(1)?,
                hn_id: r.get(2)?,
                title: r.get(3)?,
                url: r.get(4)?,
                domain: r.get(5)?,
                summary: r.get(6)?,
                reason: r.get(7)?,
                hn_score: r.get(8)?,
                hn_comments: r.get(9)?,
                created_at: r.get(10)?,
            },
            r.get(11)?,
        ))
    })?;
    rows.collect()
}

pub fn count_matches(conn: &Connection, monitor_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM feed_items WHERE monitor_id = ?1",
        [monitor_id],
        |r| r.get(0),
    )
}

pub fn list_seen(conn: &Connection, monitor_id: &str) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT hn_id FROM seen WHERE monitor_id = ?1")?;
    let rows = stmt.query_map([monitor_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

pub fn mark_seen(conn: &Connection, monitor_id: &str, hn_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO seen (monitor_id, hn_id) VALUES (?1, ?2)",
        [monitor_id, hn_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    fn sample_monitor(id: &str) -> Monitor {
        Monitor {
            id: id.into(),
            name: "AI".into(),
            prompt: "ai agents".into(),
            interval_secs: 1800,
            created_at: 100,
        }
    }

    #[test]
    fn insert_list_delete_monitor() {
        let c = mem();
        insert_monitor(&c, &sample_monitor("m1")).unwrap();
        let all = list_monitors(&c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "m1");
        delete_monitor(&c, "m1").unwrap();
        assert_eq!(list_monitors(&c).unwrap().len(), 0);
    }

    #[test]
    fn seen_dedup_and_feed_count() {
        let c = mem();
        insert_monitor(&c, &sample_monitor("m1")).unwrap();
        mark_seen(&c, "m1", "hn1").unwrap();
        mark_seen(&c, "m1", "hn1").unwrap(); // idempotent
        let seen = list_seen(&c, "m1").unwrap();
        assert!(seen.contains("hn1"));
        assert_eq!(seen.len(), 1);

        insert_feed_item(&c, &FeedRow {
            id: "f1".into(), monitor_id: "m1".into(), hn_id: "hn1".into(),
            title: "t".into(), url: "https://x.dev/a".into(), domain: "x.dev".into(),
            summary: "s".into(), reason: "r".into(), hn_score: 10, hn_comments: 2, created_at: 200,
        }).unwrap();
        assert_eq!(count_matches(&c, "m1").unwrap(), 1);
        let feed = list_feed(&c).unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].1, "AI"); // joined monitor name
    }

    #[test]
    fn delete_monitor_cascades_feed_and_seen() {
        let c = mem();
        c.execute("PRAGMA foreign_keys = ON", []).unwrap();
        insert_monitor(&c, &sample_monitor("m1")).unwrap();
        mark_seen(&c, "m1", "hn1").unwrap();
        insert_feed_item(&c, &FeedRow {
            id: "f1".into(), monitor_id: "m1".into(), hn_id: "hn1".into(),
            title: "t".into(), url: "u".into(), domain: "d".into(),
            summary: "s".into(), reason: "r".into(), hn_score: 1, hn_comments: 1, created_at: 1,
        }).unwrap();
        delete_monitor(&c, "m1").unwrap();
        assert_eq!(count_matches(&c, "m1").unwrap(), 0);
        assert_eq!(list_seen(&c, "m1").unwrap().len(), 0);
    }
}
```

Add `mod db;` near the top of `src-tauri/src/lib.rs` (above `fn run`).

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test db::`
Expected: 3 tests pass (`insert_list_delete_monitor`, `seen_dedup_and_feed_count`, `delete_monitor_cascades_feed_and_seen`).

Note: the cascade test requires `PRAGMA foreign_keys = ON` on the connection (set in the test and in `delete_monitor`).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/db.rs src-tauri/src/lib.rs
git commit -m "feat: SQLite store for monitors, feed, and seen dedup"
```

---

### Task 2: Hacker News source (`hn.rs`)

**Files:**
- Create: `src-tauri/src/hn.rs`
- Modify: `src-tauri/src/lib.rs` (declare `mod hn;`)

**Interfaces:**
- Produces:
  - `pub struct HnItem { pub hn_id: String, pub title: String, pub url: String, pub domain: String, pub points: i64, pub num_comments: i64 }`
  - `pub fn parse_algolia(body: &str) -> Vec<HnItem>` (pure; skips items with no title)
  - `pub async fn fetch_recent(limit: usize) -> Result<Vec<HnItem>, String>`

- [ ] **Step 1: Write `src-tauri/src/hn.rs` with a failing parse test**

```rust
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct HnItem {
    pub hn_id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub points: i64,
    pub num_comments: i64,
}

#[derive(Deserialize)]
struct AlgoliaResponse {
    hits: Vec<AlgoliaHit>,
}

#[derive(Deserialize)]
struct AlgoliaHit {
    #[serde(rename = "objectID")]
    object_id: String,
    title: Option<String>,
    url: Option<String>,
    points: Option<i64>,
    num_comments: Option<i64>,
}

fn domain_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string()
}

pub fn parse_algolia(body: &str) -> Vec<HnItem> {
    let resp: AlgoliaResponse = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    resp.hits
        .into_iter()
        .filter_map(|h| {
            let title = h.title?;
            // Self/Ask/Show posts without an external url point at the HN item.
            let url = h
                .url
                .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", h.object_id));
            let domain = domain_of(&url);
            Some(HnItem {
                hn_id: h.object_id,
                title,
                url,
                domain,
                points: h.points.unwrap_or(0),
                num_comments: h.num_comments.unwrap_or(0),
            })
        })
        .collect()
}

pub async fn fetch_recent(limit: usize) -> Result<Vec<HnItem>, String> {
    let url = format!(
        "https://hn.algolia.com/api/v1/search_by_date?tags=story&hitsPerPage={}",
        limit
    );
    let body = reqwest::get(&url)
        .await
        .map_err(|e| format!("hn request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("hn read failed: {e}"))?;
    Ok(parse_algolia(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits_and_derives_domain() {
        let body = r#"{
          "hits": [
            {"objectID":"1","title":"A tool","url":"https://www.example.dev/a","points":10,"num_comments":3},
            {"objectID":"2","title":"Ask HN: something","points":5},
            {"objectID":"3","points":1}
          ]
        }"#;
        let items = parse_algolia(body);
        assert_eq!(items.len(), 2); // item 3 dropped: no title
        assert_eq!(items[0].hn_id, "1");
        assert_eq!(items[0].domain, "example.dev"); // www. stripped
        assert_eq!(items[1].url, "https://news.ycombinator.com/item?id=2"); // fallback url
        assert_eq!(items[1].domain, "news.ycombinator.com");
    }

    #[test]
    fn bad_json_yields_empty() {
        assert!(parse_algolia("not json").is_empty());
    }
}
```

Add `mod hn;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the parse tests**

Run: `cd src-tauri && cargo test hn::`
Expected: `parses_hits_and_derives_domain` and `bad_json_yields_empty` pass.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/hn.rs src-tauri/src/lib.rs
git commit -m "feat: fetch recent HN stories via Algolia search API"
```

---

### Task 3: `claude -p` agent runtime (`agent.rs`)

**Files:**
- Create: `src-tauri/src/agent.rs`
- Modify: `src-tauri/src/lib.rs` (declare `mod agent;`)

**Interfaces:**
- Consumes: `crate::hn::HnItem`
- Produces:
  - `pub struct Verdict { pub hn_id: String, pub summary: String, pub reason: String }`
  - `pub fn build_prompt(user_prompt: &str, items: &[HnItem]) -> String`
  - `pub fn parse_verdict(text: &str) -> Vec<Verdict>` (extracts the JSON array; empty on failure)
  - `pub async fn judge(user_prompt: &str, items: &[HnItem]) -> Result<Vec<Verdict>, String>` (semaphore-bounded `claude -p` call)

- [ ] **Step 1: Write `src-tauri/src/agent.rs` with a failing `parse_verdict` test**

```rust
use crate::hn::HnItem;
use serde::Deserialize;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// Shared agent runtime bound: monitor ticks (one call each) and the future
/// dig-deeper swarm (many at once) both acquire from this single semaphore.
fn agent_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(4))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Verdict {
    pub hn_id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub reason: String,
}

pub fn build_prompt(user_prompt: &str, items: &[HnItem]) -> String {
    let list: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "hn_id": i.hn_id,
                "title": i.title,
                "url": i.url,
                "points": i.points,
            })
        })
        .collect();
    let items_json = serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into());
    format!(
        "You are a filter for a Hacker News watcher. The user cares about:\n\
         \"{user_prompt}\"\n\n\
         Here are recent HN stories as a JSON array:\n{items_json}\n\n\
         Return ONLY a JSON array (no prose, no markdown fences) of the stories that genuinely \
         match the user's interest. Each element must be an object with exactly these keys: \
         \"hn_id\" (string, copied from the input), \"summary\" (one or two sentences on what \
         the story is), and \"reason\" (one sentence on why it matches the interest). \
         If nothing matches, return []."
    )
}

/// Pull the first JSON array out of the model's response and parse it.
pub fn parse_verdict(text: &str) -> Vec<Verdict> {
    let start = match text.find('[') {
        Some(s) => s,
        None => return Vec::new(),
    };
    let end = match text.rfind(']') {
        Some(e) if e > start => e,
        _ => return Vec::new(),
    };
    serde_json::from_str::<Vec<Verdict>>(&text[start..=end]).unwrap_or_default()
}

pub async fn judge(user_prompt: &str, items: &[HnItem]) -> Result<Vec<Verdict>, String> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let prompt = build_prompt(user_prompt, items);
    let _permit = agent_sem()
        .acquire()
        .await
        .map_err(|e| format!("semaphore closed: {e}"))?;
    let output = tokio::process::Command::new("claude")
        .arg("-p")
        .arg(&prompt)
        .output()
        .await
        .map_err(|e| format!("failed to spawn claude: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "claude exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_verdict(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_array_amid_prose() {
        let text = "Sure! Here are the matches:\n\
            [{\"hn_id\":\"1\",\"summary\":\"A tool\",\"reason\":\"matches\"}]\nHope that helps.";
        let v = parse_verdict(text);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].hn_id, "1");
        assert_eq!(v[0].summary, "A tool");
    }

    #[test]
    fn empty_array_and_garbage() {
        assert_eq!(parse_verdict("[]").len(), 0);
        assert_eq!(parse_verdict("no json here").len(), 0);
        assert_eq!(parse_verdict("[broken").len(), 0);
    }

    #[test]
    fn prompt_contains_prompt_and_ids() {
        let items = vec![HnItem {
            hn_id: "42".into(), title: "Rust".into(), url: "u".into(),
            domain: "d".into(), points: 1, num_comments: 1,
        }];
        let p = build_prompt("rust async", &items);
        assert!(p.contains("rust async"));
        assert!(p.contains("\"42\""));
    }
}
```

Add `mod agent;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cd src-tauri && cargo test agent::`
Expected: `extracts_array_amid_prose`, `empty_array_and_garbage`, `prompt_contains_prompt_and_ids` pass. (`judge` is not unit-tested — it shells out to `claude`; it is exercised in Task 8.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/agent.rs src-tauri/src/lib.rs
git commit -m "feat: claude -p agent runtime with shared semaphore and JSON verdict parsing"
```

---

### Task 4: Tick logic (`tick.rs`)

**Files:**
- Create: `src-tauri/src/tick.rs`
- Modify: `src-tauri/src/lib.rs` (declare `mod tick;`)

**Interfaces:**
- Consumes: `crate::hn::HnItem`, `crate::agent::Verdict`, `crate::db::{Monitor, FeedRow}`, `crate::db` helpers
- Produces:
  - `pub fn select_unseen(items: Vec<HnItem>, seen: &HashSet<String>) -> Vec<HnItem>`
  - `pub fn build_feed_rows(monitor_id: &str, items: &[HnItem], verdicts: &[Verdict], now: i64) -> Vec<FeedRow>`
  - `pub async fn run_tick(db: &Arc<Mutex<Connection>>, monitor: &Monitor) -> Result<usize, String>` (returns number of new matches inserted)

- [ ] **Step 1: Write `src-tauri/src/tick.rs` with failing tests for the pure helpers**

```rust
use crate::agent::{self, Verdict};
use crate::db::{self, FeedRow, Monitor};
use crate::hn::{self, HnItem};
use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn select_unseen(items: Vec<HnItem>, seen: &HashSet<String>) -> Vec<HnItem> {
    items.into_iter().filter(|i| !seen.contains(&i.hn_id)).collect()
}

pub fn build_feed_rows(
    monitor_id: &str,
    items: &[HnItem],
    verdicts: &[Verdict],
    now: i64,
) -> Vec<FeedRow> {
    verdicts
        .iter()
        .filter_map(|v| {
            let item = items.iter().find(|i| i.hn_id == v.hn_id)?;
            Some(FeedRow {
                id: Uuid::new_v4().to_string(),
                monitor_id: monitor_id.to_string(),
                hn_id: item.hn_id.clone(),
                title: item.title.clone(),
                url: item.url.clone(),
                domain: item.domain.clone(),
                summary: v.summary.clone(),
                reason: v.reason.clone(),
                hn_score: item.points,
                hn_comments: item.num_comments,
                created_at: now,
            })
        })
        .collect()
}

/// One tick: fetch recent HN, drop already-seen items, judge the rest with
/// claude, persist matches, and record every judged id as seen. Returns the
/// number of new matches inserted. Errors are propagated so the worker can log
/// them; the worker keeps running regardless.
pub async fn run_tick(db: &Arc<Mutex<Connection>>, monitor: &Monitor) -> Result<usize, String> {
    let recent = hn::fetch_recent(30).await?;

    let seen = {
        let conn = db.lock().map_err(|_| "db poisoned".to_string())?;
        db::list_seen(&conn, &monitor.id).map_err(|e| e.to_string())?
    };
    let unseen = select_unseen(recent, &seen);
    if unseen.is_empty() {
        return Ok(0);
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
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> HnItem {
        HnItem {
            hn_id: id.into(), title: format!("t{id}"), url: "https://x.dev/a".into(),
            domain: "x.dev".into(), points: 10, num_comments: 2,
        }
    }

    #[test]
    fn select_unseen_filters_seen_ids() {
        let items = vec![item("1"), item("2"), item("3")];
        let seen: HashSet<String> = ["2".to_string()].into_iter().collect();
        let out = select_unseen(items, &seen);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|i| i.hn_id != "2"));
    }

    #[test]
    fn build_feed_rows_only_for_matched_ids() {
        let items = vec![item("1"), item("2")];
        let verdicts = vec![Verdict { hn_id: "2".into(), summary: "s".into(), reason: "r".into() }];
        let rows = build_feed_rows("m1", &items, &verdicts, 123);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hn_id, "2");
        assert_eq!(rows[0].hn_score, 10); // carried from the HN item
        assert_eq!(rows[0].summary, "s"); // carried from the verdict
        assert_eq!(rows[0].created_at, 123);
    }

    #[test]
    fn build_feed_rows_ignores_verdict_for_unknown_id() {
        let items = vec![item("1")];
        let verdicts = vec![Verdict { hn_id: "999".into(), summary: "s".into(), reason: "r".into() }];
        assert_eq!(build_feed_rows("m1", &items, &verdicts, 1).len(), 0);
    }
}
```

Add `mod tick;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cd src-tauri && cargo test tick::`
Expected: `select_unseen_filters_seen_ids`, `build_feed_rows_only_for_matched_ids`, `build_feed_rows_ignores_verdict_for_unknown_id` pass.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/tick.rs src-tauri/src/lib.rs
git commit -m "feat: per-tick logic — unseen filter, feed-row build, run_tick"
```

---

### Task 5: Scheduler (`scheduler.rs`)

**Files:**
- Create: `src-tauri/src/scheduler.rs`
- Modify: `src-tauri/src/lib.rs` (declare `mod scheduler;`)

**Interfaces:**
- Consumes: `crate::db::Monitor`, `crate::tick::run_tick`, `tauri::AppHandle`
- Produces:
  - `pub struct Scheduler { handles: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>> }`
  - `impl Scheduler`: `pub fn new() -> Self`, `pub fn spawn(&self, app: AppHandle, db: Arc<Mutex<Connection>>, monitor: Monitor)`, `pub fn stop(&self, id: &str)`
- Emits the Tauri event `"feed-updated"` (no payload) after any tick that inserts ≥1 new match.

- [ ] **Step 1: Write `src-tauri/src/scheduler.rs`**

This module drives `claude` and timers; it is verified in the native window (Task 8), not by a unit test.

```rust
use crate::db::Monitor;
use crate::tick;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub struct Scheduler {
    handles: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler { handles: Mutex::new(HashMap::new()) }
    }

    /// Spawn a long-lived worker: tick immediately, then every `interval_secs`.
    pub fn spawn(&self, app: AppHandle, db: Arc<Mutex<Connection>>, monitor: Monitor) {
        let interval = Duration::from_secs(monitor.interval_secs.max(1) as u64);
        let id = monitor.id.clone();
        let handle = tauri::async_runtime::spawn(async move {
            loop {
                match tick::run_tick(&db, &monitor).await {
                    Ok(n) if n > 0 => {
                        if let Err(e) = app.emit("feed-updated", ()) {
                            eprintln!("[hn-watch] emit failed for {}: {e}", monitor.id);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("[hn-watch] tick failed for {}: {e}", monitor.id),
                }
                tokio::time::sleep(interval).await;
            }
        });
        self.handles.lock().unwrap().insert(id, handle);
    }

    /// Cancel and drop a monitor's worker.
    pub fn stop(&self, id: &str) {
        if let Some(handle) = self.handles.lock().unwrap().remove(id) {
            handle.abort();
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
```

Add `mod scheduler;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: builds without errors (warnings about unused code are fine until Task 6 wires it in).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/scheduler.rs src-tauri/src/lib.rs
git commit -m "feat: per-monitor scheduler — immediate + interval ticks, feed-updated event"
```

---

### Task 6: Tauri commands + state wiring (`commands.rs`, `lib.rs`)

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (state, setup, handlers)

**Interfaces:**
- Consumes: everything above.
- Produces (invokable from JS):
  - `create_monitor(name: String, prompt: String, intervalSecs: i64) -> MonitorDto`
  - `list_monitors() -> Vec<MonitorDto>`
  - `delete_monitor(id: String) -> ()`
  - `list_feed() -> Vec<FeedDto>`
  - DTO JSON shapes (camelCase) exactly matching `src/types.ts`:
    - `MonitorDto { id, name, prompt, intervalLabel, status, matchCount }`
    - `FeedDto { id, monitorId, monitorName, title, url, domain, summary, reason, hnScore, hnComments, timeAgo }`

- [ ] **Step 1: Write `src-tauri/src/commands.rs`**

```rust
use crate::db::{self, Monitor};
use crate::scheduler::Scheduler;
use crate::tick::now_secs;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub scheduler: Scheduler,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDto {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub interval_label: String,
    pub status: String,
    pub match_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedDto {
    pub id: String,
    pub monitor_id: String,
    pub monitor_name: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub summary: String,
    pub reason: String,
    pub hn_score: i64,
    pub hn_comments: i64,
    pub time_ago: String,
}

fn interval_label(secs: i64) -> String {
    if secs % 3600 == 0 {
        format!("every {}h", secs / 3600)
    } else {
        format!("every {}m", (secs / 60).max(1))
    }
}

fn time_ago(created: i64, now: i64) -> String {
    let d = (now - created).max(0);
    if d < 3600 {
        format!("{}m", (d / 60).max(1))
    } else if d < 86_400 {
        format!("{}h", d / 3600)
    } else {
        format!("{}d", d / 86_400)
    }
}

fn to_monitor_dto(conn: &Connection, m: &Monitor) -> rusqlite::Result<MonitorDto> {
    Ok(MonitorDto {
        id: m.id.clone(),
        name: m.name.clone(),
        prompt: m.prompt.clone(),
        interval_label: interval_label(m.interval_secs),
        status: "active".into(),
        match_count: db::count_matches(conn, &m.id)?,
    })
}

#[tauri::command]
pub fn list_monitors(state: State<'_, AppState>) -> Result<Vec<MonitorDto>, String> {
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    let monitors = db::list_monitors(&conn).map_err(|e| e.to_string())?;
    monitors
        .iter()
        .map(|m| to_monitor_dto(&conn, m).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
pub fn list_feed(state: State<'_, AppState>) -> Result<Vec<FeedDto>, String> {
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    let rows = db::list_feed(&conn).map_err(|e| e.to_string())?;
    let now = now_secs();
    Ok(rows
        .into_iter()
        .map(|(f, monitor_name)| FeedDto {
            id: f.id,
            monitor_id: f.monitor_id,
            monitor_name,
            title: f.title,
            url: f.url,
            domain: f.domain,
            summary: f.summary,
            reason: f.reason,
            hn_score: f.hn_score,
            hn_comments: f.hn_comments,
            time_ago: time_ago(f.created_at, now),
        })
        .collect())
}

#[tauri::command]
pub fn create_monitor(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    prompt: String,
    interval_secs: i64,
) -> Result<MonitorDto, String> {
    let monitor = Monitor {
        id: Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        prompt: prompt.trim().to_string(),
        interval_secs: interval_secs.max(60),
        created_at: now_secs(),
    };
    if monitor.name.is_empty() || monitor.prompt.is_empty() {
        return Err("name and prompt are required".into());
    }
    let dto = {
        let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
        db::insert_monitor(&conn, &monitor).map_err(|e| e.to_string())?;
        to_monitor_dto(&conn, &monitor).map_err(|e| e.to_string())?
    };
    state
        .scheduler
        .spawn(app, Arc::clone(&state.db), monitor);
    Ok(dto)
}

#[tauri::command]
pub fn delete_monitor(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.scheduler.stop(&id);
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    db::delete_monitor(&conn, &id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Called once at startup: open/create the DB and spawn a worker per monitor.
pub fn init_state(app: &AppHandle) -> AppState {
    let dir = app
        .path()
        .app_data_dir()
        .expect("no app data dir");
    std::fs::create_dir_all(&dir).ok();
    let conn = Connection::open(dir.join("hn-watch.sqlite")).expect("open db");
    db::migrate(&conn).expect("migrate db");
    let db = Arc::new(Mutex::new(conn));
    let scheduler = Scheduler::new();

    let existing = {
        let conn = db.lock().unwrap();
        db::list_monitors(&conn).unwrap_or_default()
    };
    for m in existing {
        scheduler.spawn(app.clone(), Arc::clone(&db), m);
    }
    AppState { db, scheduler }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_label_formats() {
        assert_eq!(interval_label(1800), "every 30m");
        assert_eq!(interval_label(3600), "every 1h");
        assert_eq!(interval_label(7200), "every 2h");
        assert_eq!(interval_label(900), "every 15m");
    }

    #[test]
    fn time_ago_buckets() {
        assert_eq!(time_ago(0, 120), "2m");
        assert_eq!(time_ago(0, 7200), "2h");
        assert_eq!(time_ago(0, 172_800), "2d");
        assert_eq!(time_ago(100, 100), "1m"); // floor to at least 1m
    }
}
```

- [ ] **Step 2: Rewrite `src-tauri/src/lib.rs` to wire state, setup, and handlers**

```rust
mod agent;
mod commands;
mod db;
mod hn;
mod scheduler;
mod tick;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = commands::init_state(&app.handle());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_monitor,
            commands::list_monitors,
            commands::delete_monitor,
            commands::list_feed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

(The old `greet` command is removed.)

- [ ] **Step 3: Run the command-helper tests and build**

Run: `cd src-tauri && cargo test commands:: && cargo build`
Expected: `interval_label_formats` and `time_ago_buckets` pass; the crate builds.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: Tauri commands + startup state wiring for monitors and feed"
```

---

### Task 7: Frontend wiring (`api.ts`, `App.tsx`, `Sidebar.tsx`)

**Files:**
- Create: `src/api.ts`
- Modify: `src/App.tsx`
- Modify: `src/components/Sidebar.tsx`
- (Unchanged: `src/types.ts`, `src/components/Feed.tsx`, `FeedCard.tsx`, `DigDeeperPanel.tsx`, `src/mock/data.ts` — the last keeps `BRIEF_F1` for the still-mock dig-deeper panel.)

**Interfaces:**
- Consumes Tauri commands from Task 6 and the `feed-updated` event.
- Produces: `listMonitors()`, `listFeed()`, `createMonitor(name, prompt, intervalSecs)`, `deleteMonitor(id)`, `onFeedUpdated(cb)`.

- [ ] **Step 1: Create `src/api.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Monitor, FeedItem } from "./types";

export const listMonitors = () => invoke<Monitor[]>("list_monitors");
export const listFeed = () => invoke<FeedItem[]>("list_feed");

export const createMonitor = (name: string, prompt: string, intervalSecs: number) =>
  invoke<Monitor>("create_monitor", { name, prompt, intervalSecs });

export const deleteMonitor = (id: string) => invoke<void>("delete_monitor", { id });

// Fires whenever a tick inserts new matches. Returns an unlisten function.
export const onFeedUpdated = (cb: () => void) => listen("feed-updated", cb);
```

- [ ] **Step 2: Replace the interval options + create form in `src/components/Sidebar.tsx`**

Add a small inline create form and a delete affordance. Replace the whole file with:

```tsx
import { useState } from "react";
import type { Monitor, MonitorStatus } from "../types";

const STATUS_DOT: Record<MonitorStatus, string> = {
  active: "bg-ok",
  paused: "bg-faint",
  error: "bg-rust",
};

const INTERVAL_OPTIONS: { label: string; secs: number }[] = [
  { label: "every 15m", secs: 900 },
  { label: "every 30m", secs: 1800 },
  { label: "every 1h", secs: 3600 },
  { label: "every 6h", secs: 21600 },
];

function MonitorRow({
  monitor,
  selected,
  onSelect,
  onDelete,
}: {
  monitor: Monitor;
  selected: boolean;
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
        <p className="mt-1 pl-4 font-mono text-[10.5px] text-faint">{monitor.intervalLabel}</p>
      </button>
    </div>
  );
}

export function Sidebar({
  monitors,
  selectedId,
  onSelect,
  onCreate,
  onDelete,
}: {
  monitors: Monitor[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onCreate: (name: string, prompt: string, intervalSecs: number) => void;
  onDelete: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [secs, setSecs] = useState(1800);

  const submit = () => {
    if (!name.trim() || !prompt.trim()) return;
    onCreate(name.trim(), prompt.trim(), secs);
    setName("");
    setPrompt("");
    setSecs(1800);
    setOpen(false);
  };

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-line bg-card/40">
      <div className="flex items-center gap-2.5 px-4 py-4">
        <div className="h-8 w-8 shrink-0 rounded-lg bg-hn grid place-items-center">
          <svg viewBox="216 216 592 592" className="h-6 w-6" aria-hidden="true">
            <path
              d="M300 356 L376 668 L512 470 L648 668 L724 356"
              fill="none"
              stroke="#ffffff"
              strokeWidth={88}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>
        <div>
          <div className="text-[15px] font-bold leading-none tracking-tight">HN Watch</div>
          <div className="mt-1 font-mono text-[10px] text-faint">watching Hacker News</div>
        </div>
      </div>

      <div className="flex items-center justify-between px-4 pb-2 pt-1">
        <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">Monitors</span>
        <span className="font-mono text-[10px] text-faint">{monitors.length}</span>
      </div>

      <div className="flex-1 space-y-1 overflow-y-auto px-2">
        <button
          onClick={() => onSelect(null)}
          className={`w-full rounded-lg px-3 py-2 text-left text-[13px] font-semibold transition-colors border ${
            selectedId === null
              ? "bg-hn-soft border-hn-border text-ink"
              : "border-transparent text-soft hover:bg-card"
          }`}
        >
          All matches
        </button>
        {monitors.map((m) => (
          <MonitorRow
            key={m.id}
            monitor={m}
            selected={selectedId === m.id}
            onSelect={() => onSelect(m.id)}
            onDelete={() => onDelete(m.id)}
          />
        ))}
      </div>

      <div className="border-t border-line p-3">
        {open ? (
          <div className="space-y-2">
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Monitor name"
              className="w-full rounded-md border border-line bg-paper px-2 py-1.5 text-[12.5px] text-ink placeholder:text-faint"
            />
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="What do you care about? (natural language)"
              rows={3}
              className="w-full resize-none rounded-md border border-line bg-paper px-2 py-1.5 text-[12.5px] text-ink placeholder:text-faint"
            />
            <select
              value={secs}
              onChange={(e) => setSecs(Number(e.target.value))}
              className="w-full rounded-md border border-line bg-paper px-2 py-1.5 text-[12.5px] text-ink"
            >
              {INTERVAL_OPTIONS.map((o) => (
                <option key={o.secs} value={o.secs}>
                  {o.label}
                </option>
              ))}
            </select>
            <div className="flex gap-2">
              <button
                onClick={submit}
                className="flex-1 rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white transition-opacity hover:opacity-90"
              >
                Create
              </button>
              <button
                onClick={() => setOpen(false)}
                className="rounded-lg border border-line px-3 py-2 text-[13px] font-semibold text-soft hover:bg-card"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setOpen(true)}
            className="w-full rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white transition-opacity hover:opacity-90"
          >
            + New monitor
          </button>
        )}
      </div>
    </aside>
  );
}
```

- [ ] **Step 3: Rewrite `src/App.tsx` to use live data**

```tsx
import { useEffect, useMemo, useState } from "react";
import type { FeedItem, Monitor } from "./types";
import { BRIEF_F1 } from "./mock/data";
import { Sidebar } from "./components/Sidebar";
import { Feed } from "./components/Feed";
import { DigDeeperPanel } from "./components/DigDeeperPanel";
import { listMonitors, listFeed, createMonitor, deleteMonitor, onFeedUpdated } from "./api";

function App() {
  const [monitors, setMonitors] = useState<Monitor[]>([]);
  const [feed, setFeed] = useState<FeedItem[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState<string | null>(null);
  const [digItem, setDigItem] = useState<FeedItem | null>(null);

  const refresh = async () => {
    setMonitors(await listMonitors());
    setFeed(await listFeed());
  };

  useEffect(() => {
    refresh();
    const un = onFeedUpdated(() => refresh());
    return () => {
      un.then((f) => f());
    };
  }, []);

  const activeMonitor = useMemo(
    () => monitors.find((m) => m.id === selectedMonitorId) ?? null,
    [monitors, selectedMonitorId],
  );

  const visibleFeed = useMemo(
    () => (selectedMonitorId ? feed.filter((f) => f.monitorId === selectedMonitorId) : feed),
    [feed, selectedMonitorId],
  );

  const handleCreate = async (name: string, prompt: string, intervalSecs: number) => {
    await createMonitor(name, prompt, intervalSecs);
    await refresh();
  };

  const handleDelete = async (id: string) => {
    await deleteMonitor(id);
    if (selectedMonitorId === id) setSelectedMonitorId(null);
    await refresh();
  };

  return (
    <div className="flex h-full w-full overflow-hidden">
      <Sidebar
        monitors={monitors}
        selectedId={selectedMonitorId}
        onSelect={setSelectedMonitorId}
        onCreate={handleCreate}
        onDelete={handleDelete}
      />

      <Feed
        items={visibleFeed}
        monitors={monitors}
        activeMonitor={activeMonitor}
        onDigDeeper={setDigItem}
      />

      {digItem && (
        <DigDeeperPanel
          item={digItem}
          brief={digItem.id === BRIEF_F1.itemId ? BRIEF_F1 : null}
          onClose={() => setDigItem(null)}
        />
      )}
    </div>
  );
}

export default App;
```

- [ ] **Step 4: Type-check the frontend**

Run: `npm run build`
Expected: `tsc` passes and Vite builds with no type errors. If `Feed`'s props reject an empty `monitors` array or `activeMonitor: null`, they already accept these in the mock shell — no change needed.

- [ ] **Step 5: Commit**

```bash
git add src/api.ts src/App.tsx src/components/Sidebar.tsx
git commit -m "feat: wire UI to live monitors, feed, create/delete, and feed-updated event"
```

---

### Task 8: End-to-end verification in the native window

**Files:** none (verification only).

Follow `docs/TESTING.md` — test the real native window, never localhost.

- [ ] **Step 1: Launch the app**

Run: `npm run tauri dev`
Expected: the native "HN Watch" window opens with an empty monitor list and empty feed (fresh DB).

- [ ] **Step 2: Create a monitor and observe a real tick**

In the window: click **+ New monitor**, enter a name (e.g. "Show HN devtools"), a prompt (e.g. "Show HN posts for developer tools"), pick **every 15m**, click **Create**.
Expected within a few seconds (immediate first tick): the monitor appears in the sidebar; its match count and one or more real feed cards populate as `claude -p` returns matches. Check the dev console/terminal for `[hn-watch]` errors if nothing lands.

- [ ] **Step 3: Verify dedup on a second tick**

Wait for or trigger a second tick (or create a second monitor with a different prompt). Expected: no duplicate cards for the same story under the same monitor; only genuinely new matches appear.

- [ ] **Step 4: Verify persistence across restart**

Close the window and stop `npm run tauri dev`, then relaunch it.
Expected: the monitor(s) and all feed cards are still there (loaded from SQLite), and workers resume ticking.

- [ ] **Step 5: Verify delete**

Hover a monitor row and click **×**. Expected: the monitor disappears; its feed cards are gone after refresh; no worker for it keeps ticking.

- [ ] **Step 6: Update STATUS.md and commit**

Add a "Session 3 — Monitors + real tick loop" entry to `STATUS.md` summarizing what now works (persistence, CRUD, background ticks through `claude -p`, dedup), and move the Phase 2 items from "Next" to done.

```bash
git add STATUS.md
git commit -m "docs: STATUS — monitors + real tick loop working end-to-end"
```

- [ ] **Step 7: Merge the branch (keep it on origin per CLAUDE.md workflow)**

```bash
git push -u origin feat/monitors-and-tick
git checkout main
git merge --no-ff feat/monitors-and-tick -m "Merge feat/monitors-and-tick: persistence + monitors + real tick loop"
git push origin main
```

---

## Self-Review

**Spec coverage:**
- Persistence (SQLite, survive restart) → Task 1 + Task 6 (`init_state`) + Task 8 Step 4. ✅
- Monitor create/list/delete → Task 6 commands + Task 7 UI. ✅
- Long-lived background worker, tick pulls HN → `claude -p` → feed → Task 2/3/4/5. ✅
- One `claude -p` call per tick, semaphore-shared runtime → Task 3 (`judge` + `agent_sem`). ✅
- Dedup via `seen` (all judged ids) → Task 1 + Task 4 (`run_tick`). ✅
- Immediate first tick for testability → Task 5 (`spawn` loop ticks before sleeping). ✅
- Interval presets → Task 7 `INTERVAL_OPTIONS` + Task 6 `interval_label`. ✅
- Error-tolerant ticks (worker survives) → Task 4 returns `Result`, Task 5 logs and continues. ✅
- UI reuses components/tokens, DTOs match `src/types.ts` → Task 6 DTOs + Task 7. ✅
- Out-of-scope items (tray, notifications, swarm, edit/pause, Run-now) not built. ✅

**Placeholder scan:** No TBD/TODO/"add error handling"/"similar to Task N". Every code step contains full code. ✅

**Type consistency:** `Monitor`/`FeedRow` (db) → `HnItem` (hn) → `Verdict` (agent) → `FeedRow` (tick) → `MonitorDto`/`FeedDto` (commands, camelCase) → `Monitor`/`FeedItem` (`src/types.ts`). `run_tick`, `select_unseen`, `build_feed_rows`, `Scheduler::{spawn,stop}`, `init_state`, and the four command names are used consistently across tasks. ✅
