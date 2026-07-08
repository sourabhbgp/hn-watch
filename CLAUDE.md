# HN Watch — session context

Native desktop **watchtower for Hacker News**, powered by local `claude -p` agents.
Built in phases across sessions. This file is intentionally light — it loads every session.

## Rules

- **Read the requirement first:** [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md) is the
  source of truth. Don't over-build or under-build it.
- **DRY:** reuse existing components, helpers, and design tokens — don't duplicate logic or
  restate the same thing in two places.
- **Reuse the design tokens** in `src/index.css` / [`docs/design.md`](docs/design.md); never
  hardcode colors, fonts, or spacing.
- **Test in the real native window**, not a browser at localhost — see
  [`docs/TESTING.md`](docs/TESTING.md).
- **One unit of work per `feat/*` branch**, merged into `main` with a clear message.
- **Update [`STATUS.md`](STATUS.md)** at the end of each session (brief log of what changed).

## Key files (only what a session usually needs)

| Need | File |
| --- | --- |
| What we're building & why (verbatim brief) | `docs/REQUIREMENTS.md` |
| Where we are / per-session log | `STATUS.md` |
| Design system — tokens, brand, components | `docs/design.md` |
| Visual system architecture | `docs/architecture.html` |
| Native-window test loop | `docs/TESTING.md` |
| UI code | `src/` |
| Rust core (window, tray, workers, agent runtime) | `src-tauri/` |
