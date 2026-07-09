# Notification-Permission Banner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface the notification-denied state — when notifications are off, show a banner with an "Open Settings" deep-link that self-clears when the user re-enables them (TODO #5).

**Architecture:** Notification permission is a synchronous local OS query, so no `Mutex` state / preflight / Rust events (unlike Claude health). A `notification_health` command reads `permission_state()` live; the frontend re-reads it on mount + on window focus and renders a shared `Banner`. Reuses the Session-5 Claude-health banner pattern.

**Tech Stack:** Rust + Tauri 2 (`tauri-plugin-notification`, `tauri-plugin-opener`), React 19 + TypeScript, Tailwind v4.

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-07-09-notification-permission-banner-design.md`.
- **Design tokens only** — no new colors/fonts/spacing. Reuse `bg-hn-soft`, `border-hn-border`, `text-soft`, `bg-rust`, `bg-card`, `bg-paper`, `text-rust` (see `docs/design.md` / `src/index.css`).
- **DRY:** extract one shared `Banner`; both the Claude and notification banners render it.
- **Do not change notification delivery** — `scheduler.rs` `.show()` stays best-effort (`let _ = …`).
- **Statuses are exactly** `"granted" | "denied" | "default"`; only `"denied"` shows a banner.
- **Banner copy (verbatim, U+203A `›`):** `Notifications are off — enable them in System Settings › Notifications › hn-watch to get alerts when new matches land.`
- **Settings deep-link (verbatim):** `x-apple.systempreferences:com.apple.Notifications-Settings.extension`.
- **Testing:** verify in the **real native release build** (`tauri build`), never localhost; notification delivery via `screencapture -x` with the app **backgrounded** (computer-use screenshots black out the banner layer). See `docs/TESTING.md` + the `hn-watch-notification-verify-gotcha` memory.
- **Branch:** `feat/notification-permission-banner` (already created; spec committed). Push to origin, keep after merge.

---

### Task 1: Backend — `notification_health` command + pure mapping

**Files:**
- Modify: `src-tauri/src/commands.rs` (add DTO, pure fn, command, unit tests)
- Modify: `src-tauri/src/lib.rs:26-31` (capture `request_permission` result) and `:47-54` (register command)

**Interfaces:**
- Produces (consumed by Task 2 via the Tauri boundary):
  - Command `notification_health() -> NotificationHealthDto` where `NotificationHealthDto { status: String, message: String }` serializes camelCase to `{ status, message }`.
  - Pure `pub fn notification_health_dto(state: Option<PermissionState>) -> NotificationHealthDto`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src-tauri/src/commands.rs`:

```rust
    #[test]
    fn notification_health_maps_states() {
        use tauri_plugin_notification::PermissionState;
        let granted = notification_health_dto(Some(PermissionState::Granted));
        assert_eq!(granted.status, "granted");
        assert_eq!(granted.message, "");

        let denied = notification_health_dto(Some(PermissionState::Denied));
        assert_eq!(denied.status, "denied");
        assert!(denied.message.contains("System Settings"));
        assert!(denied.message.contains("hn-watch"));

        // Err from the OS / any not-yet-decided state is silent (no banner).
        let unknown = notification_health_dto(None);
        assert_eq!(unknown.status, "default");
        assert_eq!(unknown.message, "");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test notification_health_maps_states`
Expected: FAIL to compile — `cannot find function notification_health_dto`.

- [ ] **Step 3: Write the DTO + pure mapping + command**

In `src-tauri/src/commands.rs`, after the `ClaudeHealthDto` block (around line 44), add:

```rust
use tauri_plugin_notification::{NotificationExt, PermissionState};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationHealthDto {
    pub status: String,
    pub message: String,
}

/// Pure map from the OS permission state to the UI DTO. Only `Denied` shows a
/// banner; `Granted` and every not-yet-decided/unknown state (incl. an OS read
/// error → `None`) are silent. Mirrors the `ClaudeHealth::code()/message()` seam.
pub fn notification_health_dto(state: Option<PermissionState>) -> NotificationHealthDto {
    match state {
        Some(PermissionState::Granted) => {
            NotificationHealthDto { status: "granted".into(), message: String::new() }
        }
        Some(PermissionState::Denied) => NotificationHealthDto {
            status: "denied".into(),
            message: "Notifications are off — enable them in System Settings › Notifications › hn-watch to get alerts when new matches land.".into(),
        },
        _ => NotificationHealthDto { status: "default".into(), message: String::new() },
    }
}

/// Read the live OS notification permission (synchronous, no cached state).
#[tauri::command]
pub fn notification_health(app: AppHandle) -> NotificationHealthDto {
    notification_health_dto(app.notification().permission_state().ok())
}
```

