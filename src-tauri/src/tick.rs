use crate::agent::{self, Verdict};
use crate::db::{self, FeedRow, Monitor};
use crate::hn::{self, HnItem};
use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// First-tick look-back when a monitor has no watermark yet (1 hour).
pub const LOOKBACK_SECS: i64 = 3600;
/// Trailing margin behind the newest story, guarding against Algolia's async indexing (5 min).
pub const WATERMARK_MARGIN_SECS: i64 = 300;
/// Stories per `claude` call.
pub const BATCH_SIZE: usize = 30;
/// Tolerance for clock skew when rejecting far-future timestamps (1 hour).
const CLOCK_SKEW_SECS: i64 = 3600;

/// What one tick did: how many stories it scanned, how many new matches it inserted,
/// and whether this tick actually invoked the agent (false = nothing unseen, judge skipped).
pub struct TickOutcome {
    pub checked: usize,
    pub new: usize,
    pub agent_ran: bool,
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
    let recent = hn::fetch_since(now_secs() - 3600).await.map_err(TickError::Hn)?;
    let checked = recent.len();

    let seen = {
        let conn = db.lock().map_err(|_| TickError::Db("db poisoned".into()))?;
        db::list_seen(&conn, &monitor.id).map_err(|e| TickError::Db(e.to_string()))?
    };
    let unseen = select_unseen(recent, &seen);
    if unseen.is_empty() {
        return Ok(TickOutcome { checked, new: 0, agent_ran: false });
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
    Ok(TickOutcome { checked, new: rows.len(), agent_ran: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> HnItem {
        HnItem {
            hn_id: id.into(), title: format!("t{id}"), url: "https://x.dev/a".into(),
            domain: "x.dev".into(), points: 10, num_comments: 2, created_at: 1_700_000_000,
        }
    }

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
