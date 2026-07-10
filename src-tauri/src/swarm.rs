use crate::agent::{self, Brief, PlannedAngle};
use crate::db;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter};
use tokio::task::JoinSet;

/// Tracks the currently-running dig-deeper orchestration per feed item, so a panel close
/// (or switching items) can abort it. Mirrors `Scheduler.handles`.
pub struct SwarmRegistry {
    handles: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl SwarmRegistry {
    pub fn new() -> Self {
        SwarmRegistry { handles: Mutex::new(HashMap::new()) }
    }

    fn insert(&self, item_id: String, handle: JoinHandle<()>) {
        // Replacing an existing run for the same item aborts the old one first.
        if let Some(old) = self.handles.lock().unwrap().insert(item_id, handle) {
            old.abort();
        }
    }

    /// Abort + forget a running swarm. Aborting the orchestration task drops its `JoinSet`,
    /// whose `Drop` aborts every in-flight worker task — dropping each worker's `swarm_sem`
    /// permit and its `kill_on_drop(true)` `Child` (SIGKILL to the OS `claude` process) — so
    /// no leaked permit and no orphan process.
    pub fn cancel(&self, item_id: &str) {
        if let Some(handle) = self.handles.lock().unwrap().remove(item_id) {
            handle.abort();
        }
    }
}

impl Default for SwarmRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---- event payloads (camelCase, mirroring the tick events) ----

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmProgress {
    item_id: String,
    angle_id: String,
    line: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmAngleDone {
    item_id: String,
    angle_id: String,
    output: Option<String>,
    error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmBriefReady {
    item_id: String,
    brief: Brief,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmFailed {
    item_id: String,
    error: String,
}

/// Start (or restart) the swarm for `item_id` with the confirmed `angles`. Spawns one
/// orchestration task and registers it for cancellation. The task: loads the item context,
/// fans out one streaming worker per angle (all start at once — SWARM_PERMITS == MAX_ANGLES),
/// forwards progress, joins, then synthesizes and emits the brief. All angles failing → `swarm-failed`.
pub fn run_swarm(
    app: AppHandle,
    db: Arc<Mutex<Connection>>,
    registry: &SwarmRegistry,
    item_id: String,
    angles: Vec<PlannedAngle>,
) {
    // Capture the registry key before `item_id` is moved into the task.
    let registry_key = item_id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        // Load the story context (lock, read, drop — never held across an await).
        let ctx = {
            let conn = match db.lock() {
                Ok(c) => c,
                Err(_) => {
                    let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "db unavailable".into() });
                    return;
                }
            };
            db::get_feed_item(&conn, &item_id).ok().flatten()
        };
        let ctx = match ctx {
            Some(c) => Arc::new(c),
            None => {
                let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "feed item not found".into() });
                return;
            }
        };

        // Fan out: one worker task per angle, all concurrent. A `JoinSet` (not detached
        // handles) is what makes cancel cascade — dropping this `set` when the orchestration
        // task is aborted aborts every still-running worker.
        let mut set: JoinSet<(PlannedAngle, Option<String>)> = JoinSet::new();
        for angle in angles {
            let app = app.clone();
            let ctx = Arc::clone(&ctx);
            let item_id = item_id.clone();
            set.spawn(async move {
                let angle_id = angle.id.clone();
                let progress_app = app.clone();
                let progress_item = item_id.clone();
                let progress_angle = angle_id.clone();
                let result = agent::stream_investigate(&ctx, &angle, move |line| {
                    let _ = progress_app.emit(
                        "swarm-progress",
                        SwarmProgress {
                            item_id: progress_item.clone(),
                            angle_id: progress_angle.clone(),
                            line,
                        },
                    );
                })
                .await;
                match &result {
                    Ok(output) => {
                        let _ = app.emit("swarm-angle-done", SwarmAngleDone {
                            item_id: item_id.clone(),
                            angle_id,
                            output: Some(output.clone()),
                            error: None,
                        });
                    }
                    Err(e) => {
                        let _ = app.emit("swarm-angle-done", SwarmAngleDone {
                            item_id: item_id.clone(),
                            angle_id,
                            output: None,
                            error: Some(e.message()),
                        });
                    }
                }
                (angle, result.ok())
            });
        }

        // Join all workers (they run concurrently; this just gathers results).
        let mut results: Vec<(PlannedAngle, Option<String>)> = Vec::new();
        while let Some(res) = set.join_next().await {
            if let Ok(pair) = res {
                results.push(pair);
            }
        }

        // Degraded-vs-failed: if every angle failed, don't synthesize from nothing.
        if results.iter().all(|(_, out)| out.is_none()) {
            let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "all research angles failed".into() });
            return;
        }

        match agent::synthesize(&ctx, &results).await {
            Ok(brief) => {
                let _ = app.emit("swarm-brief-ready", SwarmBriefReady { item_id, brief });
            }
            Err(e) => {
                let _ = app.emit("swarm-failed", SwarmFailed { item_id, error: e.message() });
            }
        }
    });

    registry.insert(registry_key, handle);
}
