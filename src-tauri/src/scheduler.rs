use crate::db::Monitor;
use crate::tick;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

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
