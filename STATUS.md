# STATUS

A running log of what actually works, what's next, and how to run the app.
Updated at the end of every working session.

---

## Now (Session 1 — Scaffold + static UI shell)

**Done**

- [x] Scaffolded Tauri 2 + React 19 + TypeScript + Vite
- [x] Wired Tailwind CSS v4 (Vite plugin) with the HN Watch design tokens
- [x] Window configured — title "HN Watch", 1160×780, min 900×600
- [x] Project docs seeded: `README.md`, `STATUS.md`, `DECISIONS.md`, `docs/architecture.html`
- [x] Static, non-functional UI shell rendering **mock data**:
  - Left sidebar: list of monitors + "New monitor" button (no-op)
  - Center: Twitter-style feed of match cards
  - Slide-over: "Dig deeper" panel with mock agent lanes + a compiled brief
- [x] Git initialised; baseline on `main`; UI built on `feat/ui-shell` and merged

**Not yet (intentionally)** — everything below is UI-only mock; no logic behind it:
buttons are no-ops, data is hard-coded, nothing calls Rust or Claude, no persistence.

## How to run

```bash
npm install
npm run tauri dev     # opens the native HN Watch window
```

Requires Node 20+, Rust stable, and `claude` on the PATH (used from Phase 3 onward).

## Next (Session 2 — Monitors CRUD + persistence)

- [ ] Define shared types + Tauri command surface (`create_monitor`, `list_monitors`, …)
- [ ] SQLite store in the Rust core; monitors survive restart
- [ ] Wire the sidebar create/edit/delete to real Rust commands
- [ ] Replace mock monitors with live data from the store

## Backlog (later phases)

- [ ] HN ingestion + pre-filter
- [ ] Agent runtime (bounded pool over `claude -p`) + one real monitor tick
- [ ] Background workers, dedup, tray, native notifications
- [ ] Dig-deeper swarm: parallel `claude -p`, live streaming → compiled brief
- [ ] Error handling, polish, design write-up in README

---

_Workflow: each phase is built on its own `feat/*` branch and merged into `main`._
