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

/// Run the planner for `item_id` as a **registered, cancellable** task, returning the proposed
/// angles. Registering under `item_id` means a `cancel(item_id)` — which the panel fires on close
/// in every phase — aborts the planner task, dropping its buffered `claude` child via
/// `kill_on_drop`. That is the same cascade that stops running workers, so closing during the
/// "Planning…" phase kills the planner immediately instead of orphaning it for up to 45s.
/// Returns `Err` if the run was cancelled before planning finished (the frontend ignores it).
pub async fn run_planner(
    db: Arc<Mutex<Connection>>,
    registry: &SwarmRegistry,
    item_id: String,
) -> Result<Vec<PlannedAngle>, String> {
    // Load the story context (lock, read, drop — never held across the await).
    let ctx = {
        let conn = db.lock().map_err(|_| "db poisoned".to_string())?;
        db::get_feed_item(&conn, &item_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "feed item not found".to_string())?
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tauri::async_runtime::spawn(async move {
        // `plan_angles` is infallible (falls back to defaults). If we're aborted mid-flight,
        // this future drops → the `claude` child drops → SIGKILL; the send below never runs.
        let angles = agent::plan_angles(&ctx).await;
        let _ = tx.send(angles); // receiver gone (cancelled) → ignore
    });
    registry.insert(item_id, handle);
    // Task aborted by `cancel` → sender dropped → `rx` errors → report cancellation.
    rx.await.map_err(|_| "planning cancelled".to_string())
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
        // Result carries the error text (not just None) so a saved run can show why an angle failed.
        let mut set: JoinSet<(PlannedAngle, Result<String, String>)> = JoinSet::new();
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
                (angle, result.map_err(|e| e.message()))
            });
        }

        // Join all workers (they run concurrently; this just gathers results).
        let mut results: Vec<(PlannedAngle, Result<String, String>)> = Vec::new();
        while let Some(res) = set.join_next().await {
            if let Ok(pair) = res {
                results.push(pair);
            }
        }

        // Degraded-vs-failed: if every angle failed, don't synthesize from nothing.
        if results.iter().all(|(_, out)| out.is_err()) {
            let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "all research angles failed".into() });
            return;
        }

        // synthesize still consumes Option<String> findings.
        let synth_input: Vec<(PlannedAngle, Option<String>)> = results
            .iter()
            .map(|(a, r)| (a.clone(), r.clone().ok()))
            .collect();
        match agent::synthesize(&ctx, &synth_input).await {
            Ok(brief) => {
                // Persist the completed run (latest-wins) so a reopen shows it without re-running.
                let saved: Vec<db::SavedAngle> = results
                    .iter()
                    .map(|(a, r)| db::SavedAngle {
                        id: a.id.clone(),
                        icon: a.icon.clone(),
                        label: a.label.clone(),
                        focus: a.focus.clone(),
                        status: if r.is_ok() { "done".into() } else { "failed".into() },
                        findings: r.clone().ok(),
                        error: r.clone().err(),
                    })
                    .collect();
                if let Ok(conn) = db.lock() {
                    let _ = db::save_research(&conn, &item_id, &brief, &saved, crate::tick::now_secs());
                }
                let _ = app.emit("swarm-brief-ready", SwarmBriefReady { item_id, brief });
            }
            Err(e) => {
                let _ = app.emit("swarm-failed", SwarmFailed { item_id, error: e.message() });
            }
        }
    });

    registry.insert(registry_key, handle);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Is `pid` a live, non-zombie process? A SIGKILLed-but-not-yet-reaped child briefly lingers
    /// as a zombie ("Z"), which `kill -0` would still report as existing — so key off the state.
    fn alive(pid: u32) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
            .expect("run ps");
        let state = String::from_utf8_lossy(&out.stdout);
        let state = state.trim();
        !state.is_empty() && !state.starts_with('Z')
    }

    /// The cancellation guarantee `run_planner` (and `run_swarm`) rely on: a task registered in
    /// the `SwarmRegistry` that owns a `kill_on_drop` child is SIGKILLed by `cancel` — the same
    /// abort → drop-future → drop-`Child` → kill cascade. This covers the planner phase without
    /// needing a live `claude`: the planner spawns its buffered `claude` child exactly this way
    /// (`claude_command()` sets `kill_on_drop(true)`, then `.output().await`). The registered task
    /// runs on Tauri's runtime (as in production); the test body waits with plain std blocking.
    #[test]
    fn cancel_sigkills_registered_childs_process() {
        use std::sync::mpsc;

        let registry = SwarmRegistry::new();
        let (tx, rx) = mpsc::channel::<u32>();

        // Mirror the planner path: a registered task owns a `kill_on_drop` child and awaits it
        // (the future that gets dropped when the task is aborted).
        let handle = tauri::async_runtime::spawn(async move {
            let mut child = tokio::process::Command::new("sleep")
                .arg("30")
                .kill_on_drop(true)
                .spawn()
                .expect("spawn sleep");
            let pid = child.id().expect("child pid");
            let _ = tx.send(pid);
            let _ = child.wait().await; // dropped on abort → child dropped → SIGKILL
        });
        registry.insert("item-1".to_string(), handle);

        let pid = rx.recv_timeout(Duration::from_secs(5)).expect("receive child pid");
        assert!(alive(pid), "child must be running before cancel");

        registry.cancel("item-1"); // abort the registered task

        // kill_on_drop signals + reaps asynchronously; poll briefly for the process to vanish.
        let mut dead = false;
        for _ in 0..60 {
            if !alive(pid) {
                dead = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(dead, "cancel must SIGKILL the registered child's process (pid {pid})");
    }

    /// Cancelling an unknown item is a harmless no-op (planner not yet registered, double-close).
    #[test]
    fn cancel_unknown_item_is_noop() {
        SwarmRegistry::new().cancel("never-registered");
    }
}