Note: `use tauri::{AppHandle, ...}` already exists at the top of the file — reuse it.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test notification_health_maps_states`
Expected: PASS.

- [ ] **Step 5: Register the command + capture the startup request result**

In `src-tauri/src/lib.rs`, add to the `invoke_handler` list (after `commands::recheck_claude,` ~line 53):

```rust
            commands::notification_health,
```

And change the fire-and-forget startup request (lines 28-30) so a failure is no longer silently discarded:

```rust
                if !matches!(n.permission_state(), Ok(PermissionState::Granted)) {
                    if let Err(e) = n.request_permission() {
                        eprintln!("[hn-watch] notification permission request failed: {e}");
                    }
                }
```

- [ ] **Step 6: Verify the whole crate builds + full test suite green**

Run: `cd src-tauri && cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -15`
Expected: build succeeds with **zero warnings**; all prior tests + the new one PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(notif): notification_health command + live permission read

Synchronous notification_health command reads permission_state() live and
maps Granted/Denied/other to a {status,message} DTO via a pure, unit-tested
fn. Register it; capture (log) a failed startup permission request instead
of discarding it. Delivery in scheduler.rs is unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Frontend — shared `Banner`, notification banner, and wiring

**Files:**
- Create: `src/components/Banner.tsx` (shared presentational banner)
- Modify: `src/components/ClaudeBanner.tsx` (render `<Banner>`)
- Create: `src/components/NotificationBanner.tsx` (denied banner + Open Settings)
- Modify: `src/types.ts` (add `NotificationHealth`)
- Modify: `src/api.ts` (add `getNotificationHealth`)
- Modify: `src/App.tsx` (state, mount fetch, focus re-check, stacked render)
- Verify/Modify: `src-tauri/capabilities/default.json` (opener scope, only if the button errors)

**Interfaces:**
- Consumes: the `notification_health` command from Task 1 → `{ status: "granted"|"denied"|"default", message: string }`.
- Produces: `Banner` component (`{ message: string; action?: ReactNode }`); `getNotificationHealth(): Promise<NotificationHealth>`; `NotificationHealth` type.

- [ ] **Step 1: Add the shared `Banner` component**

Create `src/components/Banner.tsx` (markup lifted verbatim from the current `ClaudeBanner` so the visual is unchanged):

```tsx
import type { ReactNode } from "react";

/// Shared top banner: a rust status dot, a message, and an optional action slot.
/// Rendered by both ClaudeBanner and NotificationBanner (DRY — one visual, two sources).
export function Banner({ message, action }: { message: string; action?: ReactNode }) {
  return (
    <div className="flex items-center gap-3 border-b border-hn-border bg-hn-soft px-6 py-2.5">
      <span className="h-2 w-2 shrink-0 rounded-full bg-rust" />
      <p className="min-w-0 flex-1 text-[12.5px] leading-snug text-soft">{message}</p>
      {action}
    </div>
  );
}
```

- [ ] **Step 2: Refactor `ClaudeBanner` to render `<Banner>`**

Replace the entire body of `src/components/ClaudeBanner.tsx` with:

```tsx
import type { ClaudeHealth } from "../types";
import { Banner } from "./Banner";

export function ClaudeBanner({
  health,
  onRecheck,
  rechecking,
}: {
  health: ClaudeHealth;
  onRecheck: () => void;
  rechecking: boolean;
}) {
  if (health.status === "ok") return null;
  return (
    <Banner
      message={health.message}
      action={
        <button
          onClick={onRecheck}
          disabled={rechecking}
          className="shrink-0 rounded-md border border-hn-border bg-card px-3 py-1 text-[12px] font-semibold text-rust transition-colors hover:bg-paper disabled:opacity-50"
        >
          {rechecking ? "Checking…" : "Re-check"}
        </button>
      }
    />
  );
}
```

- [ ] **Step 3: Add the `NotificationHealth` type + API call**

In `src/types.ts`, after the `ClaudeHealth` interface (line 9), add:

```ts
export interface NotificationHealth {
  status: "granted" | "denied" | "default";
  message: string;
}
```

In `src/api.ts`, add the import and the call. Update line 3's type import to include `NotificationHealth`:

```ts
import type { Monitor, FeedItem, ClaudeHealth, NotificationHealth } from "./types";
```

and append at the end of the file:

```ts
// Live OS notification permission (drives the notification banner). Re-invoked
// on window focus so re-enabling in System Settings clears the banner.
export const getNotificationHealth = () =>
  invoke<NotificationHealth>("notification_health");
```

- [ ] **Step 4: Add the `NotificationBanner` component**

Create `src/components/NotificationBanner.tsx`:

```tsx
import type { NotificationHealth } from "../types";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Banner } from "./Banner";

