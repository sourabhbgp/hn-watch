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
             created_at INTEGER NOT NULL,
             UNIQUE(monitor_id, hn_id)
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
}
