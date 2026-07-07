# HN Watch

A native desktop **watchtower for Hacker News**, powered by local Claude agents.

You describe what you care about in plain English (e.g. _"AI-agent startup launches"_) and
set a cadence. A background worker per **monitor** polls Hacker News, asks `claude -p` to judge
and summarize what's relevant, and streams matches into one Twitter-style feed. Everything is
deduplicated, saved locally, and survives restarts — the app lives in the system tray and fires
a native notification when new items land. Any feed item can launch a **research swarm**: several
`claude -p` agents investigating in parallel, streaming live, then compiled into one brief.

> **Status:** early scaffolding. This is being built in phases across multiple sessions.
> See [`STATUS.md`](./STATUS.md) for exactly what works today and what's next, and
> [`DECISIONS.md`](./DECISIONS.md) for the architectural choices and trade-offs.

---

## The interesting part

Scheduled monitors and the on-demand swarm are the **same primitive** — a `claude -p` call —
driven at opposite tempos: a **trickle** (one call per tick, runs forever) versus a **burst**
(many calls the instant you click). Both go through a single bounded **agent runtime**, so the
design question the app is really about is: _how do you handle one-call-per-tick and
many-calls-at-once through the same runtime without melting the machine or the rate limit?_

A visual walkthrough lives in [`docs/architecture.html`](./docs/architecture.html) — open it in a browser.

## Stack

| Layer      | Choice                                        |
| ---------- | --------------------------------------------- |
| Shell      | [Tauri 2](https://tauri.app) (Rust core + OS WebView) |
| UI         | React 19 + TypeScript + Vite                  |
| Styling    | Tailwind CSS v4                               |
| Agent runtime | `claude -p` (Claude Code, headless) — spawned as child processes |
| Storage    | SQLite (local, restart-safe) — _coming in a later phase_ |

Cross-platform by construction (one codebase → macOS / Windows / Linux). Development and
builds target **macOS** for now, since that's the dev machine; shipping the others is just
adding CI runners, no code changes.

## Prerequisites

- **Node.js** 20+ and **npm**
- **Rust** (stable) + Cargo — [rustup.rs](https://rustup.rs)
- **Claude Code** installed and on your `PATH` (`claude --version`) — the app shells out to `claude -p`

## Run it

```bash
npm install
npm run tauri dev
```

This opens the native HN Watch window. To produce a bundle: `npm run tauri build`.

## Project structure

```
hn-watch/
├─ src/               # React UI (the WebView)
│  ├─ components/     # Sidebar, Feed, FeedCard, DigDeeperPanel …
│  ├─ mock/          # static sample data (until the backend is wired)
│  └─ types.ts       # shared UI types
├─ src-tauri/         # Rust core — window, tray, workers, agent runtime (grows by phase)
├─ docs/
│  └─ architecture.html   # the visual system design
├─ STATUS.md          # what's done / next / how to run
├─ DECISIONS.md       # architectural choices + trade-offs log
└─ README.md
```

## Roadmap (phased)

1. **Scaffold + static UI shell** ← _we are here_
2. Monitors CRUD + local persistence (SQLite)
3. HN ingestion + the agent runtime + one real monitor tick end-to-end
4. Background workers, dedup, tray, native notifications
5. Dig-deeper research swarm (parallel `claude -p`, live streaming → compiled brief)
6. Polish, error handling, README design write-up

Each phase is a feature branch merged into `main`.
