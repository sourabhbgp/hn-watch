# Dig-deeper research swarm — design

**Date:** 2026-07-10
**Ticket:** [`STATUS.md`](../../../STATUS.md) "Next — Dig-deeper research swarm (last phase)";
verbatim requirement in [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)
**Branch:** `feat/dig-deeper-swarm`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

Every feed item's **Dig deeper** button is currently a no-op wired to mock data
(`src/mock/data.ts`, `DigDeeperPanel.tsx`). The requirement: clicking it must kick off a **Rust
orchestrator** that spins up **several parallel `claude -p` agents**, each investigating the
story from a different angle, **streaming live progress** to the panel, then **compiling one
combined brief**. The brief explicitly frames this as the second of two tempos the same
`claude -p` runtime must support — "one call per tick versus many at once" — and says **how we
handle that is what's being evaluated**.

This is the last unbuilt piece of the core requirement.

## Goal (acceptance)

- Clicking **Dig deeper** on any feed item starts a swarm scoped to that one story.
- The set of investigative angles is **decided per story** (2–5), not a fixed template — a
  "startup launch" story and a "Rust async runtime" story need different lenses, and a single
  hardcoded angle set is wrong for one of them.
- The user can **review and edit** the proposed angles (remove down to 2, add up to 5, in their
  own words) before the swarm actually runs — a human-in-the-loop checkpoint before the costly
  parallel step fires.
- Each confirmed angle runs as its **own `claude -p` process**, streaming visible progress to its
  own lane in the panel, independently of the others.
- Once all angles have finished (or failed/timed out), **one** more `claude -p` call compiles a
  combined brief from whatever succeeded — a degraded brief if some angles failed, never a stuck
  panel.
- Monitor ticks and the swarm **share the same bounded `claude -p` runtime** without either
  starving the other: a dig-deeper click never queues behind a background tick, and ticks are
  never blocked by an in-flight swarm.

## Core idea

**Orchestrator-worker, Rust-owned, angle count decided per story.** This matches Anthropic's own
published multi-agent research architecture (lead agent plans → parallel subagents investigate →
lead compiles) — but our "lead" is a single cheap planning call, not a full agentic loop, because
our domain (investigate one already-identified HN story) is narrow enough that dynamic *planning*
is valuable but dynamic *orchestration* (retries, re-planning, tool budgets) is not. Rust remains
the orchestrator throughout, per the requirement's own wording ("an orchestrator **in the Rust
layer**") — it owns process spawning, concurrency, streaming, and invoking the planning/synthesis
calls; the LLM's job is narrowly "decide what to investigate," never "decide how to run."

