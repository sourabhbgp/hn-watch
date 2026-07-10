# Dig-deeper research swarm — design

**Date:** 2026-07-10
**Ticket:** [`STATUS.md`](../../../STATUS.md) "Next — Dig-deeper research swarm (last phase)";
verbatim requirement in [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)
**Branch:** `feat/dig-deeper-swarm`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

Every feed item's **Dig deeper** button is a no-op wired to mock data (`src/mock/data.ts`
`BRIEF_F1`, rendered by `DigDeeperPanel.tsx`). The requirement: clicking it must kick off a
**Rust orchestrator** that spins up **several parallel `claude -p` agents**, each investigating
the story from a different angle, **streaming live progress** to the panel, then **compiling one
combined brief**. The brief frames this as the second of two tempos the same `claude -p` runtime
must serve — "one call per tick versus many at once" — and states plainly that **how we handle
that is what's being evaluated**.

This is the last unbuilt piece of the core requirement.

## Goal (acceptance)

- Clicking **Dig deeper** on any feed item starts a swarm scoped to that one story.
- The investigative angles are **decided per story** (2–5), not a fixed template — a startup-
  launch story and a Rust-async-runtime story need different lenses, and one hardcoded set is
  demonstrably wrong for at least one of the brief's own two example monitors.
- The user can **review and edit** the proposed angles — remove down to 2, add up to 5 in their
  own words (one word or a full sentence) — before the swarm runs. A human-in-the-loop checkpoint
  before the costly parallel step fires.
- Each confirmed angle runs as its **own `claude -p` process**, actually using read-only web
  tools to investigate, streaming visible progress to its own lane, independently of the others.
- Once all angles settle (done / failed / timed out), **one** more `claude -p` call compiles a
  combined brief from whatever succeeded — a degraded brief noting any gaps, never a stuck panel.
- Monitor ticks and the swarm **share the same bounded `claude -p` runtime** so that neither
  starves the other: a dig-deeper click never queues behind background ticks, and ticks are never
  blocked by an in-flight swarm.

## Core design decisions (the parts under evaluation)

### 1. Topology — orchestrator-worker, Rust-owned, planner decides the fan-out

Matches Anthropic's own published multi-agent research architecture (lead plans → parallel
subagents investigate → lead compiles), which the research pass confirmed as the proven default
(3-0 verified across Anthropic's engineering blog, the coordination-patterns blog, and two
secondary write-ups). Two deliberate narrowings for this app's scope:

- **The "lead" is a single cheap planning call, not an agentic loop.** Our domain — investigate
  one already-identified HN story — is narrow enough that dynamic *planning* (which angles) is
  valuable but dynamic *orchestration* (re-planning, tool budgets, spawning more waves) is not.
- **Rust is the orchestrator throughout**, per the requirement's literal wording ("an
  orchestrator **in the Rust layer**"). Rust owns process spawning, concurrency, streaming,
  cancellation, and *invokes* the planning/synthesis calls. The LLM only ever decides *what to
  investigate*, never *how to run it*. This keeps the concurrency/failure logic in testable Rust
  rather than in a prompt.

Rejected — **fully agentic/unbounded orchestration** (an LLM plans + spawns dynamically with no
Rust-enforced ceiling): against a brief that says "keep it lightweight," a senior reviewer reads
unbounded-N-with-no-fallback as weak judgment. Bounded 2–5 is the defensible middle.

### 2. Two tempos → two reserved concurrency pools

A monitor tick is **background** — nobody is watching, a short queue is invisible (monitors fire
every 15m–2h). A dig-deeper is **interactive** — the user just clicked and is watching the panel.
Today's single shared `agent_sem` (4 permits, FIFO-fair) treats both as equals, which is
backwards: it lets an invisible background job delay a visible one.

Fix: **split the one semaphore into two independent, fixed-size pools.** Ticks draw only from a
small `tick` pool; every swarm call (planner, each worker, synthesis) draws only from a `swarm`
pool. Neither can block the other — the guarantee the brief asks for, stated as a structural
invariant rather than a hope about timing.

- `TICK_PERMITS = 2`, `SWARM_PERMITS = 5`. `SWARM_PERMITS` = `MAX_ANGLES` so one dig-deeper's own
  angles never queue behind each other (true intra-run parallelism). Total `claude` processes
  alive at once, app-wide, ever: **≤ 7**.
- **No overflow between pools.** Strict separation is simpler than a priority queue or a
  borrow-when-idle scheme, fully testable as two independent counters, and sufficient for the
  actual guarantee. (Overflow/borrowing is an explicit non-goal.)