const SETTINGS_URL =
  "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

export function NotificationBanner({ health }: { health: NotificationHealth }) {
  if (health.status !== "denied") return null;
  return (
    <Banner
      message={health.message}
      action={
        <button
          onClick={() => {
            openUrl(SETTINGS_URL);
          }}
          className="shrink-0 rounded-md border border-hn-border bg-card px-3 py-1 text-[12px] font-semibold text-rust transition-colors hover:bg-paper"
        >
          Open Settings
        </button>
      }
    />
  );
}
```

- [ ] **Step 5: Wire it into `App.tsx`**

In `src/App.tsx`:

(a) Extend the type import (line 2) and component imports (lines 5-7 area) and API imports (lines 8-19):

```ts
import type { ClaudeHealth, FeedItem, Monitor, NotificationHealth } from "./types";
```
```ts
import { NotificationBanner } from "./components/NotificationBanner";
import { getCurrentWindow } from "@tauri-apps/api/window";
```
Add `getNotificationHealth` to the existing `from "./api"` import list.

(b) Add state next to `health` (after line 28):

```ts
  const [notifHealth, setNotifHealth] = useState<NotificationHealth>({
    status: "granted",
    message: "",
  });
```

(c) Inside the mount `useEffect` (after the `getClaudeHealth().then(setHealth);` line ~51), add the fetch + focus re-check, and register cleanup:

```ts
    const refetchNotif = () => getNotificationHealth().then(setNotifHealth);
    refetchNotif();
    const uFocus = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) refetchNotif();
    });
```
and in the cleanup `return () => { … }` block, add:
```ts
      uFocus.then((f) => f());
```

(d) Render the notification banner directly under the Claude banner (line 99):

```tsx
      <ClaudeBanner health={health} onRecheck={handleRecheck} rechecking={rechecking} />
      <NotificationBanner health={notifHealth} />
```

- [ ] **Step 6: Typecheck + build the frontend**

Run: `npm run build 2>&1 | tail -20` (runs `tsc` then `vite build`)
Expected: no type errors, build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/components/Banner.tsx src/components/ClaudeBanner.tsx src/components/NotificationBanner.tsx src/types.ts src/api.ts src/App.tsx
git commit -m "feat(notif): denied banner + Open Settings, self-clearing on focus

Extract a shared Banner (DRY); ClaudeBanner and the new NotificationBanner
both render it. App reads notification_health on mount and re-reads on window
focus, so re-enabling in System Settings clears the banner. Open Settings
deep-links to the macOS Notifications pane via the opener plugin.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: End-to-end verification on the release build (all corner cases)

No code changes expected. Two possible follow-up edits, each with its trigger below. Produce documented evidence for every corner case — this is the deliverable the user explicitly asked for.

**Files (only if a trigger fires):**
- Modify: `src-tauri/src/commands.rs` (banner copy) — only if the recovery experiment shows per-process caching.
- Modify: `src-tauri/capabilities/default.json` (opener scope) — only if the Open Settings button errors.

- [ ] **Step 1: Build the release app**

Run: `npm run tauri build 2>&1 | tail -20`
Expected: `.app` bundle built. Launch it from the built bundle (not `tauri dev`) — permission is keyed to the bundle ID and dev-mode can differ (Session 8).

- [ ] **Step 2: Load-bearing experiment — recovery without restart (run this FIRST)**

The whole recovery story assumes `permission_state()` reflects a System-Settings toggle within the running process. Prove it before trusting the other cases:
1. In System Settings › Notifications › hn-watch, set notifications **OFF** (Denied). Launch the app.
2. Confirm the **notification banner shows** with the exact copy.
3. Leave the app running. In System Settings, turn hn-watch notifications **ON**.
4. Click back to the HN Watch window (focus). 
5. Observe: the banner **clears without a restart**.

- **Clears →** design stands. Continue.
- **Does not clear →** `permission_state()` is cached per-process. Edit the Denied `message` in `commands.rs` to end with `… — then restart HN Watch.` instead of relying on auto-clear, rebuild, re-run this step, then re-commit with `git commit -m "fix(notif): recovery copy — macOS caches permission per-process"`. The focus re-check still helps after a restart.

- [ ] **Step 3: Open Settings button works (opener scope)**

With the app open and the banner showing (Denied), click **Open Settings**.
- **The macOS Notifications settings pane opens →** good.
- **Nothing happens / console shows a `opener.open_url not allowed` (scope) error →** add to `src-tauri/capabilities/default.json` `permissions` array, replacing the bare `"opener:default"` with an explicit url scope:
  ```json
    {
      "identifier": "opener:allow-open-url",
      "allow": [{ "url": "x-apple.systempreferences:*" }]
    }
  ```
  Keep `"opener:default"` as well. Rebuild, re-test, commit `git commit -m "fix(notif): allow x-apple.systempreferences deep-link scope"`.

- [ ] **Step 4: Corner cases — capture evidence for each**

Run through all six, backgrounding the app + using `screencapture -x <path>.png` for any banner-delivery check:

1. **Granted** — turn notifications ON, launch. No banner. Create/trigger a monitor that lands a match (or use an existing one); background the app; confirm a real notification banner delivers (`screencapture -x`). ✅ = no in-app banner AND a delivered OS notification.
2. **Denied** — notifications OFF, launch. In-app banner shows with correct copy. ✅
3. **NotDetermined** — on a machine/user where the app has never asked (or reset via `tccutil reset ... <bundle-id>` if available), first launch shows the OS prompt and **no in-app banner flashes** while the prompt is up. If a clean NotDetermined state can't be produced, note that and rely on the Task-1 unit test (`None → default`, silent). ✅
4. **Recovery** — covered by Step 2. ✅ (banner clears + a subsequent match delivers a notification again).
5. **Both down** — force Claude down (`HN_WATCH_CLAUDE_BIN` → a fake missing binary, per Session 5) AND notifications OFF → **both banners stack** cleanly (Claude on top, notification below), layout intact, no overlap/truncation. ✅
6. **Release build** — all of the above were on the built `.app` (Step 1), not dev. ✅

- [ ] **Step 5: Clean up any test monitors**

If verification created throwaway monitors, back up the DB and delete them (per Session 8's cleanup), leaving the real monitors untouched.

---

### Task 4: Docs + finish the branch

**Files:**
- Modify: `STATUS.md` (add a Session 9 entry)
- Modify: `docs/TODO.md` (mark #5 ✅ SHIPPED)

- [ ] **Step 1: Update `STATUS.md`**

Add a `## Session 9 — Notification-denied banner (TODO #5)` entry summarizing: the synchronous `notification_health` command + pure mapping + test, the shared `Banner` refactor, the denied banner with Open Settings + focus self-clear, the recovery-experiment outcome (no-restart vs. restart-copy), the opener-scope outcome, and the six corner cases verified live on the release build.

