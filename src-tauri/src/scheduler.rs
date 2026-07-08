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
