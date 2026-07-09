use crate::agent::ClaudeHealth;
use crate::db::Monitor;
use crate::tick;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

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

/// Build the notification (title, body) from a tick's new matches. Pure — unit-tested.
/// Title: "{name} · N new match(es)". Body: the top match's title, plus " +N more"
/// when more than one landed; falls back to the monitor prompt if no title is known.
fn format_notification(
    name: &str,
    new: i64,
    newest_title: Option<&str>,
    prompt: &str,
) -> (String, String) {
    let noun = if new == 1 { "match" } else { "matches" };
    let title = format!("{name} · {new} new {noun}");
    let body = match newest_title {
        Some(t) if new > 1 => format!("{t} +{} more", new - 1),
        Some(t) => t.to_string(),
        None => prompt.to_string(),
    };
    (title, body)
}

/// Fire one native OS notification for a monitor's new matches. Best-effort:
/// a failed `.show()` is ignored so notification trouble never affects the tick.
fn notify_new_matches(
    app: &AppHandle,
    name: &str,
    new: i64,
    newest_title: Option<&str>,
    prompt: &str,
) {
    let (title, body) = format_notification(name, new, newest_title, prompt);
    let _ = app.notification().builder().title(title).body(body).show();
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
                let (checked, new, error, code, agent_ran, newest_title) = match &result {
                    Ok(o) => (o.checked as i64, o.new as i64, None, None, o.agent_ran, o.newest_title.clone()),
                    Err(e) => {
                        eprintln!(
                            "[hn-watch] tick failed for {}: {} ({}) [{e:?}]",
                            monitor.id,
                            e.message(),
                            e.code()
                        );
                        (0i64, 0i64, Some(e.message()), Some(e.code()), false, None)
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
                    notify_new_matches(&app, &monitor.name, new, newest_title.as_deref(), &monitor.prompt);
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

#[cfg(test)]
mod tests {
    use super::format_notification;

    #[test]
    fn singular_title_and_title_body() {
        let (title, body) =
            format_notification("Rust async", 1, Some("Tokio 2.0 released"), "rust async runtimes");
        assert_eq!(title, "Rust async · 1 new match");
        assert_eq!(body, "Tokio 2.0 released");
    }

    #[test]
    fn plural_title_and_more_suffix() {
        let (title, body) =
            format_notification("AI startups", 3, Some("OpenAI ships thing"), "ai startup launches");
        assert_eq!(title, "AI startups · 3 new matches");
        assert_eq!(body, "OpenAI ships thing +2 more");
    }

    #[test]
    fn body_falls_back_to_prompt_when_no_title() {
        let (title, body) = format_notification("Quiet", 1, None, "some prompt");
        assert_eq!(title, "Quiet · 1 new match");
        assert_eq!(body, "some prompt");
    }
}
