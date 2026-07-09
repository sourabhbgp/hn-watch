use rusqlite::Connection;
use std::collections::HashSet;

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

pub fn delete_monitor(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    conn.execute("DELETE FROM monitors WHERE id = ?1", [id])?;
    Ok(())
}

pub fn insert_feed_item(conn: &Connection, f: &FeedRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO feed_items
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

/// Null out the persisted per-tick error on every monitor. Used when Claude health
/// recovers via preflight / Re-check (which set health Ok without running a tick), so
/// recovered monitors show `active` immediately instead of a stale `error` chip.
pub fn clear_all_errors(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("UPDATE monitors SET last_error = NULL", [])?;
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
            last_checked_at: None,
            last_checked_count: None,
            last_new_count: None,
            last_error: None,
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

    #[test]
    fn feed_item_dedup_on_monitor_and_hn_id() {
        let c = mem();
        insert_monitor(&c, &sample_monitor("m1")).unwrap();
        let mut row = FeedRow {
            id: "f1".into(), monitor_id: "m1".into(), hn_id: "hn1".into(),
            title: "t".into(), url: "u".into(), domain: "d".into(),
            summary: "s".into(), reason: "r".into(), hn_score: 1, hn_comments: 1, created_at: 1,
        };
        insert_feed_item(&c, &row).unwrap();
        row.id = "f2".into(); // different row id, same (monitor_id, hn_id)
        insert_feed_item(&c, &row).unwrap();
        assert_eq!(count_matches(&c, "m1").unwrap(), 1);
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
    fn clear_all_errors_nulls_every_monitor() {
        let c = mem();
        insert_monitor(&c, &sample_monitor("m1")).unwrap();
        let mut m2 = sample_monitor("m2");
        m2.name = "AI2".into();
        insert_monitor(&c, &m2).unwrap();
        record_tick(&c, "m1", 5, 0, Some("Claude Code was not found on this machine"), 1).unwrap();
        record_tick(&c, "m2", 5, 0, Some("Claude isn't logged in"), 1).unwrap();
        clear_all_errors(&c).unwrap();
        for m in list_monitors(&c).unwrap() {
            assert_eq!(m.last_error, None);
        }
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
}
