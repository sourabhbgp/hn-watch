# Tray + Native Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep HN Watch running in the macOS system tray when its window is closed, and fire a native OS notification when a monitor tick lands new matches.

**Architecture:** Two independent slices on the existing Tauri v2 app. Slice 1 (tray + close-to-tray) adds a menu-bar icon with Show/Quit and intercepts window-close to hide instead of quit — all Rust, in a new `tray.rs` plus `lib.rs` setup. Slice 2 (notifications) fires an OS notification from the Rust scheduler at the existing `new > 0` site, using a pure, unit-tested formatting helper and a new additive `TickOutcome.newest_title` field. No frontend (`src/`) changes.

**Tech Stack:** Rust, Tauri 2 (`tray-icon` feature, `tauri-plugin-notification` v2), `rusqlite`, `tokio`. Existing modules: `lib.rs`, `scheduler.rs`, `tick.rs`, `commands.rs`.

## Global Constraints

- Notifications and tray/window control are driven from **Rust**, never the frontend. No changes under `src/`.
- Reuse existing design/tokens where UI is involved — here there is no new UI, so nothing to add.
- The tick/ingestion/agent/dedup/watermark logic is **not** altered beyond adding one additive field (`TickOutcome.newest_title`); a failed tick must still never kill its worker.
- Menu item copy is exact: **`Show HN Watch`** and **`Quit HN Watch`**.
- Notification title copy is exact: **`{monitor.name} · {N} new match`** for N==1, **`{monitor.name} · {N} new matches`** for N>1 (middle dot `·`, U+00B7).
- Every task keeps `cargo build` green and the existing test suite passing (34 tests today; additive only).
- Commit at the end of each task with the exact message given.

---

### Task 1: Keep running in the system tray (tray icon + close-to-tray)

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/Cargo.toml` (enable `tray-icon` feature on `tauri`)
- Modify: `src-tauri/src/lib.rs` (declare `mod tray`; build tray + close-to-tray in `setup`)

**Interfaces:**
- Consumes: existing `commands::init_state(app.handle())` and the `"main"` window declared in `tauri.conf.json`.
- Produces: `pub fn tray::build(app: &tauri::AppHandle) -> tauri::Result<()>` and (private to `tray.rs`) `fn show_main_window(app: &tauri::AppHandle)`. `lib.rs` calls `tray::build(app.handle())?` in `setup` and registers a `WindowEvent::CloseRequested` handler that hides the window.

- [ ] **Step 1: Enable the `tray-icon` feature on Tauri**

In `src-tauri/Cargo.toml`, change the `tauri` dependency line:

```toml
tauri = { version = "2", features = ["tray-icon"] }
```

- [ ] **Step 2: Verify it still builds**

Run: `cd src-tauri && cargo build`
Expected: compiles (a warning-free build; the feature adds tray APIs but nothing uses them yet).

- [ ] **Step 3: Create `src-tauri/src/tray.rs`**

```rust
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// Restore the main window: show it, un-minimize, and focus. Best-effort —
/// every call is ignore-on-error so a missing/closing window never panics.
fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Build the menu-bar tray icon with a Show / Quit menu. Left-clicking the
/// tray icon opens this menu (Tauri's default when a menu is attached), so
/// "Show HN Watch" is always one click away — the guaranteed window-restore
/// path. Quit calls `app.exit(0)`, which bypasses the close-to-tray handler.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show HN Watch", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit HN Watch", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });

    // Reuse the app's existing icon for the tray glyph.
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}
```

- [ ] **Step 4: Wire the tray and close-to-tray into `lib.rs` setup**

In `src-tauri/src/lib.rs`, add `mod tray;` with the other module declarations, and replace the `setup` closure body so it (a) keeps the existing state init and (b) adds the tray + close handler. Update the imports line to include `WindowEvent`.

Change the top of the file from:

```rust
use tauri::Manager;
```

to:

```rust
use tauri::{Manager, WindowEvent};
```

Add `mod tray;` to the module list (alongside `mod scheduler;` etc.).

Replace the `.setup(|app| { ... })` block with:

```rust
        .setup(|app| {
            // app.handle() already returns &AppHandle — no extra & (init_state takes &AppHandle).
            let state = commands::init_state(app.handle());
            app.manage(state);

            // Menu-bar tray icon (Show / Quit) — keeps the app alive with the window closed.
            tray::build(app.handle())?;

            // Close-to-tray: the red button / Cmd-W hides the window instead of
            // quitting, so monitor workers keep ticking. Quit lives in the tray menu.
            if let Some(win) = app.get_webview_window("main") {
                let win_for_events = win.clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_for_events.hide();
                    }
                });
            }

            Ok(())
        })