- **Waiting needs no new code.** `tokio::sync::Semaphore::acquire` already blocks the caller
  until a permit frees, in FIFO order — a documented property of the primitive (verified 3-0),
  not something we build. A third tick simply awaits behind the two running ones.

This replaces the single `agent_sem()` in `agent.rs`, whose doc comment already anticipated ticks
and the swarm sharing one thing — this is that plan realized as two pools instead of one.

### 3. Per-story angle planning (2–5), bounded and fallback-guarded

One buffered, closed-book `claude -p` call (same shape as today's `judge()` — no tools, sandboxed,
reasons only over text we already have in the DB):

```
You are planning a research swarm to dig deeper into one Hacker News story,
for a user whose monitor is interested in: "{monitor_prompt}"

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

`reason` and `summary` already sit in `feed_items` from the tick that produced this match — no
extra HN fetch. Parsing reuses the codebase's existing "find the first `[ … ]`" tolerance
(`parse_verdict`).

`parse_plan` then **clamps and defends** (pure, unit-tested):

- Drop entries with an empty `label` or `focus`.
- If fewer than `MIN_ANGLES` (2) valid entries survive, **return the default 4-angle set**
  (the mock's original: Company & people / Tech & how it works / Market & rivals / Skeptic risks).
- Truncate to `MAX_ANGLES` (5).
- Assign each a stable `id` (uuid) and an **icon in Rust**, cycling a fixed pool
  (`["🏢","🔧","📊","🕵️","🧭"]`) by index — the LLM never emits an emoji, so a bad value can't
  reach the renderer. A user-added angle gets the next icon by its position too.

`plan_angles` therefore **never returns an error** — a failed/timed-out/garbage planning call
resolves to the default set, so the confirm step below always has something to show.

### 4. Human-in-the-loop confirm step (stateless across the two calls)

Between "planned" and "running", the panel shows each angle as a **pill displaying only the short
`label`** (scannable — not the full `focus` sentence), with:

- **✕** to remove — disabled once only 2 remain (`MIN_ANGLES`).
- A text input + **Add** — disabled once 5 are present (`MAX_ANGLES`). Accepts any length: one
  word or a full sentence. Whatever is typed becomes that angle's `focus` **verbatim** (so
  "what's happened in this funding round in the last month" steers the agent precisely); the
  displayed `label` is a short truncated preview of the same text. No extra LLM call for a
  user-added angle.
- **Start research** — confirms the (possibly edited) list and fires the swarm.

**No backend state persists between the two calls.** `start_dig_deeper` runs the planner and
returns the proposal to the frontend; the frontend edits it locally; `confirm_dig_deeper` receives
the final list back and re-loads the item context from the DB by id. The only server-side state is
a registry of *running* swarms (for cancellation, §7). This removes a whole class of stale-plan
bugs and keeps the backend near-stateless.

`confirm_dig_deeper` **re-clamps to 2–5 server-side** — never trusts the frontend's disabled-
button guards alone.

### 5. Fan-out — parallel streaming workers with least-privilege tools

One child process per confirmed angle, run concurrently under the `swarm` pool:

```
claude -p --output-format stream-json --verbose --allowedTools WebSearch WebFetch --model claude-sonnet-5 "<prompt>"
```

Worker prompt:

```
You are one investigator in a research swarm looking into a single HN story,
focused ONLY on this angle: "{angle.focus}"

Story: "{title}" ({url})
Context: this matched a monitor interested in "{monitor_prompt}" because: {reason}

