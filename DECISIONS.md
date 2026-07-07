# DECISIONS

An append-only log of the meaningful choices and their trade-offs. Newest first.
Each entry: what we chose, why, and what we gave up.

---

## D-008 · Feature-branch workflow, no tags / releases / CI

**Chose:** every unit of work on its own `feat/*` branch, merged into `main` with a proper
commit message. No version tags, no GitHub releases, no CI pipelines.
**Why:** keeps history clean and every change isolated and revertible, without ceremony that
doesn't earn its keep at this stage.
**Gave up:** automated build/verification on push — fine to add later if the project outlives
the weekend.

## D-007 · Public GitHub repo, no automated deploy

**Chose:** host on a public `github.com/sourabhbgp/hn-watch`, push branches, merge to `main`.
**Why:** a desktop app has no "web deploy"; the repo is the deliverable. Public so it can be shared.
**Gave up:** release artifacts (.dmg, etc.) — deferred; not needed to demonstrate the design.

## D-006 · Two living docs — STATUS.md + DECISIONS.md

**Chose:** `STATUS.md` (what's done / next / how to run) and `DECISIONS.md` (this file).
**Why:** clean separation — one answers "where are we," the other "why is it this way." Survives
across sessions and makes the work legible to a reviewer.
**Gave up:** a single mega-README; slightly more files to keep current.

## D-005 · Tailwind CSS v4

**Chose:** Tailwind v4 via the `@tailwindcss/vite` plugin, with design tokens in `@theme`.
**Why:** fast iteration on the feed/panel UI; the Vite plugin means almost no config. Tokens keep
the look aligned with `docs/architecture.html`.
**Gave up:** some risk of a generic utility-class look — mitigated by a small custom token palette
and component structure.

## D-004 · React 19 + TypeScript + Vite for the UI

**Chose:** React + TS + Vite (the Tauri `react-ts` template).
**Why:** most legible to reviewers, huge ecosystem, and reactive updates map naturally onto a live
feed that will later be driven by Rust events.
**Gave up:** a leaner bundle (Svelte/vanilla) — worth it for familiarity and velocity.

## D-003 · Build for macOS now; cross-platform by construction

**Chose:** develop and build on macOS; rely on Tauri's cross-platform APIs rather than writing
per-OS code.
**Why:** it's the dev machine and the likely review environment; Tauri already abstracts window,
tray, notifications, and fs across OSes. Shipping Windows/Linux is a CI concern, not a code one.
**Gave up:** verified Windows/Linux builds this weekend — out of scope by design.

## D-002 · One shared agent runtime for both rhythms _(design intent)_

**Chose:** monitors (one `claude -p` per tick) and the swarm (many at once) submit to a single
bounded runtime — a semaphore-guarded pool with a small lane reserved for the interactive swarm,
per-call timeout, retry, and backoff on rate limits.
**Why:** it's the crux of the challenge; one chokepoint gives bounded resource use, fairness, and a
place to prioritize the click over the tick. The scarce resource is the upstream Claude limit, not
local CPU.
**Gave up:** the simplicity of two independent code paths — but that would duplicate the hard parts.
_(Implemented in a later phase; recorded here so the intent is fixed.)_

## D-001 · Tauri over Electron

**Chose:** Tauri 2 (Rust core + the OS's built-in WebView).
**Why:** tiny bundle, native-fast Rust backend for the workers and process orchestration, and
first-class tray/notification support — exactly what a long-lived background app needs.
**Gave up:** Electron's single-language (JS) simplicity and its bundled-Chromium consistency across
OSes; acceptable given the backend is the interesting half.
