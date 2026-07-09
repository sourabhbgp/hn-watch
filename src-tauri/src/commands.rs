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
    pub last_checked_at: Option<i64>,
    pub next_check_at: Option<i64>,
    pub last_checked_count: Option<i64>,
    pub last_new_count: Option<i64>,
    pub last_error: Option<String>,
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
        last_checked_at: None,
        last_checked_count: None,
        last_new_count: None,
        last_error: None,
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

    #[test]
    fn next_check_at_adds_interval_or_none() {
        assert_eq!(next_check_at(Some(1000), 1800), Some(2800));
        assert_eq!(next_check_at(None, 1800), None);
    }
}
