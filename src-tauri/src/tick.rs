use crate::agent::{self, Verdict};
use crate::db::{self, FeedRow, Monitor};
use crate::hn::{self, HnItem};
use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// What one tick did: how many stories it scanned and how many new matches it inserted.
pub struct TickOutcome {
    pub checked: usize,
    pub new: usize,
}

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

/// A classified tick failure. `code()` feeds paused-vs-error + global health;
/// `message()` is the friendly reason stored in `last_error`.
#[derive(Debug)]
pub enum TickError {
    // Underlying fetch error retained (mirrors Db's shape) for logs/Debug; the
    // user-facing message() stays a generic, friendly string.
    Hn(#[allow(dead_code)] String),
    Agent(agent::AgentError),
    Db(String),
}

impl TickError {
    pub fn code(&self) -> &'static str {
        match self {
            TickError::Hn(_) => "hn_error",
            TickError::Agent(a) => a.code(),
            TickError::Db(_) => "db_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            TickError::Hn(_) => "Couldn't reach Hacker News".into(),
            TickError::Agent(a) => a.message(),
            TickError::Db(e) => format!("Local database error: {e}"),
        }
    }
}

/// One tick: fetch recent HN, drop already-seen items, judge the rest with
/// claude, persist matches, and record every judged id as seen. Returns a
/// TickOutcome with how many stories were scanned and how many new matches were
/// inserted. Errors are propagated so the worker can log them; the worker keeps
/// running regardless.
pub async fn run_tick(
    db: &Arc<Mutex<Connection>>,
    monitor: &Monitor,
) -> Result<TickOutcome, TickError> {
    let recent = hn::fetch_recent(30).await.map_err(TickError::Hn)?;
    let checked = recent.len();

    let seen = {
        let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
        db::list_seen(&conn, &monitor.id).map_err(|e| TickError::Db(e.to_string()))?
    };
    let unseen = select_unseen(recent, &seen);
    if unseen.is_empty() {
        return Ok(TickOutcome { checked, new: 0 });
    }

    let verdicts = agent::judge(&monitor.prompt, &unseen)
        .await
        .map_err(TickError::Agent)?;
    let rows = build_feed_rows(&monitor.id, &unseen, &verdicts, now_secs());

    let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
    for row in &rows {
        db::insert_feed_item(&conn, row).map_err(|e| TickError::Db(e.to_string()))?;
    }
    for item in &unseen {
        db::mark_seen(&conn, &monitor.id, &item.hn_id).map_err(|e| TickError::Db(e.to_string()))?;
    }
    Ok(TickOutcome { checked, new: rows.len() })
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

    #[test]
    fn tick_error_projection() {
        assert_eq!(TickError::Hn("boom".into()).code(), "hn_error");
        assert_eq!(TickError::Db("x".into()).code(), "db_error");
        assert_eq!(
            TickError::Agent(agent::AgentError::NotAuthenticated).code(),
            "claude_auth"
        );
        assert!(TickError::Hn("boom".into()).message().contains("Hacker News"));
        assert_eq!(
            TickError::Agent(agent::AgentError::Timeout).message(),
            agent::AgentError::Timeout.message()
        );
    }
}
