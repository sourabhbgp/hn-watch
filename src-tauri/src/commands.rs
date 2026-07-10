use crate::agent::{self, ClaudeHealth};
use crate::db::{self, Monitor};
use crate::scheduler::Scheduler;
use crate::swarm::{self, SwarmRegistry};
use crate::tick::now_secs;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub scheduler: Scheduler,
    pub claude_health: Arc<Mutex<ClaudeHealth>>,
    pub swarm: SwarmRegistry,
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
    pub last_checked_at: Option<i64>,
    pub next_check_at: Option<i64>,
    pub last_checked_count: Option<i64>,
    pub last_new_count: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeHealthDto {
    pub status: String,
    pub message: String,
}

impl ClaudeHealthDto {
    pub fn from_health(h: &ClaudeHealth) -> Self {
        ClaudeHealthDto { status: h.code().into(), message: h.message() }
    }
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

fn next_check_at(last_checked_at: Option<i64>, interval_secs: i64) -> Option<i64> {
    last_checked_at.map(|t| t + interval_secs)
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

fn to_monitor_dto(conn: &Connection, m: &Monitor, claude_ok: bool) -> rusqlite::Result<MonitorDto> {
    let status = if !claude_ok {
        "paused"
    } else if m.last_error.is_some() {
        "error"
    } else {
        "active"
    };
    Ok(MonitorDto {
        id: m.id.clone(),
        name: m.name.clone(),
        prompt: m.prompt.clone(),
        interval_label: interval_label(m.interval_secs),
        status: status.into(),
        match_count: db::count_matches(conn, &m.id)?,
        last_checked_at: m.last_checked_at,
        next_check_at: next_check_at(m.last_checked_at, m.interval_secs),
        last_checked_count: m.last_checked_count,
        last_new_count: m.last_new_count,
        last_error: m.last_error.clone(),
    })
}

#[tauri::command]
pub fn list_monitors(state: State<'_, AppState>) -> Result<Vec<MonitorDto>, String> {
    let claude_ok = state
        .claude_health
        .lock()
        .map_err(|_| "health poisoned".to_string())?
        .is_ok();
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    let monitors = db::list_monitors(&conn).map_err(|e| e.to_string())?;
    monitors
        .iter()
        .map(|m| to_monitor_dto(&conn, m, claude_ok).map_err(|e| e.to_string()))
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
        last_checked_at: None,
        last_checked_count: None,
        last_new_count: None,
        last_error: None,
        watermark: None,
    };
    if monitor.name.is_empty() || monitor.prompt.is_empty() {
        return Err("name and prompt are required".into());
    }
    let claude_ok = state
        .claude_health
        .lock()
        .map_err(|_| "health poisoned".to_string())?
        .is_ok();
    let dto = {
        let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
        db::insert_monitor(&conn, &monitor).map_err(|e| e.to_string())?;
        to_monitor_dto(&conn, &monitor, claude_ok).map_err(|e| e.to_string())?
    };
    state
        .scheduler
        .spawn(app, Arc::clone(&state.db), Arc::clone(&state.claude_health), monitor);
    Ok(dto)
}

#[tauri::command]
pub fn delete_monitor(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.scheduler.stop(&id);
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    db::delete_monitor(&conn, &id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn claude_health(state: State<'_, AppState>) -> Result<ClaudeHealthDto, String> {
    let h = state.claude_health.lock().map_err(|_| "health poisoned".to_string())?;
    Ok(ClaudeHealthDto::from_health(&h))
}

/// Apply a freshly-probed ClaudeHealth: store it, clear stale per-monitor errors when
/// healthy (so recovered monitors show `active`, not a stale `error`), and notify the UI.
/// Shared by the startup preflight and the Re-check command. Lock order health→db, and
/// neither guard is held across the emit.
fn apply_claude_health(
    db: &Arc<Mutex<Connection>>,
    health_state: &Arc<Mutex<ClaudeHealth>>,
    app: &AppHandle,
    new_health: ClaudeHealth,
) {
    if let Ok(mut h) = health_state.lock() {
        *h = new_health.clone();
    }
    if new_health.is_ok() {
        if let Ok(conn) = db.lock() {
            let _ = db::clear_all_errors(&conn);
        }
    }
    let _ = app.emit("claude-health", ClaudeHealthDto::from_health(&new_health));
}

#[tauri::command]
pub async fn recheck_claude(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ClaudeHealthDto, String> {
    let health = agent::preflight().await; // no lock held across the await
    apply_claude_health(&state.db, &state.claude_health, &app, health.clone());
    Ok(ClaudeHealthDto::from_health(&health))
}

/// Start dig-deeper on a feed item: load its context and run the planner, returning the
/// proposed angles. Nothing runs yet — the frontend edits the list and calls `confirm_dig_deeper`.
/// Stateless across the two calls (the proposal lives in the frontend, not the backend).
#[tauri::command]
pub async fn start_dig_deeper(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Vec<agent::PlannedAngle>, String> {
    // Load context (lock, read, drop) before the await — never hold the guard across it.
    let ctx = {
        let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
        db::get_feed_item(&conn, &item_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "feed item not found".to_string())?
    };
    Ok(agent::plan_angles(&ctx).await)
}

/// Confirm the (edited) angle list and fire the swarm. Re-clamps to MIN..=MAX server-side.
#[tauri::command]
pub fn confirm_dig_deeper(
    app: AppHandle,
    state: State<'_, AppState>,
    item_id: String,
    angles: Vec<agent::PlannedAngle>,
) -> Result<(), String> {
    let mut angles = angles;
    angles.truncate(agent::MAX_ANGLES);
    if angles.len() < agent::MIN_ANGLES {
        return Err(format!("need at least {} angles", agent::MIN_ANGLES));
    }
    swarm::run_swarm(app, Arc::clone(&state.db), &state.swarm, item_id, angles);
    Ok(())
}

/// Cancel a running swarm (panel closed / switched items). Idempotent.
#[tauri::command]
pub fn cancel_dig_deeper(state: State<'_, AppState>, item_id: String) -> Result<(), String> {
    state.swarm.cancel(&item_id);
    Ok(())
}

/// Called once at startup: open/create the DB, spawn a worker per monitor, and
/// kick off an async Claude preflight (never blocks the window).
pub fn init_state(app: &AppHandle) -> AppState {
    let dir = app.path().app_data_dir().expect("no app data dir");
    std::fs::create_dir_all(&dir).ok();
    let conn = Connection::open(dir.join("hn-watch.sqlite")).expect("open db");
    db::migrate(&conn).expect("migrate db");
    let db = Arc::new(Mutex::new(conn));
    let scheduler = Scheduler::new();
    let claude_health = Arc::new(Mutex::new(ClaudeHealth::Ok));
    let swarm = SwarmRegistry::new();

    let existing = {
        let conn = db.lock().unwrap();
        db::list_monitors(&conn).unwrap_or_default()
    };
    for m in existing {
        scheduler.spawn(app.clone(), Arc::clone(&db), Arc::clone(&claude_health), m);
    }

    // Startup preflight, async so the window opens immediately.
    {
        let app = app.clone();
        let db = Arc::clone(&db);
        let health = Arc::clone(&claude_health);
        tauri::async_runtime::spawn(async move {
            let result = agent::preflight().await;
            apply_claude_health(&db, &health, &app, result);
        });
    }

    AppState { db, scheduler, claude_health, swarm }
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

    #[test]
    fn next_check_at_adds_interval_or_none() {
        assert_eq!(next_check_at(Some(1000), 1800), Some(2800));
        assert_eq!(next_check_at(None, 1800), None);
    }

    #[test]
    fn status_paused_overrides_error_when_claude_down() {
        use crate::db::Monitor;
        let c = Connection::open_in_memory().unwrap();
        db::migrate(&c).unwrap();
        let mut m = Monitor {
            id: "m1".into(), name: "n".into(), prompt: "p".into(),
            interval_secs: 1800, created_at: 1,
            last_checked_at: Some(10), last_checked_count: Some(5),
            last_new_count: Some(0), last_error: None, watermark: None,
        };
        db::insert_monitor(&c, &m).unwrap();
        // Claude down → paused, regardless of last_error
        assert_eq!(to_monitor_dto(&c, &m, false).unwrap().status, "paused");
        // Claude ok, no error → active
        assert_eq!(to_monitor_dto(&c, &m, true).unwrap().status, "active");
        // Claude ok, error set → error
        m.last_error = Some("Claude timed out".into());
        assert_eq!(to_monitor_dto(&c, &m, true).unwrap().status, "error");
    }
}