Investigate strictly from your assigned angle — don't try to cover the whole
story. Use web search / fetch to look into the story and related context.
Produce a concise 3-6 sentence findings write-up that stands on its own — it
will be compiled into a combined brief.
```

**Tooling — the security-judgment decision.** Unlike `judge()`, these agents must actually browse,
so `--safe-mode` (which sandboxes the judge call against tool/file access) is wrong here. In
headless `-p` there is no interactive permission prompt, so any tool needing approval is
auto-denied unless allow-listed. Three options weighed:

- `--safe-mode` — rejected: blocks the tool use these agents exist to do.
- `--dangerously-skip-permissions` — rejected: grants **everything** (Bash, Write, Edit) to an
  agent that only needs to read the web. Needless privilege; a poor look for a reviewer.
- **`--allowedTools WebSearch WebFetch`** — chosen: least privilege, exactly the read-only web
  tools a researcher needs, no shell/filesystem. This is the real functional difference between
  the two tempos — ticks are closed-book judging, the swarm is open, tool-using investigation.

The temp-dir / `PWD` isolation from `claude_command()` is kept (harmless — it only affects where
`claude` looks for project settings; web tools don't care).

> **Empirical risk to retire first (see plan Task order).** The exact spelling of `--allowedTools`
> vs `--allowed-tools`, the tool identifiers (`WebSearch`/`WebFetch`), whether the installed CLI +
> account actually grant these tools in `-p`, and whether `--safe-mode` is what would otherwise
> block them, are **unverified** — the research pass hit a session-token limit before reaching this
> angle. Confirm against the real `claude` binary before building the fan-out around it. Fallback
> if web tools prove unavailable in headless: run workers **closed-book** (analysis from the
> model's own knowledge, no `--allowedTools`) — same streaming/synthesis plumbing, weaker "research"
> claim, documented as a limitation.

**Streaming.** Rust reads each child's stdout line-by-line (`AsyncBufReadExt::lines()`). Each line
is newline-delimited JSON; a pure `parse_stream_line` maps it to one of:

- `Progress(String)` — an `assistant` text block (truncated) or a `tool_use` event rendered as a
  short human line (e.g. `🔍 WebSearch: "orbital tax agent funding"`, `📄 WebFetch: techcrunch.com`).
  Forwarded to the lane via a `swarm-progress` event.
- `Final { text, is_error }` — the terminal `{"type":"result",…}` event. Its `result` field is the
  **authoritative final output** for the angle (not a reassembly of deltas). `is_error`/non-zero
  exit → the angle failed.
- `Ignore` — `system`/`user`/unknown lines.

`--include-partial-messages` (token-level deltas) is intentionally **not** used — assistant-block +
tool-use granularity is enough for a meaningful live view and far less noisy to parse. Filed as an
optional later enhancement, not built.

Each worker is an independent `tauri::async_runtime::spawn` task with its own `ANGLE_TIMEOUT_SECS`;
one worker failing or timing out flips only its own lane to `error` and does not short-circuit the
others (they are `join`-ed, never `?`-propagated).

### 6. Synthesis — one LLM call, not a string merge

Once every angle has settled, one buffered call compiles the brief from whatever succeeded:

```
Compile a combined research brief from {N} investigators who each looked at
one HN story from a different angle.

Story: "{title}" ({url})

### {angle1.label}
{angle1.output}
...
[Note: the "{label}" angle could not be completed (timed out).]   <- only for failures