**Two tempos, two reserved pools.** A monitor tick is background — nobody is watching it, a short
queue is invisible. A dig-deeper is interactive — the user just clicked and is watching the panel.
A single shared FIFO semaphore (today's `agent_sem`, 4 permits) treats both as equals, which is
backwards: it lets an invisible background job delay a visible one. Fix: split into two
independent, fixed-size pools — ticks only ever draw from a small `tick` pool, the swarm only
ever draws from its own `swarm` pool. Neither can block the other. This is the concrete answer to
the brief's "how do you handle two tempos sharing one runtime" question.

## Design

### A. Data model — no new persistence; swarm state is in-memory and ephemeral

A dig-deeper run lives only as long as the panel does — closing it and reopening re-runs from
scratch. This is a deliberate non-goal (see below), not an oversight: the requirement asks for a
live view + a compiled brief, not a research history feature. No new SQLite tables.

Backend keeps one in-memory registry, mirroring the existing `Scheduler.handles` pattern in
`scheduler.rs`:

```rust
struct SwarmRegistry {
    // item_id -> the orchestration task, so a cancel can find and abort it
    handles: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}
```

Frontend types (`src/types.ts`):

- `AngleStatus` gains `"error"` (currently `"queued" | "running" | "done"`).
- `SwarmAngle` gains `error?: string`.
- New `PlannedAngle { id: string; label: string; focus: string }` — the shape both the planner's
  output and a user-added pill take, before a run starts.
- `Brief` / `BriefSection` unchanged — the synthesis call's output already matches this shape
  exactly.

### B. Planning call — decides the angles for *this* story

One buffered `claude -p` call (like today's `judge()` — closed-book, sandboxed, no tool use
needed since it only reasons over text we already have):

```
You are planning a research swarm to dig deeper into one Hacker News story,
for a user whose monitor is interested in: "{monitor.prompt}"

Story: "{title}" ({domain}, {url})
Why it matched: {reason}
Initial summary: {summary}

Decide between 2 and 5 distinct investigative angles for THIS SPECIFIC STORY.
Each angle should pull from genuinely different context or sources — do not
force a generic template if it doesn't fit.

Return ONLY a JSON array (2 to 5 elements, no prose) of objects with exactly:
- "label": short 2-4 word angle name
- "focus": one sentence telling an investigator exactly what to look into
```

`reason` and `summary` are already sitting in `feed_items` from the tick that produced this match
— no extra HN fetching needed.

```rust
async fn plan_angles(monitor_prompt: &str, item: &FeedItemContext) -> Vec<PlannedAngle>
```

Parses defensively (same "find the first `[...]`" approach `parse_verdict` already uses), then:

- **Clamps** to `MIN_ANGLES..=MAX_ANGLES` (2..=5), drops entries with an empty `label`/`focus`.
- **Falls back** to a fixed default 4-angle set (the mock's original: Company & people / Tech &
  how it works / Market & rivals / Skeptic risks) if the call fails, times out, or the parsed/
  filtered result has fewer than 2 valid angles. The confirm step must always have something to
  show.
- Assigns each a stable `id` (uuid) and an icon **client-side**, cycling a small fixed emoji pool
  by index — the LLM never supplies the icon, so rendering can't break on a bad value.

This function never returns `Result` — a total failure resolves to the default set, not an error,
so the UI flow below always has a next step.

### C. Human-in-the-loop confirm step

New panel phase between "planning" and "running": `swarm-planned` event delivers the angle list,
the panel renders each as a **pill showing only the short `label`** (not the full `focus`
sentence — scannable, not a wall of text), with:

- **✕** to remove a pill — disabled once only 2 remain.
- A text input + **Add** — disabled once 5 are present. Accepts any length text: one word or a
  full sentence. Whatever is typed becomes that angle's `focus` verbatim; the pill's displayed
  `label` is a short derived/truncated preview of the same text (no extra LLM call needed for
  user-added angles).
- **Start research** — confirms the (possibly edited) list and fires the swarm.

Backend command split in two, so nothing runs until the user confirms:

```rust
async fn start_dig_deeper(item_id: String) -> Vec<PlannedAngle>   // runs the planner, returns proposal
async fn confirm_dig_deeper(item_id: String, angles: Vec<PlannedAngle>)  // re-validates, spawns the run
```

`confirm_dig_deeper` re-clamps to 2..=5 server-side (never trusts the frontend's own disabled-
button guard alone) before spawning anything.

The panel is a single overlay (`DigDeeperPanel.tsx` is one `fixed inset-0` slide-over), so only
one dig-deeper run is ever active at a time by construction: clicking **Dig deeper** on a second
item while one is running cancels the first (§G's cancellation path) and starts fresh on the new
item. `swarm_sem` being a single shared pool of 5 means the total-concurrency guarantee in §F
holds regardless of this — it's a UX clarification, not a correctness one.

### D. Fan-out — parallel streaming workers

One `claude -p --output-format stream-json --verbose --include-partial-messages` child process
per confirmed angle, run concurrently (bounded by the `swarm` pool, §F):

```
You are one investigator in a research swarm looking into a single HN story,
focused ONLY on this angle: "{angle.focus}"

Story: "{title}" ({url})
Context: this matched a monitor interested in "{monitor.prompt}" because: {reason}

Investigate strictly from your assigned angle — don't try to cover the whole
story. You may use web search/fetch to look into the story's URL and related
context. Produce a concise 3-6 sentence findings write-up. This is what gets
compiled into a combined brief, so make it stand on its own.
```

Unlike `judge()`, these calls **keep normal tool access** (web fetch/search) — `judge()`'s
`--safe-mode` sandbox exists to stop a closed-book classification task from touching the
filesystem or the network; these agents are specifically supposed to go look things up. This is
the real functional difference between the two tempos, not just concurrency: ticks are closed-
book judging, the swarm is open, tool-using investigation. (See open question below — whether
`--safe-mode` itself would block tool use is unverified; the plan either way is: workers run
*without* `--safe-mode`, since they need real tool access.)

```rust
async fn investigate_angle(item: &FeedItemContext, angle: &PlannedAngle, app: AppHandle) -> Result<String, AgentError>
```

Reads the child's stdout with `AsyncBufReadExt::lines()`; each NDJSON line is parsed for its
`type` (`text_delta` / tool-call events) and forwarded as a short human-readable line via
`swarm-progress`. On process exit, the accumulated final assistant text is the angle's output; a
failure or timeout produces `AgentError` instead, handled per §G — one angle's failure does not
stop the others (each runs in its own `tauri::async_runtime::spawn`, joined at the end, not
short-circuited by `?`).

### E. Synthesis call — one LLM call, not a merge

Once every angle has settled (succeeded or failed), one buffered call compiles the brief:

```
Compile a combined research brief from {N} investigators who each looked at
one HN story from a different angle.

Story: "{title}" ({url})

### {angle1.label}
{angle1.output}
...
[Note: the "{label}" angle could not be completed (timed out).]   <- only for failures

Write: a 2-3 sentence overview, then sections (reuse or reorganize the angle
labels as headings). Return ONLY JSON: {"summary": "...", "sections": [{"heading","body"}, ...]}
```

```rust
async fn synthesize_brief(item: &FeedItemContext, results: &[(PlannedAngle, Result<String, AgentError>)]) -> Result<Brief, AgentError>
```

Output shape matches `Brief { summary, sections }` exactly — no new type needed. If **every**
angle failed, this call is skipped entirely and the panel shows an error state directly ("All
research angles failed — try again") rather than synthesizing from nothing.

### F. Concurrency — two reserved pools, no shared queue

```rust
fn tick_sem() -> &'static Semaphore { Semaphore::new(TICK_PERMITS) }   // 2
fn swarm_sem() -> &'static Semaphore { Semaphore::new(SWARM_PERMITS) } // 5
```

Every tick call (`judge()`) acquires from `tick_sem`. Every swarm call — planner, each angle
worker, synthesis — acquires from `swarm_sem`. Strict separation, **no overflow** between pools:
simpler than a priority queue, fully testable as two independent counters, and gives the exact
guarantee needed — a dig-deeper's own permits are never contended by background ticks, and vice
versa. `swarm_sem` is sized to `MAX_ANGLES` (5) so one dig-deeper's own angles never queue behind
each other — true parallelism within a single run, not just across runs.

Waiting itself needs no new code: `tokio::sync::Semaphore::acquire` already blocks the caller
until a permit frees, in FIFO order — this is a documented property of the primitive, not
something built by hand. Total `claude` processes running at once, ever, across the whole app:
at most `TICK_PERMITS + SWARM_PERMITS` = **7**.

This replaces the single `agent_sem()` in `agent.rs` (its doc comment already anticipated ticks
and the swarm sharing one thing — this is that plan realized as two pools instead of one).

### G. Failure handling & cancellation

- Each angle worker has its own timeout (`ANGLE_TIMEOUT_SECS`); one angle timing out or erroring
  only flips that lane to `"error"` (with a final line + `error` message) — the others keep
  running unaffected.
- Synthesis runs against whatever succeeded; a per-angle failure note is included so the compiled
  brief is honest about gaps rather than silently pretending every angle worked.
- **Cancellation** (closing the panel mid-run): the orchestration task and any still-running angle
  tasks are `abort()`-ed via the `SwarmRegistry` handle. `claude_command()` already sets
  `kill_on_drop(true)` on every child process it builds, so aborting the Rust task tree is
  sufficient to kill the underlying OS processes too — no new process-management code needed,
  same guarantee the app already relies on elsewhere.

### H. Events (Tauri, mirrors the existing `tick-started`/`tick-finished` camelCase convention)

| event | payload |
| --- | --- |
| `swarm-planned` | `{ itemId, angles: PlannedAngle[] }` |
| `swarm-progress` | `{ itemId, angleId, line }` |
| `swarm-angle-done` | `{ itemId, angleId, output: string \| null, error: string \| null }` |
| `swarm-brief-ready` | `{ itemId, brief: Brief }` |
| `swarm-failed` | `{ itemId, error }` (all angles failed) |

### I. Constants

| name | value | meaning |
| --- | --- | --- |
| `TICK_PERMITS` | `2` | max concurrent monitor-tick `claude` calls |
| `SWARM_PERMITS` | `5` | max concurrent swarm `claude` calls (= `MAX_ANGLES`) |
| `MIN_ANGLES` | `2` | floor enforced both client- and server-side |
| `MAX_ANGLES` | `5` | ceiling enforced both client- and server-side |
| `PLAN_TIMEOUT_SECS` | `30` | planning call — short, closed-book |
| `ANGLE_TIMEOUT_SECS` | `120` | per-angle worker — longer than `judge()`'s 90s; may use tools |
| `SYNTHESIS_TIMEOUT_SECS` | `90` | matches `judge()`'s existing timeout |

## Open questions / risks to verify during implementation

- **Does `--safe-mode` block web tool use, or only filesystem access?** Unconfirmed — the
  research pass that would have verified this hit a session-token limit before reaching it. The
  design's assumption (§D) is that angle workers run *without* `--safe-mode` so they can actually
  browse; this needs an empirical check with a real `claude -p` call early in implementation,
  before building the rest of the fan-out around it.
- **`stream-json` event shape in practice** — the exact NDJSON `type` values worth surfacing as
  progress lines (vs. ones to drop) should be confirmed against a real streamed response, not
  assumed from docs alone.

## Non-goals (staying strictly in this ticket)

- **No persistence of swarm runs.** A dig-deeper's plan, progress, and brief are ephemeral —
  closing the panel discards them; reopening dig-deeper on the same item starts fresh. Revisit
  only if a later session wants a "past research" feature; not asked for here.
- **No fully agentic/unbounded orchestration.** The planner proposes angles once, up front; there
  is no re-planning loop, no dynamic spawning of *more* agents mid-run, no separate citation-
  verification pass (unlike Anthropic's own system, which has one). Bounded 2–5 stays lightweight
  and testable, matching the brief's "keep it lightweight" guidance.
- **No pool-overflow / priority queue.** Reserved pools are strictly separate (§F) — simpler than
  letting one borrow from the other, and sufficient for the guarantee actually needed.
- **No cross-monitor or cross-item swarm reuse/caching.** Each dig-deeper click is independent,
  even for the same story investigated twice.

## Testing

- **Rust unit tests (pure functions):**
  - Planner output parsing/clamping: valid 2–5 array passes through; `<2` valid entries after
    filtering falls back to the default set; malformed JSON falls back; entries missing
    `label`/`focus` are dropped before the count check.
  - Icon assignment is deterministic by index, never LLM-supplied.
  - `tick_sem`/`swarm_sem` are independent — exhausting one doesn't affect the other's available
    permit count (can be tested by acquiring all of one and confirming the other still yields
    immediately).
- **Build/verify order (not a testing type, but the right sequence given the moving parts):**
  build the fan-out + streaming + synthesis path first against a **fixed** angle set (validates
  `stream-json` parsing, event forwarding, the two-pool concurrency interplay, cancellation) —
  the riskiest plumbing. Add the dynamic planner and the human-in-the-loop confirm step last; it's
  the simplest piece to add on top and the easiest to drop if time runs short.
- **Live verification** in the native window per [`docs/TESTING.md`](../../TESTING.md): click dig
  deeper on a real feed item → planner proposes angles suited to that story → edit (remove one,
  add a custom one) → Start → lanes stream live progress independently → one lane forced to fail
  (e.g. bad `HN_WATCH_CLAUDE_BIN` mid-run) still yields a degraded brief noting the gap → close
  panel mid-run confirms the underlying `claude` processes are actually killed (`ps` check) → a
  monitor tick firing during an active swarm is confirmed to run without delay (two pools holding).

## Files touched

- `src-tauri/src/agent.rs` — `tick_sem`/`swarm_sem` replace `agent_sem`; `plan_angles`,
  `investigate_angle`, `synthesize_brief`; streaming variant of the `claude` invocation
  (`--output-format stream-json`, no `--safe-mode` for angle workers).
- `src-tauri/src/swarm.rs` (new) — orchestration: `start_dig_deeper`/`confirm_dig_deeper` flow,
  `SwarmRegistry`, event emission, cancellation.
- `src-tauri/src/commands.rs` — new Tauri commands wired to `swarm.rs`.
- `src/types.ts` — `AngleStatus` gains `"error"`; `SwarmAngle.error?`; new `PlannedAngle`.
- `src/components/DigDeeperPanel.tsx` — new confirm phase (editable pills + Start), `error` lane
  styling reusing the existing `text-rust`/`bg-rust` tokens (already used for monitor errors in
  `Sidebar.tsx` — no new colors).
- `src/App.tsx` / `src/api.ts` — event listeners for the five new `swarm-*` events, replacing the
  mock wiring.