```

- [ ] **Step 5: Build and run the existing test suite**

Run: `cd src-tauri && cargo build && cargo test`
Expected: build is clean; all existing tests pass (34 today). No new unit tests here — tray/window behavior is glue verified live in Step 6.

- [ ] **Step 6: Live-verify in the native window (per `docs/TESTING.md`)**

Build and launch the release app, then drive with computer-use:
- Close the window (red button) → the window disappears but the app is **still running** (tray icon present in the menu bar); a monitor tick still fires (feed grows / `tick-finished` in logs).
- Click the tray icon → menu shows **Show HN Watch** / **Quit HN Watch**. Click **Show HN Watch** → the window returns, focused.
- Click the tray icon → **Quit HN Watch** → the process exits.

Run: `cd src-tauri && cargo build --release` then launch `src-tauri/target/release/bundle/macos/*.app` (or `npm run tauri build` per `docs/TESTING.md`).
Expected: all three behaviors as described.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/tray.rs src-tauri/src/lib.rs
git commit -m "feat(tray): keep running in the system tray with the window closed

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Fire a native notification when new items land

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `tauri-plugin-notification`)
- Modify: `src-tauri/capabilities/default.json` (add `notification:default`)
- Modify: `src-tauri/src/lib.rs` (register the plugin; request notification permission in `setup`)
- Modify: `src-tauri/src/tick.rs` (add `TickOutcome.newest_title`; set it in `run_tick`)
- Modify: `src-tauri/src/scheduler.rs` (pure `format_notification` helper + tests; fire the notification when `new > 0`)

**Interfaces:**
- Consumes: `TickOutcome` from `tick.rs` (now with `newest_title: Option<String>`), the `AppHandle` already in scope in the scheduler worker, `monitor.name`, and the `new` count.
- Produces: `fn format_notification(name: &str, new: i64, newest_title: Option<&str>, prompt: &str) -> (String, String)` (private to `scheduler.rs`, unit-tested) returning `(title, body)`; and a private `fn notify_new_matches(app: &AppHandle, name: &str, new: i64, newest_title: Option<&str>, prompt: &str)` that formats and shows the OS notification.

- [ ] **Step 1: Add `newest_title` to `TickOutcome` and set it in `run_tick`**

In `src-tauri/src/tick.rs`, extend the struct (around line 22):

```rust
pub struct TickOutcome {
    pub checked: usize,
    pub new: usize,
    pub agent_ran: bool,
    pub watermark: Option<i64>,
    /// Title of the top match this tick landed (first inserted feed row), for the
    /// notification body. `None` when nothing new was inserted.
    pub newest_title: Option<String>,
}
```

Update the two `Ok(TickOutcome { ... })` construction sites:

The early-return (all-seen) site (around line 152) — nothing new, so `None`:

```rust
        return Ok(TickOutcome { checked, new: 0, agent_ran: false, watermark: new_watermark, newest_title: None });
```

The happy-path site (around line 176) — the top match's title:

```rust
    Ok(TickOutcome {
        checked,
        new: rows.len(),
        agent_ran: true,
        watermark: new_watermark,
        newest_title: rows.first().map(|r| r.title.clone()),
    })
```

- [ ] **Step 2: Build to confirm the additive field compiles**

Run: `cd src-tauri && cargo build`
Expected: compiles. (`newest_title` is only read in Task 2 Step 5; unused-field warnings are acceptable at this intermediate step and disappear once the scheduler reads it.)

- [ ] **Step 3: Write the failing test for `format_notification`**

In `src-tauri/src/scheduler.rs`, add a test module at the end of the file:

```rust
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
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cd src-tauri && cargo test format_notification`
Expected: FAIL to compile — `cannot find function 'format_notification' in this scope`.

- [ ] **Step 5: Implement `format_notification` + `notify_new_matches`, register the plugin dep, and fire on `new > 0`**

**5a. Add the plugin dependency.** In `src-tauri/Cargo.toml`:

```toml
tauri-plugin-notification = "2"
```

**5b. Add the capability.** In `src-tauri/capabilities/default.json`, add `"notification:default"` to `permissions`:

```json
  "permissions": [
    "core:default",
    "opener:default",
    "notification:default"
  ]
```

**5c. Register the plugin and request permission in `lib.rs`.** Add the plugin to the builder chain (next to `tauri_plugin_opener::init()`):

```rust
        .plugin(tauri_plugin_notification::init())
```

And inside `setup`, after `tray::build(...)`, request notification permission once (best-effort, so the first real notification on macOS isn't dropped before the OS prompt is answered):

```rust
            // Ask for notification permission up front (macOS shows the OS prompt once).
            {
                use tauri_plugin_notification::{NotificationExt, PermissionState};
                let n = app.notification();
                if !matches!(n.permission_state(), Ok(PermissionState::Granted)) {
                    let _ = n.request_permission();
                }
            }
```

**5d. Add the helpers to `scheduler.rs`.** At the top of `scheduler.rs`, add the import:

```rust
use tauri_plugin_notification::NotificationExt;
```

Add both functions (near the top of the file, after the imports):

```rust
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
```

**5e. Capture `newest_title` and fire on `new > 0`.** In the worker loop, extend the destructured tuple to carry `newest_title`, then call `notify_new_matches` next to the existing `feed-updated` emit.

Change the match that builds the per-tick locals from:

```rust
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
```

to:

```rust
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
```

And change the new-match emit block from:

```rust
                if new > 0 {
                    let _ = app.emit("feed-updated", ());
                }
```

to:

```rust
                if new > 0 {
                    let _ = app.emit("feed-updated", ());
                    notify_new_matches(&app, &monitor.name, new, newest_title.as_deref(), &monitor.prompt);
                }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test`
Expected: PASS — the three `format_notification` tests plus all existing tests (37 total).

- [ ] **Step 7: Build clean**

Run: `cd src-tauri && cargo build`
Expected: compiles with no warnings (the `newest_title` field is now read).

- [ ] **Step 8: Live-verify in the native window (per `docs/TESTING.md`)**

Build/launch the release app. Create (or keep) a monitor with a broad prompt likely to match current HN (e.g. name "Anything", prompt "any technology or software story") so a tick lands ≥1 match. On that tick:
- A native macOS notification appears titled **`Anything · N new match(es)`** with a story-title body.
- Clicking the notification activates the app (best-effort; the guaranteed restore path is the tray **Show HN Watch** item — verify that still works).
- Regression: fully **Quit** (tray) → relaunch → monitors + feed persist; the Claude-health banner and countdown UI behave as before.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/capabilities/default.json src-tauri/src/lib.rs src-tauri/src/tick.rs src-tauri/src/scheduler.rs
git commit -m "feat(notify): native notification when a monitor lands new matches

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Close-to-tray + tray icon with Show/Quit → Task 1. ✓
- Tray **Show** / tray-icon click restore path → Task 1 (menu opens on tray click; **Show HN Watch** restores). The spec's separate `on_tray_icon_event` left-click handler is intentionally folded into "menu opens on left click" — a version-safe simplification that still delivers one-click access to Show. ✓ (documented deviation)
- Dock icon stays (no activation-policy change) → Task 1 makes no policy call. ✓
- Native notification, one per monitor, title `{name} · N new match(es)`, body top story title → Task 2. ✓
- Fired from Rust backend at the `new > 0` site → Task 2 Step 5e. ✓
- `TickOutcome.newest_title` additive field → Task 2 Step 1. ✓
- Startup notification permission request → Task 2 Step 5c. ✓
- `notification:default` capability + plugin dep + `tray-icon` feature → Tasks 1–2. ✓
- No frontend changes → confirmed; no `src/` file is touched. ✓
- Non-goals (settings toggle, menu-bar-only, badges, grouping, swarm) → none introduced. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"/"add tests" — every code and test step shows complete content. ✓

**Type consistency:** `format_notification(&str, i64, Option<&str>, &str) -> (String, String)` is defined in Step 5d and called identically in the tests (Step 3) and in `notify_new_matches` (Step 5d). `notify_new_matches` is called in Step 5e with `(&app, &monitor.name, new, newest_title.as_deref(), &monitor.prompt)` matching its signature (`new` is `i64`). `TickOutcome.newest_title: Option<String>` (Step 1) is `.clone()`d into the tuple and passed as `.as_deref()` → `Option<&str>`, matching the helper. ✓

**Deviation from spec (noted):** the tray uses the menu-on-click default rather than a separate left-click-to-show handler, avoiding the `show_menu_on_left_click`/`menu_on_left_click` method-rename risk across Tauri 2.x point releases while preserving one-click access to **Show HN Watch**.