- [ ] **Step 2: Mark TODO #5 shipped**

In `docs/TODO.md`, change the `## 5.` heading from `🆕 NOT STARTED` to `✅ SHIPPED (Session 9)` and add a one-paragraph history blockquote (like #1/#2/#3) noting the branch, the synchronous-read design, and the recovery outcome. Update the closing "Order to tackle" line's open-backlog note.

- [ ] **Step 3: Commit docs**

```bash
git add STATUS.md docs/TODO.md
git commit -m "docs: log Session 9 (notification-denied banner) + mark TODO #5 shipped

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: Whole-branch review, then push + merge**

Per project workflow: request a whole-branch code review (opus), address findings, then push the branch to origin and merge into `main` with `--no-ff`, keeping the branch on origin.

```bash
git push -u origin feat/notification-permission-banner
git checkout main && git merge --no-ff feat/notification-permission-banner && git push
```

---

## Self-Review

**Spec coverage:**
- `notification_health` command + live read → Task 1. ✅
- Pure unit-tested mapping (`granted`/`denied`/`default`) → Task 1 Steps 1-4. ✅
- Capture startup `request_permission` result → Task 1 Step 5. ✅
- Shared `Banner` (DRY) + `ClaudeBanner` refactor → Task 2 Steps 1-2. ✅
- `NotificationBanner` denied-only + Open Settings deep-link → Task 2 Step 4. ✅
- `types.ts` / `api.ts` additions → Task 2 Step 3. ✅
- App mount fetch + window-focus re-check + stacked render → Task 2 Step 5. ✅
- Opener capability verification/scope → Task 3 Step 3. ✅
- Load-bearing recovery experiment (no-restart) → Task 3 Step 2. ✅
- All six corner cases on the release build → Task 3 Step 4. ✅
- Non-goal (delivery unchanged) → honored; no task touches `.show()`. ✅

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows complete code; the two conditional edits in Task 3 have explicit triggers + exact diffs. ✅

**Type consistency:** `NotificationHealthDto {status,message}` (Rust, camelCase) ↔ `NotificationHealth {status,message}` (TS); statuses `"granted"|"denied"|"default"` identical in the Rust mapping, the TS type, and the banner's `!== "denied"` guard; `notification_health_dto(Option<PermissionState>)` signature matches its test and command call site; `getNotificationHealth` name consistent across `api.ts` and `App.tsx`. ✅
