# HN Watch

A native desktop **watchtower for Hacker News**, powered by local Claude agents.

You describe what you care about in plain English (e.g. _"AI-agent startup launches"_) and
set a cadence. A background worker per **monitor** polls Hacker News, asks `claude -p` to judge
and summarize what's relevant, and streams matches into one Twitter-style feed. Everything is
deduplicated, saved locally, and survives restarts — the app lives in the system tray and fires
a native notification when new items land. Any feed item can launch a **research swarm**: several
`claude -p` agents investigating in parallel, streaming live, then compiled into one brief.

![status: feature-complete against the brief](https://img.shields.io/badge/status-feature--complete-1f6feb)

> A full per-session build log — what was built when, and every bug caught along the way — lives
> in [`STATUS.md`](./STATUS.md). The verbatim assignment brief is in
> [`docs/REQUIREMENTS.md`](./docs/REQUIREMENTS.md).

---

## The interesting part

Scheduled monitors and the on-demand swarm are the **same primitive** — a `claude -p` call —
driven at opposite tempos: a **trickle** (one call per tick, runs forever) versus a **burst**
(many calls the instant you click). The design question the brief is really about is: _how do you
run one-call-per-tick and many-calls-at-once through the same runtime without starving either or
melting the rate limit?_

**The answer here is two reserved concurrency pools.** Monitor ticks draw only from a 2-permit
`tick_sem`; the dig-deeper swarm (planner + up to 5 workers + synthesis) draws only from a
separate 5-permit `swarm_sem`. Strict separation, no overflow: an interactive swarm never queues
behind background ticks, and a long-running swarm never blocks a scheduled tick. (See
`src-tauri/src/agent.rs`.)

A visual walkthrough lives in [`docs/architecture.html`](./docs/architecture.html) — open it in a browser.

## Stack

| Layer         | Choice                                                              |
| ------------- | ------------------------------------------------------------------ |
| Shell         | [Tauri 2](https://tauri.app) (Rust core + OS WebView)              |
| UI            | React 19 + TypeScript + Vite                                       |
| Styling       | Tailwind CSS v4                                                    |
| Agent runtime | `claude -p` (Claude Code, headless) — spawned as child processes  |
| Data          | HN via the [Algolia HN Search API](https://hn.algolia.com/api)    |
| Storage       | SQLite (`rusqlite`, bundled) — local, restart-safe                 |

Cross-platform by construction (one codebase → macOS / Windows / Linux). Development and builds
target **macOS** (the dev machine); the tray/notification/preflight paths are macOS-verified.
Shipping the others is adding CI runners, not code changes.

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

> **Test in the real native window, not a browser at `localhost`.** The app's behavior (tray,
> notifications, `claude` subprocesses, SQLite in the app-data dir) only exists in the Tauri
> shell. See [`docs/TESTING.md`](./docs/TESTING.md).

## Design decisions & trade-offs

The parts worth calling out (fuller rationale per feature in [`STATUS.md`](./STATUS.md)):

- **Two reserved agent pools, not one shared semaphore.** The whole point of the brief is the two
  tempos through one runtime; isolating them is the cleanest way to guarantee neither starves the
  other. Trade-off: a saturated swarm can't borrow idle tick permits. Accepted — predictability
  beats peak throughput for a background watchtower.

- **Watermark-based ingestion, fetched in full — not "newest 30."** Each monitor carries a
  watermark and pulls *everything* since it (paginated), so a burst of 500 stories isn't silently
  truncated to 30. The watermark advances to `max(created_at) − 5min`: Algolia indexes
  asynchronously, so an older-timestamped story can be indexed *after* newer ones — the 5-minute
  margin re-scans that tail each tick (free, since it's `seen`-deduplicated). A per-tick 500-story
  cap bounds the window after a long laptop sleep. Trade-off: after a multi-day gap, stories beyond
  the cap are intentionally skipped rather than replayed.

- **Fail-closed judging.** The unseen set is chunked into `claude` calls of ≤30, run sequentially
  within a tick. If any batch fails, the tick returns an error **before any DB write** — nothing is
  committed and the watermark doesn't advance, so the whole window is re-judged next tick. No
  half-ingested state.

- **Sandboxed `claude` calls.** Every judge/planner/synthesis call runs from a temp dir with `$PWD`
  overridden, `--safe-mode`, and null stdin, so a background tick can never read your files or
  trip a macOS file-access prompt. Swarm *workers* run with `--allowedTools WebSearch WebFetch`
  (least privilege — they need the web, nothing else). The tick filter is pinned to
  `claude-sonnet-5` so results don't drift with the host's default model.

- **Streaming swarm with real cancellation.** Workers run `claude -p --output-format stream-json`;
  progress is forwarded live to per-angle lanes. Closing the panel aborts in-flight workers via a
  `JoinSet` + `kill_on_drop`, which SIGKILLs their `claude` children — no orphaned processes. A
  failed/timed-out angle degrades gracefully: the brief still compiles from the survivors and notes
  the gap.

- **Completed research is persisted; reopening spawns zero `claude`.** A finished dig-deeper run is
  saved per feed item (brief + every angle). Reopening shows it instantly from SQLite; a **Dig
  deeper again** button re-runs on demand.

- **Close-to-tray, Rust-fired notifications.** Closing the window hides it (workers keep ticking);
  the only quit path is the tray menu. One native notification per monitor that landed matches
  (coalesced so a burst can't spam you).

- **Errors are surfaced, never swallowed.** A startup preflight probes whether `claude` is present
  and logged in; every tick failure carries a human-readable reason shown on the monitor. A
  0-match or unparseable model response is a valid empty result, **not** an error.

### Deliberately not built (scope control)

Per the brief's "keep it lightweight / stub incidental plumbing" guidance: monitor edit/pause,
"Run now", full-history (FTS) search, and wall-clock catch-up scheduling after sleep were all
considered and left out or stubbed. The reasoning for each is in [`STATUS.md`](./STATUS.md).

## Project structure

```
hn-watch/
├─ src/               # React UI (the WebView)
│  ├─ components/     # Sidebar, Feed, FeedCard, DigDeeperPanel, ClaudeBanner
│  ├─ lib/            # search, highlight, time helpers
│  ├─ api.ts          # typed wrappers over Tauri commands/events
│  └─ types.ts        # shared UI types (mirror the Rust DTOs)
├─ src-tauri/         # Rust core
│  └─ src/            # db, hn, agent, tick, scheduler, swarm, tray, commands, lib
├─ docs/
│  ├─ REQUIREMENTS.md     # the verbatim requirement (source of truth)
│  ├─ design.md           # design system — tokens, brand, components
│  ├─ architecture.html   # the visual system design
│  └─ TESTING.md          # native-window test loop
├─ STATUS.md          # per-session build log — what's done, how, and why
└─ README.md
```

## Tests

```bash
cd src-tauri && cargo test     # Rust core (parsers, DB, health/state machines, concurrency)
npm run build                  # tsc typecheck + Vite production build
```

The Rust suite covers the pure logic — verdict/brief/stream parsing, the DB layer and migrations,
the Claude-health state machine, and the two-pool concurrency invariant. UI flows are verified
live in the native window (see `docs/TESTING.md`).
</content>
</invoke>
