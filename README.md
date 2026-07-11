# HN Watch

A native desktop **watchtower for Hacker News**, powered by local Claude agents. **macOS.**

You describe what you care about in plain English (e.g. _"AI-agent startup launches"_) and set a
cadence. A background worker per **monitor** polls Hacker News, asks `claude -p` to judge and
summarize what's relevant, and streams matches into one feed — deduplicated, saved locally, and
restart-safe. The app lives in the system tray and fires a native notification when new items land.
Any feed item can launch a **research swarm**: several `claude -p` agents investigating in parallel,
streaming live, then compiled into one brief.

![status: feature-complete against the brief](https://img.shields.io/badge/status-feature--complete-1f6feb)

## Stack

| Layer         | Choice                                                              |
| ------------- | ------------------------------------------------------------------ |
| Shell         | [Tauri 2](https://tauri.app) (Rust core + OS WebView)             |
| UI            | React 19 + TypeScript + Vite                                       |
| Styling       | Tailwind CSS v4                                                    |
| Agent runtime | `claude -p` (Claude Code, headless) — spawned as child processes  |
| Data          | HN via the [Algolia HN Search API](https://hn.algolia.com/api)    |
| Storage       | SQLite (`rusqlite`, bundled) — local, restart-safe                 |

## Prerequisites

- **Node.js** 20+ and **npm**
- **Rust** (stable) + Cargo — [rustup.rs](https://rustup.rs)
- **Claude Code** installed, on your `PATH`, and logged in (`claude --version`) — the app shells
  out to `claude -p`. If it's missing or logged out, the app still opens and tells you so with a
  banner + a **Re-check** button; monitors show as `Paused` until it's available.

## Run it

```bash
npm install
npm run tauri dev      # opens the native HN Watch window
```

To produce a bundle: `npm run tauri build`.

> **Test in the real native window, not a browser at `localhost`.** The tray, notifications,
> `claude` subprocesses, and SQLite storage only exist in the Tauri shell. See
> [`docs/TESTING.md`](./docs/TESTING.md).

**Platform:** built, run, and tested on **macOS** only. It's a Tauri app, so a Windows or Linux
build may well be possible — but that's unverified, and the tray/notification paths are
macOS-specific in practice.

## How it works

Scheduled monitors and the on-demand swarm are the **same primitive** — a `claude -p` call —
driven at opposite tempos: a **trickle** (one call per tick, forever) versus a **burst** (many
calls the instant you click). They share one agent runtime but draw from **two reserved
concurrency pools**, so an interactive swarm never queues behind background ticks and a long swarm
never blocks a scheduled tick.

- **Visual system design** → [`docs/architecture.html`](./docs/architecture.html) (open in a browser)
- **Per-feature rationale, trade-offs, and what was deliberately left out** → [`STATUS.md`](./STATUS.md)
- **The verbatim assignment brief** → [`docs/REQUIREMENTS.md`](./docs/REQUIREMENTS.md)

## Project structure

```
hn-watch/
├─ src/               # React UI (the WebView)
├─ src-tauri/         # Rust core — db, hn, agent, tick, scheduler, swarm, tray, commands
├─ docs/              # REQUIREMENTS · design · architecture.html · TESTING
├─ STATUS.md          # per-session build log — what's done, how, and why
└─ README.md
```

## Tests

```bash
cd src-tauri && cargo test     # Rust core (parsers, DB, health/state machines, concurrency)
npm run build                  # tsc typecheck + Vite production build
```

The Rust suite covers the pure logic — verdict/brief/stream parsing, the DB layer and migrations,
the Claude-health state machine, and the two-pool concurrency invariant. UI flows are verified live
in the native window (see [`docs/TESTING.md`](./docs/TESTING.md)).
