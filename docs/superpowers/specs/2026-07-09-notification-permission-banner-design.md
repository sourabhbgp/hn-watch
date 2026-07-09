# Surface the notification-denied state — design

**Ticket:** `docs/TODO.md` #5. **Date:** 2026-07-09. **Status:** ⛔ **DESCOPED** — not
deliverable on desktop with the current stack (see Outcome). The design below is kept for a
future native implementation.

## Outcome (2026-07-09) — descoped after live E2E

Built the backend command + banner + wiring (all reviewed clean), then verified on the **release
build**: with macOS notifications truly **off**, the banner **never appeared** — cold or warm.

**Root cause (primary-source confirmed):** `tauri-plugin-notification` **2.3.3 (the latest published
version)** hardcodes the desktop permission API — `desktop.rs` returns `Ok(PermissionState::Granted)`
from **both** `permission_state()` and `request_permission()`, never querying the OS. So
`notification_health` always reads `granted` and the `denied` banner can never fire. This is not our
code — the mapping/banner/wiring were correct; they were fed a constant. The official Tauri docs are
**silent** on desktop permission behavior (neither confirm nor deny), which is why the false premise
survived design + code review; only the live E2E caught it.

**No off-the-shelf fix:** we are already on the latest official plugin, and the popular fork
`Choochmeque/tauri-plugin-notifications` (v0.4.6) **stubs desktop the same way**. Detecting a
notification-**denied** state on desktop macOS requires calling the native
`UNUserNotificationCenter.getNotificationSettings` API directly via `objc2` bindings — the standard
native-app pattern, but risky in an ad-hoc-signed build (the query subsystem may disagree with
`notify_rust`'s delivery subsystem) and it is exactly the kind of incidental plumbing the weekend
brief allows stubbing. **Decision:** descope; document; redirect effort to the dig-deeper swarm (a
core, still-unbuilt requirement). The feature code remains recoverable in git history (commits
`7e9061a`, `6227ffc`, `8d8f2d5`, reverted in `13e59a1`).

**Note — first-time prompt still works:** the app still gets its one macOS permission prompt for
free, triggered by the OS on the **first delivered notification** (via `notify_rust`), not by our
code. The dead startup `request_permission()` guard was removed (`72a5825`). Only *denial detection*
is missing.

---

_Original design (pre-implementation) follows._

## Problem

Notification permission is requested once at startup (`lib.rs` `setup` → `request_permission()`,
result **discarded**). If the user denies it — or later turns notifications off in System Settings —
every `.show()` in `scheduler.rs` fails **silently** (`let _ = …`). macOS never re-shows the prompt
after a denial, so the core "fire a native notification when new items land" requirement quietly
stops working with **zero signal** to the user. On a fresh install / different machine, a denied
prompt yields a watchtower that never taps you on the shoulder and never says why.

## Goal

Make the off-state **visible and recoverable**, reusing the Session-5 Claude-health banner pattern.
Do **not** change notification delivery — it stays best-effort.

## Core insight (why this is small)

Notification permission is a **synchronous, local OS query**, not an async subprocess probe like
Claude health. It therefore needs **none** of Claude's machinery — no `Arc<Mutex<…>>` state, no
background preflight task, no Rust-emitted `*-health` events. The only thing that ever changes the
permission is the user visiting System Settings, and **window focus is the exact moment to re-read
it**. The whole feature is: *read live state on mount + on window focus → show a banner when denied →
deep-link to Settings.*

## Backend (Rust)

### New command — `notification_health`
In `commands.rs`, add:

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationHealthDto { pub status: String, pub message: String }

#[tauri::command]
pub fn notification_health(app: AppHandle) -> NotificationHealthDto { … }
```

Reads `app.notification().permission_state()` **live on every call** (no cached state). The
`(status, message)` mapping lives in a **pure, unit-tested** free function
`notification_health_dto(state: PermissionState) -> NotificationHealthDto`, mirroring the existing
`ClaudeHealth::code()/message()` seam:

| `PermissionState` | `status`  | `message`                                                                                                  | banner? |
| ----------------- | --------- | ---------------------------------------------------------------------------------------------------------- | ------- |
| `Granted`         | `granted` | `""`                                                                                                       | no      |
| `Denied`          | `denied`  | `Notifications are off — enable them in System Settings › Notifications › hn-watch to get alerts when new matches land.` | yes     |
| anything else¹    | `default` | `""`                                                                                                       | no      |

¹ Covers `Prompt`/not-yet-answered/unknown variants. We deliberately do **not** nag in this state —
the OS prompt may still be up, and there is nothing actionable to say yet.

Register `notification_health` in the `invoke_handler` list in `lib.rs`.

### Startup: capture the request result
In `lib.rs` `setup`, the current `let _ = n.request_permission();` **discards** the outcome. Bind it
instead so first-run state is definite (and log on error). No behavior change beyond not throwing the
result away; the frontend still reads the authoritative state via `notification_health`.

### Non-goals (backend)
- No shared `Mutex` notification state, no preflight, no Rust-side events, no `recheck` command —
  the frontend simply re-invokes `notification_health`.
- `scheduler.rs` `.show()` stays best-effort (`let _ = …`) — unchanged.

## Frontend

### DRY: extract a shared `Banner`
The current `ClaudeBanner` hardcodes the visual (rust dot + message + action button). Extract a
presentational **`Banner`** component — props: `message: string` and an **action slot** (children /
`action?: ReactNode`) — carrying the existing markup and tokens (`bg-hn-soft`, `border-hn-border`,
`text-soft`, the `bg-rust` dot, the `border-hn-border`/`bg-card` button style). `ClaudeBanner` keeps
its Re-check button and now renders `<Banner>`; the new notification banner renders `<Banner>` too.
One visual, two data sources. **Existing design tokens only — no new colors.**

### `NotificationBanner`
Renders **only when `status === "denied"`**. Action button **"Open Settings"** →
`openUrl("x-apple.systempreferences:com.apple.Notifications-Settings.extension")` from
`@tauri-apps/plugin-opener`. No Re-check button — focus-recheck (below) clears it automatically.

### `App.tsx`
- Add `notifHealth` state; fetch via `getNotificationHealth()` on mount.
- Add a **window-focus listener** — Tauri `getCurrentWindow().onFocusChanged(({payload:focused}) => focused && refetch)` (fall back to the DOM `window` `focus` event if simpler) — that re-fetches notification health so the banner **self-clears** when the user returns after enabling it.
- Render banners **stacked**, Claude first then notification, above the existing layout.

### `types.ts` / `api.ts`
- `types.ts`: `export interface NotificationHealth { status: "granted" | "denied" | "default"; message: string }`.
- `api.ts`: `export const getNotificationHealth = () => invoke<NotificationHealth>("notification_health");`.

## Plumbing to verify (not architecture-critical)

The `x-apple.systempreferences:` deep-link rides on the `opener:default` capability
(`src-tauri/capabilities/default.json`). If that permission's scope is http/https/file-only, add an
`opener:allow-open-url` scope entry for the scheme. If it cannot be made to work, the banner still
functions with its text guidance and the **button is dropped** — the feature does not depend on it.

## Load-bearing assumption (verify empirically, early)

Recovery-without-restart assumes `permission_state()` observes a System-Settings toggle **within the
already-running process**. This is **not** analogous to `recheck_claude` (which spawns a fresh
`claude`); it is a status read, and macOS notification-auth status is sometimes cached per-process.
**5-minute experiment before finalizing banner copy:** launch (release build) → deny → confirm banner
→ flip ON in System Settings → return focus → confirm it flips to no-banner **without relaunching**.
- Yes → design stands as written.
- No → recovery copy changes to "enable in Settings, then restart HN Watch," and focus-recheck still
  helps after the restart. This changes only banner copy + one acceptance line, not the architecture.

## Corner cases — all to be proven in the E2E run

Verified on the **release build** (`tauri build`), since notification permission is keyed to the
bundle ID and dev-mode can differ (per Session 8). Delivery checks use `screencapture -x` with the
app **backgrounded** — computer-use screenshots black out the NotificationCenter banner layer and
macOS suppresses banners while the app is frontmost (recorded `hn-watch-notification-verify-gotcha`).

1. **Granted** → no banner **and** a real notification delivers.
2. **Denied** → banner shows with the correct copy.
3. **NotDetermined** (first launch, prompt unanswered) → no banner flashes while the OS prompt is up.
4. **Denied → enable in Settings → focus-recheck → banner clears + delivery resumes** — the
   load-bearing case; confirm no restart needed.
5. **Claude down + notifications off** → both banners stack cleanly.
6. Re-confirm on the release build (bundle-ID-keyed permission).

## Acceptance

On a machine where notification permission is denied/off, the app shows a clear
"notifications are off + how to enable" banner with an **Open Settings** button, instead of silently
never notifying. Enabling it in System Settings and returning focus to the app clears the banner
(without a restart, pending the experiment above). When permission is granted there is no banner and
notifications deliver as today.

## Reuses

Session-5 banner pattern (`feat/error-handling-preflight`), `tauri-plugin-opener` (already
registered), existing design tokens, the pure-mapping + unit-test seam used by `ClaudeHealth`.
