# Tray + native notifications (Phase 3) — design

**Date:** 2026-07-09
**Ticket:** Phase 3 in [`STATUS.md`](../../../STATUS.md) ("Next — Tray + native notifications")
**Branch:** `feat/tray-notifications`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

Two must-have lines of the verbatim brief are still entirely unbuilt:

> …the app should keep running in the system tray with the window closed and fire a native
> notification when new items land.

Today, closing the window **quits the app** (default Tauri behavior), so every monitor worker
dies — the watchtower only watches while you stare at it. And when a tick lands new matches the
UI feed updates silently; nothing tells you unless the window is open and focused. A watchtower
you must keep on-screen is not a watchtower.

## Goal (acceptance)

- Closing the window (red button / Cmd-W) **hides** it; the app keeps running, workers keep
  ticking, and a **tray (menu-bar) icon** stays present with **Show HN Watch** / **Quit HN Watch**.
- The tray **Show** item (and a click on the tray icon) restores + focuses the window; **Quit**
  actually exits.
- When a monitor's tick lands **≥1 new match**, a **native OS notification** fires, titled with
  the monitor name and the new-match count, body = the top matched story's title.
- Nothing regresses: monitors + feed still persist across a real quit/relaunch; the tick loop,
  dedup, watermark ingestion, and Claude-health banner are untouched.

## Decisions (locked in brainstorming)

- **One notification per monitor** that landed new matches (not one aggregate across monitors) —
  each names its monitor so you know *what* fired.
- **Closing hides to tray, Dock icon stays** (Spotify-style). Not menu-bar-only; no macOS
  activation-policy change. Quit lives only in the tray menu.
- **Notifications fire from the Rust backend**, in the scheduler, where `new > 0` is already
  known — not from the frontend. This is the robust choice: it fires even when the window is
  hidden and the webview is idle, and it matches the brief's "long-lived background worker in the
  Rust layer." (A frontend-fired notification would depend on the hidden webview still running.)

## Architecture

Three small, independent pieces. None touch the tick/ingestion/agent logic beyond a one-field
additive change to `TickOutcome`.

### 1. Tray icon + menu — new `src-tauri/src/tray.rs`

A dedicated module keeps `lib.rs` thin (it already only wires setup + handlers). Public surface:

```rust
pub fn build(app: &AppHandle) -> tauri::Result<()>
```

- Builds a `Menu` with two `MenuItem`s: `show` ("Show HN Watch"), `quit` ("Quit HN Watch").
- Builds a `TrayIconBuilder` using the app's existing icon (`app.default_window_icon().cloned()`),
  attaches the menu, and wires:
  - `on_menu_event`: `show` → `show_main_window(app)`; `quit` → `app.exit(0)`.
  - `on_tray_icon_event`: a left click also calls `show_main_window(app)` (one-click restore, in
    addition to the menu that opens on click/right-click).
- `show_main_window(app)` helper: get the `"main"` webview window → `.show()`, `.unminimize()`,
  `.set_focus()` (best-effort; ignore errors). Reused by the tray and, later, by notification
  click if it proves deliverable.

Called once from `lib.rs` `setup`.

### 2. Close-to-tray — `lib.rs` setup

On the main window, register an `on_window_event` handler:

```rust
WindowEvent::CloseRequested { api, .. } => { api.prevent_close(); let _ = window.hide(); }
```

Workers keep running; the Dock icon stays (no activation-policy call). The **only** exit path is
tray → Quit (`app.exit(0)`), which bypasses `CloseRequested` and terminates normally.

### 3. Native notifications — `scheduler.rs` (+ one field in `tick.rs`)

At the existing `if new > 0 { emit("feed-updated") }` site, also fire a notification via the
plugin's **Rust** API:

```rust
app.notification().builder()
   .title(format!("{} · {} new match{}", monitor.name, new, if new == 1 {""} else {"es"}))
   .body(body)   // top matched story title, fallback to the monitor prompt
   .show();
```

`body` comes from a new field on `TickOutcome`:

```rust
pub struct TickOutcome { …existing…, pub newest_title: Option<String> }
```

Set in `run_tick` as `rows.first().map(|r| r.title.clone())` (the top match of the tick). The
all-seen early-return path returns `newest_title: None` (it has `new: 0`, so it never notifies).
The scheduler builds `body` as `outcome.newest_title.unwrap_or_else(|| monitor.prompt.clone())`,
and appends `" +N more"` when `new > 1`.

**Startup permission:** in `setup`, request notification authorization once
(`app.notification().request_permission()` if `permission_state()` isn't `Granted`) so the first
real notification on macOS isn't dropped before the user has granted the OS prompt. Best-effort;
errors are logged, not fatal.

## Notification click behavior (honest scope)

Clicking a macOS notification **activates the app** (brings it to the front). Reliably
un-hiding a *hidden* window from a notification click is **not well-supported** by
`tauri-plugin-notification` on desktop. So click-to-restore is **best-effort only**; the
**guaranteed** restore path is the tray **Show** item / tray-icon click. We will not ship a
click handler we can't verify actually fires on macOS. If, during implementation, the plugin
exposes a reliable desktop action callback, we route it through the same `show_main_window`
helper — otherwise we leave it out and document the tray as the restore path.

## Dependencies & config

- `Cargo.toml`: add `tauri-plugin-notification = "2"`; enable the **`tray-icon`** feature on the
  `tauri` dependency (`features = ["tray-icon"]`).
- `lib.rs`: `.plugin(tauri_plugin_notification::init())`.
- `capabilities/default.json`: add `"notification:default"`. Tray, window hide/show, and the
  notification are all driven from **Rust**, so no extra window/IPC capability permissions are
  needed.
- **No frontend changes.** `src/` is untouched — no new JS plugin, no new event listeners.

## Non-goals (YAGNI / out of scope)

- No settings/toggle to enable/disable notifications, no per-monitor mute, no notification
  history — the brief asks only to *fire* one when items land.
- No menu-bar-only mode / Dock-icon hiding.
- No "unread count" badge on the tray or Dock.
- No notification grouping/coalescing across monitors (decided: one per monitor).
- The dig-deeper swarm remains out of scope (its own future session).

## Testing (real native window — `docs/TESTING.md`)

Build the release `.app`, drive with computer-use:

1. **Close-to-tray:** launch → close the window → app is still alive (tray icon present in the
   menu bar), and a monitor tick still runs (feed grows / `tick-finished`). Tray **Show** →
   window returns focused. Tray **Quit** → process exits.
2. **Notification:** create/keep a monitor whose prompt matches current HN; on a tick that lands
   ≥1 new match, a native notification appears titled `"{name} · N new match(es)"` with a story
   title body. (If HN is quiet, force it with a broad prompt so a match is near-certain.)
3. **Regression:** real Quit → relaunch → monitors + feed persist; Claude-health banner and the
   countdown/observability UI still behave.

Plus `cargo test` (existing suite stays green; `TickOutcome`'s new field is additive) and a clean
`cargo build` / `tsc` / `vite build`.

## Files touched

| File | Change |
| --- | --- |
| `src-tauri/Cargo.toml` | + `tauri-plugin-notification`; `tauri` `tray-icon` feature |
| `src-tauri/capabilities/default.json` | + `notification:default` |
| `src-tauri/src/tray.rs` | **new** — tray build + menu/click handlers + `show_main_window` |
| `src-tauri/src/lib.rs` | register plugin, build tray, close-to-tray handler, permission request |
| `src-tauri/src/scheduler.rs` | fire notification when `new > 0` |
| `src-tauri/src/tick.rs` | `TickOutcome.newest_title` (additive) |
