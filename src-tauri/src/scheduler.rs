use crate::agent::ClaudeHealth;
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
struct ClaudeHealthPayload {
    status: String,
    message: String,
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
    /// `health` is the shared global Claude state — this worker flips it to
    /// Missing/NotAuthenticated on those failures and clears it on success.
    pub fn spawn(
        &self,
        app: AppHandle,
        db: Arc<Mutex<Connection>>,
        health: Arc<Mutex<ClaudeHealth>>,
        monitor: Monitor,
    ) {
        let interval = Duration::from_secs(monitor.interval_secs.max(1) as u64);
        let id = monitor.id.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let mut monitor = monitor;
            loop {
                let _ = app.emit("tick-started", TickStarted { monitor_id: monitor.id.clone() });

                let result = tick::run_tick(&db, &monitor).await;
                if let Ok(o) = &result {
                    if let Some(wm) = o.watermark {
                        monitor.watermark = Some(wm);
                    }
                }
                let now = tick::now_secs();
                let (checked, new, error, code, agent_ran) = match &result {
                    Ok(o) => (o.checked as i64, o.new as i64, None, None, o.agent_ran),
                    Err(e) => {
                        eprintln!(
                            "[hn-watch] tick failed for {}: {} ({}) [{e:?}]",
                            monitor.id,
                            e.message(),
                            e.code()
                        );
                        (0i64, 0i64, Some(e.message()), Some(e.code()), false)
                    }
                };

                match db.lock() {
                    Ok(conn) => {
                        if let Err(e) = db::record_tick(
                            &conn,
                            &monitor.id,
                            checked,
                            new,
                            error.as_deref(),
                            now,
                        ) {
                            eprintln!("[hn-watch] record_tick failed for {}: {e}", monitor.id);
                        }
                    }
                    Err(_) => {
                        eprintln!("[hn-watch] db poisoned; skipped record_tick for {}", monitor.id)
                    }
                }

                // Global Claude health: only claude_missing / claude_auth flip it;
                // a tick that actually ran the agent and succeeded clears it; a
                // transient error, or a successful early return that never ran the
                // agent, leaves it unchanged.
                if let Ok(mut h) = health.lock() {
                    let next = crate::agent::next_claude_health(&*h, agent_ran, code);
                    if *h != next {
                        *h = next.clone();
                        let _ = app.emit(
                            "claude-health",
                            ClaudeHealthPayload {
                                status: next.code().into(),
                                message: next.message(),
                            },
                        );
                    }
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
