# HN Watch — TODO (deferred enhancements)

Prioritized backlog of things we deliberately deferred. **Nothing here is started.**
We will pick these up **one at a time** in upcoming sessions. Each item has enough
detail to act on cold. Source of truth for scope remains [`docs/REQUIREMENTS.md`](REQUIREMENTS.md).

---

## 1. Tick observability — the app must show what it's doing

**Problem.** Today a monitor is a black box. When you create one there is no loading
indicator, no "checking…" state, no "last checked" time, and no "0 new" message. If a tick
finds nothing (or fails), the feed is just blank with no explanation — you cannot tell
"working, nothing matched" from "broken." (We confirmed this live: a "Claude" monitor
correctly checked 30 stories and matched 0, but the UI showed nothing, so it *looked* dead.)

**Why it matters.** A watchtower that gives zero feedback feels broken even when it works.
This was the single biggest source of "is it even running?" confusion.

**Current behavior.** A tick runs HN fetch → `claude` → insert matches, and only
`eprintln!`s to stderr on failure. The UI never learns a tick started, finished, or failed.

**Proposed approach.**
- Backend: emit tick-lifecycle events (e.g. `tick-started` / `tick-finished`) carrying
  `{ monitor_id, checked_count, new_count, error? }`. Persist a `last_checked_at` (and maybe
  `last_result`) per monitor.
- UI: per-monitor status line — "Checking…" while a tick runs, then
  "Last checked 3:31 PM · checked 30 · 0 new". Empty-feed message: "Checked N stories,
  nothing matched yet" instead of a blank pane.

**Acceptance.** From the UI alone you can always tell: is it checking now, when did it last
check, how many stories it looked at, how many were new, and whether the last tick errored.

---

## 2. Lossless ingestion under variable volume — never miss, never re-analyze

**Problem.** Between two 30-min ticks the number of new HN stories varies a lot — sometimes
5, sometimes 200, sometimes 100. We must guarantee **(a) no story is missed** and
**(b) no story is analyzed twice**, regardless of that volume.

**Current behavior.**
- **Duplication (b): already solved.** The per-monitor `seen` table (persisted in SQLite) +
  `UNIQUE(monitor_id, hn_id)` on `feed_items` mean a story is sent to `claude` at most once
  per monitor, ever — even across restarts. This part is robust; keep it.
- **Missing (a): a real gap.** Each tick fetches only the **30 newest** stories
  (`hn::fetch_recent(30)` → Algolia `search_by_date`). If a burst of >30 new stories lands
  between ticks, stories beyond the newest 30 are never fetched → never seen → permanently
  missed. In practice HN's story rate is well under 30/30-min, so it rarely bites, but the
  fixed cap does **not guarantee completeness**.

**Proposed approach (make it bulletproof at any volume).**
- **Watermark, not a fixed count.** Store the newest processed HN submission timestamp per
  monitor. Each tick, request everything submitted since that watermark
  (`numericFilters=created_at_i>watermark`) and **paginate** (Algolia `page=0,1,2…`) until you
  reach already-seen / pre-watermark stories. This pulls the exact new window — 5 or 5,000 —
  with nothing skipped.
- **Chunk the `claude` calls.** Never jam a burst into one giant prompt. Split the unseen set
  into batches of ~30 and run them as separate `claude` calls, bounded by the existing shared
  semaphore. (This is the deeper reason ~30 is a good number — it's a sane per-call batch size,
  not a coverage limit.)
- Keep `seen` + `UNIQUE` exactly as-is for dedup.

**Acceptance.** Simulate/observe a high-volume window: every story submitted since the last
tick appears (paginated in), each is analyzed exactly once, and no duplicate feed cards —
independent of how many arrived.

---

## 3. Robust error handling + mandatory Claude Code startup check

**Problem.** `claude` is mandatory — it's the whole engine (relevance judging + summaries).
Right now, if the user hasn't installed Claude Code, or isn't logged in, or `claude` errors,
the tick fails silently (stderr only) and the feed is just empty with no explanation.

**Why it matters.** A person who clones/installs the app without a working, authenticated
`claude` gets a silent, broken-looking app. The app should tell them clearly.

**Proposed approach.**
- **Startup preflight (first thing on launch).** Before/while spawning workers, check:
  1. Is the `claude` binary present? (reuse the existing `claude_bin()` resolver.)
  2. Is it authenticated / usable? (e.g. a quick probe call, or detect the
     "Not logged in · Please run /login" response.)
  If either fails, surface a clear, blocking-but-friendly banner: "Claude Code not found —
  install it and run `claude` to log in" / "Claude Code is not logged in — run `claude`".
- **Per-tick error surfacing.** Distinguish and display the failure modes instead of silence:
  `claude` missing, `claude` not authenticated, `claude` timed out, HN fetch failed, bad/empty
  verdict. Show the reason on the monitor (ties into TODO #1's status line) rather than only
  logging to stderr.
- Keep the current graceful behavior (a failed tick never kills the worker) — just make it
  *visible*.

**Acceptance.** On a machine without Claude Code (or logged out), the app opens and immediately
tells the user exactly what's wrong and how to fix it — no silent empty feed. Every tick failure
mode shows a human-readable reason in the UI.

---

_Order to tackle: #3 (make failures visible / preflight) and #1 (observability) pair naturally
and are the highest user-facing value; #2 (lossless ingestion) is the correctness upgrade for
scale. Do them one per session._