Write: a 2-3 sentence overview, then sections (reuse or reorganize the angle
labels as headings). Return ONLY JSON:
{"summary": "...", "sections": [{"heading": "...", "body": "..."}, ...]}
```

`parse_brief` maps the JSON to `Brief { summary, sections }` — the shape already in `types.ts`, no
new type. If **every** angle failed, synthesis is skipped and the panel shows an error state
directly (`swarm-failed`) rather than synthesizing from nothing.

### 7. Failure handling & cancellation

- **Per-angle isolation** (§5): one lane can fail while the rest finish; synthesis runs on the
  survivors with an explicit gap note. Honest degradation, never a stuck panel.
- **Cancellation** (closing the panel, or clicking Dig deeper on a different item — the panel is a
  single `fixed inset-0` overlay, so at most one run is ever active): `cancel_dig_deeper(item_id)`
  looks up the orchestration task in the `SwarmRegistry` and `abort()`s it. **The worker fan-out
  must use a `tokio::task::JoinSet`, not a `Vec<JoinHandle>`.** This is the subtle correctness point:
  tokio tasks are *not* structured-concurrency children — dropping a bare `JoinHandle` *detaches*
  its task, it does not abort it. So aborting the orchestration task alone would leave the
  per-angle workers running (holding `swarm_sem` permits and live `claude` processes) until their
  own `ANGLE_TIMEOUT_SECS`, and a same-item restart would stall behind them since 5 workers
  saturate the pool. A `JoinSet` fixes this: it is a local of the orchestration task, and
  `JoinSet::drop` **aborts every still-running worker**. So aborting the orchestration task drops
  its `JoinSet`, which aborts each worker future → each drops its `swarm_sem` permit (returned to
  the pool) **and** its `Child` (built by `claude_command()` with `kill_on_drop(true)` → SIGKILL to
  the OS `claude` process). One abort therefore cascades: no leaked permit, no orphan process, no
  stale events after cancel. (The `kill_on_drop` half is the same guarantee the app already relies
  on for tick timeouts; the `JoinSet` half is what makes the *cascade* real.)

## Data model — no new persistence; swarm state is ephemeral

A dig-deeper run lives only as long as its panel: closing and reopening re-runs from scratch. This
is a deliberate non-goal, not an oversight — the requirement asks for a live view + a compiled
brief, not a research-history feature. **No new SQLite tables or columns.**

- **Backend:** one in-memory registry, mirroring `Scheduler.handles` in `scheduler.rs`:

  ```rust
  pub struct SwarmRegistry {
      handles: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>, // item_id -> running swarm
  }
  ```

- **`db::get_feed_item`** (new read): `get_feed_item(conn, id) -> rusqlite::Result<Option<FeedItemContext>>`,
  one row joining `feed_items ⋈ monitors`, returning everything the planner + workers + synthesis
  need:

  ```rust
  pub struct FeedItemContext {
      pub title: String,
      pub url: String,
      pub domain: String,
      pub summary: String,
      pub reason: String,
      pub monitor_prompt: String,
  }
  ```

- **Frontend types (`src/types.ts`):**
  - `AngleStatus` gains `"error"` (currently `"queued" | "running" | "done"`).
  - `SwarmAngle` gains `error?: string`.
  - New `PlannedAngle { id: string; icon: string; label: string; focus: string }` — the shape the
    planner returns and the confirm step sends back.
  - `Brief` / `BriefSection` unchanged.

## Module structure

- **`src-tauri/src/agent.rs`** — the claude runtime primitive + pure prompt/parse logic:
  - `tick_sem()` (2) / `swarm_sem()` (5) replace `agent_sem()`; `judge()` switches to `tick_sem`.
  - Core `PlannedAngle` struct (serde, camelCase); `ANGLE_ICONS`; `default_angles()`.
  - Pure + unit-tested: `build_plan_prompt`, `parse_plan`, `build_investigate_prompt`,
    `build_synthesis_prompt`, `parse_brief`, `parse_stream_line` (→ `StreamLine` enum).
  - Async runtime helpers: `plan_angles(ctx) -> Vec<PlannedAngle>` (buffered, `swarm_sem`, never
    errors → defaults); `stream_investigate(ctx, angle, on_line) -> Result<String, AgentError>`
    (streaming, `swarm_sem`, `--allowedTools`); `synthesize(ctx, results) -> Result<Brief, AgentError>`
    (buffered, `swarm_sem`).
- **`src-tauri/src/swarm.rs`** (new) — orchestration only (side effects, events, registry):
  `SwarmRegistry`; the five `swarm-*` event payload structs; `run_swarm(app, db, item_id, angles)`
  (spawn workers, forward progress, join, synthesize, emit brief/failed); registry insert/abort.
- **`src-tauri/src/commands.rs`** — `AppState` gains `swarm: SwarmRegistry`; three commands
  (`start_dig_deeper`, `confirm_dig_deeper`, `cancel_dig_deeper`).
- **`src-tauri/src/db.rs`** — `FeedItemContext` + `get_feed_item`.
- **`src-tauri/src/lib.rs`** — register the three commands in `generate_handler!`.
- **`src/types.ts`, `src/api.ts`** — new types + command wrappers + five event listeners.
- **`src/components/DigDeeperPanel.tsx`** — phases planning → confirm (editable pills) → running
  (live lanes) → brief; `error` lane styling reuses `text-rust`/`bg-rust` (already used for monitor
  errors in `Sidebar.tsx` — no new tokens).
- **`src/App.tsx`** — drop the `BRIEF_F1` mock import + `brief` prop; the panel self-manages its run.

## Events (Tauri; camelCase, mirroring `tick-started` / `tick-finished`)

| event | payload |
| --- | --- |
| `swarm-progress` | `{ itemId, angleId, line }` |
| `swarm-angle-done` | `{ itemId, angleId, output: string \| null, error: string \| null }` |
| `swarm-brief-ready` | `{ itemId, brief: Brief }` |
| `swarm-failed` | `{ itemId, error }` (every angle failed) |

(The planned angle list is delivered as the **return value** of the `start_dig_deeper` command, not
an event — it's a request/response, not a push. `swarm-planned` from the earlier draft is dropped as
redundant.)

## Constants

| name | value | meaning |
| --- | --- | --- |
| `TICK_PERMITS` | `2` | max concurrent monitor-tick `claude` calls |
| `SWARM_PERMITS` | `5` | max concurrent swarm `claude` calls (= `MAX_ANGLES`) |
| `MIN_ANGLES` | `2` | floor, enforced client- and server-side |
| `MAX_ANGLES` | `5` | ceiling, enforced client- and server-side |
| `PLAN_TIMEOUT_SECS` | `45` | planning call (closed-book, short) |
| `ANGLE_TIMEOUT_SECS` | `150` | per-angle worker (uses web tools — longer than `judge()`'s 90s) |
| `SYNTHESIS_TIMEOUT_SECS` | `90` | matches `judge()`'s existing timeout |

## Non-goals (staying strictly in this ticket)

- **No persistence of swarm runs.** Plan, progress, and brief are ephemeral; closing the panel
  discards them. Not asked for here.
- **No fully agentic/unbounded orchestration.** The planner proposes once; no re-planning loop, no
  mid-run spawning of more agents, no separate citation-verification pass (Anthropic's system has
  one; out of scope for a weekend).
- **No pool overflow / priority queue.** Reserved pools are strictly separate (§2).
- **No token-delta (`--include-partial-messages`) streaming.** Assistant-block + tool-use
  granularity is sufficient (§5).
- **No cross-item/cross-monitor caching.** Each dig-deeper click is independent, even for the same
  story twice.

## Testing

- **Rust unit tests (pure functions — the existing `parse_verdict` / `find_claude` seam style):**
  - `parse_plan`: a valid 2–5 array passes through with icons assigned by index; `< 2` valid
    entries after filtering → default set; malformed JSON → default set; entries missing
    `label`/`focus` dropped before the count check; `> 5` truncated to 5.
  - `default_angles()` returns exactly 4 well-formed angles with distinct icons.
  - `parse_brief`: valid JSON → `Brief`; prose-wrapped JSON tolerated; garbage → an `Err`/empty
    handled by the caller.
  - `parse_stream_line`: an `assistant` text line → `Progress`; a `tool_use` line → `Progress`
    with a tool label; the `result` line → `Final { text, is_error }`; a `system` line → `Ignore`;
    non-JSON → `Ignore` (never panics).
  - `build_*_prompt`: each contains the story title, the monitor prompt, and (workers) the angle
    focus.
  - `tick_sem` / `swarm_sem` are independent: exhausting all of one leaves the other immediately
    acquirable.
  - `db::get_feed_item`: round-trips an inserted item + joined monitor prompt; missing id → `None`.
- **Build order (front-load the risk).** Verify the `--allowedTools` flag against the real CLI
  **first** (§5 empirical risk). Then build the streaming + synthesis + two-pool path against a
  **fixed** angle set — the riskiest plumbing (stream-json parsing, event forwarding, concurrency,
  cancellation). Add the dynamic planner + human-in-the-loop confirm **last** — simplest to add,
  easiest to drop if time runs out.
- **Live verification** in the native window per [`docs/TESTING.md`](../../TESTING.md): click dig
  deeper on a real feed item → planner proposes story-appropriate angles → edit (remove one, add a
  custom sentence) → Start → lanes stream live tool-use/progress independently → force one lane to
  fail (bad `HN_WATCH_CLAUDE_BIN` or a nonsense angle) → a degraded brief still compiles, noting the
  gap → close the panel mid-run and confirm via `ps` that the child `claude` processes are actually
  killed → a monitor tick firing during an active swarm runs without delay (two pools holding).

## Files touched

- `src-tauri/src/agent.rs` — two semaphores; `PlannedAngle`, `ANGLE_ICONS`, `default_angles`;
  `build_plan_prompt`/`parse_plan`, `build_investigate_prompt`, `build_synthesis_prompt`/
  `parse_brief`, `parse_stream_line`; async `plan_angles`/`stream_investigate`/`synthesize`;
  streaming `claude` invocation.
- `src-tauri/src/swarm.rs` (new) — `SwarmRegistry`, event payloads, `run_swarm`, cancellation.
- `src-tauri/src/db.rs` — `FeedItemContext`, `get_feed_item`.
- `src-tauri/src/commands.rs` — `AppState.swarm`; `start_dig_deeper`/`confirm_dig_deeper`/
  `cancel_dig_deeper`.
- `src-tauri/src/lib.rs` — register the three commands.
- `src/types.ts` — `AngleStatus` +`"error"`; `SwarmAngle.error?`; `PlannedAngle`.
- `src/api.ts` — command wrappers + five swarm event listeners.
- `src/components/DigDeeperPanel.tsx` — planning/confirm/running/brief phases; editable pills;
  `error` lane styling (existing tokens).
- `src/App.tsx` — remove the `BRIEF_F1` mock wiring; panel self-manages its run.
