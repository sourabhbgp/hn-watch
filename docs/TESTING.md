# Testing the real desktop app

**Rule:** verify features against the **real native app window**, not a browser at
`localhost:1420`. A localhost page is the web UI only — it can behave differently from
the packaged Tauri app (native APIs, Rust commands, IPC, permissions, window chrome).
"It works in the browser" is not evidence the app works.

## Two ways to run the real app

| Mode | Command | What it is |
|------|---------|------------|
| Dev  | `npm run tauri dev`   | Real native window, hot-reload, frontend served from the Vite dev server. Fast loop for UI + wiring. |
| Build| `npm run tauri build` | Production bundle → `src-tauri/target/release/bundle/macos/hn-watch.app`. The true "real app". |

Both open a **native WKWebView window** driven by the Rust core — both are valid "real app"
targets. Use dev for fast iteration; do a **build** run to confirm anything that could differ
in packaging (bundled assets, signing, native permissions) before calling a feature done.

## Driving the app (computer-use)

The app is a native window, so it is tested with the `computer-use` MCP, not the browser MCP.

1. **Grant access (once per session):** `request_access(["hn-watch"])`
   → resolves to bundle id `com.sourabh.hnwatch`, **full tier** (click + type allowed).
   This is per-session — a new session needs one fresh approval from the user.
2. **Launch:** `open_application("com.sourabh.hnwatch")`
   → launches `src-tauri/target/release/bundle/macos/hn-watch.app` (the in-repo build,
   so it always reflects the latest `tauri build`).
3. **Observe:** `screenshot` (screen-recording permission verified working).
4. **Drive:** `left_click` / `type` / `key` on the native window (accessibility control verified).

## Verified working (Session 1, 2026-07-08)

- Native control granted, full tier, native screenshot filtering.
- Real built bundle launches and renders the feed.
- Click on a sidebar monitor filtered the feed (All → "AI-agent startups · 2 matches") — input control confirmed.
- Toolchain present: `cargo`, `rustc`, and `claude` all on PATH (backend agents from Phase 3 use `claude`).

## Backend / feature test loop (Phase 2+)

For Rust commands, persistence, ingestion, agent runs:
1. Implement the Rust command + frontend wiring.
2. `npm run tauri dev` (or `build` for a packaging check).
3. `open_application` → screenshot → drive the flow → screenshot the result.
4. For persistence: quit the app, relaunch, confirm state survived.
5. For anything using `claude -p`: confirm `claude` is on PATH inside the app's env, not just the shell.
