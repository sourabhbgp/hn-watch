# HN Watch

A native desktop **watchtower for Hacker News**, powered by local Claude agents. **macOS.**

Tell it what you care about in plain English, and a background agent watches Hacker News for you -
judging, summarizing, and streaming matches into one feed, with a native notification when
something lands. Any story can spin up a **research swarm** that investigates it from several angles
and hands you one combined brief.

![status: feature-complete against the brief](https://img.shields.io/badge/status-feature--complete-1f6feb)

## Requirements

- **macOS** (only platform tested - see [Platform](#platform))
- **Node.js** 20+ and **npm**
- **Rust** (stable) + Cargo - [rustup.rs](https://rustup.rs)
- **Claude Code** installed, on your `PATH`, and logged in - check with `claude --version`. HN Watch
  shells out to `claude -p` for everything. If it's missing or logged out, the app still opens and
  tells you so with a banner + a **Re-check** button; monitors stay `Paused` until it's available.

## Get it running

HN Watch runs from source. Once the [requirements](#requirements) above are installed, open a
terminal (Terminal.app) and run these four steps in order:

```bash
# 1. Clone this repository
git clone https://github.com/sourabhbgp/hn-watch.git

# 2. Go into the project folder
cd hn-watch

# 3. Install the dependencies
npm install

# 4. Launch the app - opens the native HN Watch window
npm run tauri dev
```

The **first** launch takes a few minutes: Rust compiles the app the first time. Every launch after
that is fast. When you're done, quit from the tray menu (see [Using HN Watch](#using-hn-watch)).

## Using HN Watch

**1. Create a monitor.** In the left sidebar, click **New monitor** and fill in three things:

- **Name** - anything, e.g. _"AI agent launches"_
- **What you care about** - a plain-English description, e.g. _"AI-agent startup launches"_ or
  _"Rust async runtime discussions"_. This is the prompt Claude uses to judge each story.
- **Cadence** - how often it checks: **every 15m**, **30m**, **1h**, or **6h**.

Save it, and the monitor starts running immediately.

**2. Watch the feed.** On each tick, the monitor pulls recent Hacker News stories, asks `claude -p`
which ones match your prompt, and appends the matches to the central feed - each with a one-line
summary. Results are deduplicated, so you never see the same story twice, and everything is saved
locally, so your monitors and feed survive a restart.

**3. Let it run in the background.** Closing the window doesn't quit HN Watch - it tucks into the
**system tray** and keeps watching. When new matches land, you get one **native notification** per
monitor. To actually quit, use the tray-menu **Quit**.

**4. Dig deeper on any story.** Click **Dig deeper** on a feed card to launch a research swarm:
several `claude -p` agents investigate the story in parallel - the company/people, how the tech
works, the market, and a skeptic's take - streaming their progress live, then compiling into one
**combined brief**. The brief is saved with the story; reopen it any time (instantly, no re-run), or
hit **Dig deeper again** to refresh it.

## How it works

Scheduled monitors and the on-demand swarm are the **same primitive** - a `claude -p` call - run at
opposite tempos: a **trickle** (one call per tick, forever) versus a **burst** (many calls the
instant you click). They share one agent runtime but draw from **two reserved concurrency pools**,
so an interactive swarm never queues behind background ticks and a long swarm never blocks a tick.

| Layer         | Choice                                                              |
| ------------- | ------------------------------------------------------------------ |
| Shell         | [Tauri 2](https://tauri.app) (Rust core + OS WebView)             |
| UI            | React 19 + TypeScript + Vite + Tailwind CSS v4                     |
| Agent runtime | `claude -p` (Claude Code, headless) - spawned as child processes  |
| Data          | HN via the [Algolia HN Search API](https://hn.algolia.com/api)    |
| Storage       | SQLite (`rusqlite`, bundled) - local, restart-safe                 |

**Want the full picture?** A visual system design - the two-pool runtime, the tick flow, and the
swarm fan-out - lives in [`docs/architecture.html`](./docs/architecture.html) (open it in a browser).

## Platform

Built, run, and tested on **macOS** only. It's a Tauri app, so a Windows or Linux build may well be
possible - but that's unverified, and the tray/notification paths are macOS-specific in practice.

## Development

```bash
npm run tauri build            # produce a standalone app bundle
cd src-tauri && cargo test     # Rust core (parsers, DB, health/state machines, concurrency)
npm run build                  # tsc typecheck + Vite production build
```

Test against the **real native window**, not a browser at `localhost` - the tray, notifications,
`claude` subprocesses, and SQLite storage only exist in the Tauri shell. The verified test loop is in
[`docs/TESTING.md`](./docs/TESTING.md). For the assignment brief see
[`docs/REQUIREMENTS.md`](./docs/REQUIREMENTS.md); for the per-session build log and design rationale,
[`STATUS.md`](./STATUS.md).
