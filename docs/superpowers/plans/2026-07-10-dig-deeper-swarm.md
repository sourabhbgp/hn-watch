# Dig-Deeper Research Swarm Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the mock "Dig deeper" panel into a real Rust-orchestrated research swarm: a per-story planner picks 2–5 investigative angles, the user edits them, several `claude -p` agents investigate in parallel (streaming live), and one synthesis call compiles a combined brief — all sharing the `claude` runtime with monitor ticks via two reserved concurrency pools.

**Architecture:** Orchestrator-worker, Rust-owned. `agent.rs` holds the pure prompt/parse logic + three async runtime helpers (`plan_angles`, `stream_investigate`, `synthesize`) and two reserved semaphores (`tick_sem`=2, `swarm_sem`=5). `swarm.rs` orchestrates: load item context, fan out one streaming `claude -p` worker per angle, forward progress as Tauri events, join, synthesize, emit the brief. The frontend panel drives a `plan → confirm(edit) → run → brief` flow; closing it cancels the run (one `abort()` releases each worker's permit and kills its `kill_on_drop` child).

**Tech Stack:** Rust (Tauri v2 core), `tokio` (process spawn + `Semaphore` + `AsyncBufReadExt`), `rusqlite`, `serde`, `uuid`; React 19 + TypeScript + Tailwind v4 frontend. Rust pure logic is `#[cfg(test)]` unit-tested; process/streaming I/O is verified live per `docs/TESTING.md`.

## Global Constraints

- **Scope source of truth:** `docs/REQUIREMENTS.md`; design: `docs/superpowers/specs/2026-07-10-dig-deeper-swarm-design.md`.
- **Constants (exact values):** `TICK_PERMITS = 2`, `SWARM_PERMITS = 5`, `MIN_ANGLES = 2`, `MAX_ANGLES = 5`, `PLAN_TIMEOUT_SECS = 45`, `ANGLE_TIMEOUT_SECS = 150`, `SYNTHESIS_TIMEOUT_SECS = 90`, `ANGLE_ICONS = ["🏢","🔧","📊","🕵️","🧭"]`.
- **Model:** every swarm `claude -p` call passes `--model claude-sonnet-5` (matches the pinned tick model, Session 12).
- **No new persistence** — swarm state is in-memory/ephemeral. No SQLite schema change.
- **No new design tokens** — reuse `text-rust`/`bg-rust`/`hn-soft`/`bg-card`/`border-line` etc. (see `src/index.css`, `docs/design.md`).
- **Pure decision logic is unit-tested** (mirror the `parse_verdict`/`find_claude` seam); process + `claude` I/O (`plan_angles`, `stream_investigate`, `synthesize`, `run_swarm`) is verified live, not unit-tested.
- **Rust test command:** `cargo test --manifest-path src-tauri/Cargo.toml` — all must pass (37 exist today; this plan adds more).
- **Rust build check:** `cargo build --manifest-path src-tauri/Cargo.toml` — no errors, no warnings.
- **Frontend build check:** `npm run build` (tsc + vite) — clean.
- **Branch:** `feat/dig-deeper-swarm` (already created; spec already committed).
- **Commit trailer:** end every commit message with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

## File Structure

- `src-tauri/src/agent.rs` — two semaphores replace `agent_sem`; `PlannedAngle`, `ANGLE_ICONS`, `default_angles`; pure `build_plan_prompt`/`parse_plan`/`build_investigate_prompt`/`build_synthesis_prompt`/`parse_brief`/`parse_stream_line`; async `plan_angles`/`stream_investigate`/`synthesize`; a streaming `claude` spawn helper.
- `src-tauri/src/db.rs` — `FeedItemContext` + `get_feed_item`.
- `src-tauri/src/swarm.rs` (new) — `SwarmRegistry`, five `swarm-*` event payloads, `run_swarm`, cancellation.
- `src-tauri/src/commands.rs` — `AppState.swarm`; `start_dig_deeper`/`confirm_dig_deeper`/`cancel_dig_deeper`.
- `src-tauri/src/lib.rs` — declare `mod swarm`; register the three commands.
- `src/types.ts` — `AngleStatus` +`"error"`; `SwarmAngle.error?`; `PlannedAngle`.
- `src/api.ts` — three command wrappers + four swarm event listeners.
- `src/components/DigDeeperPanel.tsx` — planning/confirm/running/brief phases; editable pills; error lanes.
- `src/App.tsx` — drop `BRIEF_F1` mock wiring; panel self-manages via `key={digItem.id}`.

---

### Task 1: Empirical spike — verify `claude -p` streaming + tool flags (de-risk before building)

**Files:** none (a spike — record findings in the commit message / a scratch note; adjust later tasks if reality differs).

**Why first:** the whole fan-out is built around `--output-format stream-json` and `--allowedTools`. Their exact spelling/behavior is unverified (the research pass ran out of tokens). Retire that risk before writing code that assumes it.

- [ ] **Step 1: Confirm the streaming JSON shape**

Run (a prompt that will emit at least one tool call):

```bash
claude -p --output-format stream-json --verbose --model claude-sonnet-5 \
  "In one sentence, what is the Rust tokio crate?"
```

Expected: multiple newline-delimited JSON objects, each with a `"type"` field. Note the exact values you see (`system`, `assistant`, `result`, …) and, for `assistant`, the `message.content[].type` values (`text`, `tool_use`). Confirm the terminal line is `{"type":"result", ... "result":"<final text>", "is_error":false, ...}`. **Record the real field names** — `parse_stream_line` (Task 5) keys on them.

- [ ] **Step 2: Confirm the tool-permission flag**

Run (forces a web tool; must NOT hang on an interactive permission prompt):

```bash
claude -p --output-format stream-json --verbose --model claude-sonnet-5 \
  --allowedTools WebSearch WebFetch \
  "Search the web for the latest tokio release version and cite the source."
```

Expected: exits non-interactively; the stream shows a `tool_use` for `WebSearch`/`WebFetch` and a real answer. If the flag name errors, try `--allowed-tools`; note which works. If the account/CLI cannot use web tools in `-p` at all, record that — Task 6 then falls back to **closed-book** workers (drop `--allowedTools`, keep everything else; document the limitation in `STATUS.md`).

- [ ] **Step 3: Record the outcome**

Write a one-paragraph note (in the branch's first code commit body, or `docs/superpowers/specs/…`'s "empirical risk" section) stating: the confirmed `--output-format stream-json` line types, the working tool-allow flag spelling, and whether web tools work in `-p`. **No code commit in this task** — it gates the flag literals used in Tasks 5–6.

---

### Task 2: `agent.rs` — split the shared semaphore into two reserved pools

**Files:**
- Modify: `src-tauri/src/agent.rs`
- Test: `src-tauri/src/agent.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `fn tick_sem() -> &'static Semaphore` (2 permits), `fn swarm_sem() -> &'static Semaphore` (5 permits); constants `TICK_PERMITS`/`SWARM_PERMITS`. Removes `agent_sem`.
- Consumes: nothing.

- [ ] **Step 1: Replace `agent_sem` with two pools**

In `src-tauri/src/agent.rs`, replace the `agent_sem` function (lines ~7–12) with:

```rust
/// Reserved-pool concurrency for the shared `claude` runtime. Monitor ticks draw only
/// from `tick_sem`; the dig-deeper swarm (planner, each worker, synthesis) draws only
/// from `swarm_sem`. Strict separation (no overflow) means an interactive swarm never
/// queues behind background ticks, and ticks are never blocked by an in-flight swarm.
/// Both are FIFO-fair, so a third caller of a full pool simply waits its turn.
pub const TICK_PERMITS: usize = 2;
pub const SWARM_PERMITS: usize = 5;

fn tick_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(TICK_PERMITS))
}

fn swarm_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(SWARM_PERMITS))
}
```

- [ ] **Step 2: Point `judge()` at `tick_sem`**

In `judge()` (~line 275), change the permit acquisition from `agent_sem()` to `tick_sem()`:

```rust
    let _permit = tick_sem()
        .acquire()
        .await
        .map_err(|e| AgentError::Failed(format!("semaphore closed: {e}")))?;
```

- [ ] **Step 3: Write the pool-independence test**

Add to `src-tauri/src/agent.rs` `#[cfg(test)] mod tests`:

```rust
    #[tokio::test]
    async fn pools_are_independent() {
        // Exhaust the tick pool entirely...
        let mut held = Vec::new();
        for _ in 0..TICK_PERMITS {
            held.push(tick_sem().acquire().await.unwrap());
        }
        assert_eq!(tick_sem().available_permits(), 0);
        // ...the swarm pool is untouched and still fully available.
        assert_eq!(swarm_sem().available_permits(), SWARM_PERMITS);
        let _s = swarm_sem().try_acquire().expect("swarm pool must be free while ticks are saturated");
    }
```

> The crate already builds tokio with the `macros`/`rt` features (used elsewhere); `#[tokio::test]` is available. If a plain `#[test]` is preferred, wrap the body in `tauri::async_runtime::block_on(async { … })`.

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — `pools_are_independent` green; existing agent tests unchanged.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/agent.rs
git commit -m "feat(agent): split shared semaphore into reserved tick/swarm pools

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `db.rs` — `FeedItemContext` + `get_feed_item`

**Files:**
- Modify: `src-tauri/src/db.rs`
- Test: `src-tauri/src/db.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `pub struct FeedItemContext { title, url, domain, summary, reason, monitor_prompt }`; `pub fn get_feed_item(conn: &Connection, id: &str) -> rusqlite::Result<Option<FeedItemContext>>`.
- Consumes: nothing.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/db.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn get_feed_item_joins_monitor_prompt() {
        let c = mem();
        insert_monitor(&c, &sample_monitor("m1")).unwrap(); // prompt = "ai agents"
        insert_feed_item(&c, &FeedRow {
            id: "f1".into(), monitor_id: "m1".into(), hn_id: "hn1".into(),
            title: "Orbital launches".into(), url: "https://x.dev/a".into(), domain: "x.dev".into(),
            summary: "an agent".into(), reason: "matches".into(),
            hn_score: 10, hn_comments: 2, created_at: 200,
        }).unwrap();

        let ctx = get_feed_item(&c, "f1").unwrap().expect("item exists");
        assert_eq!(ctx.title, "Orbital launches");
        assert_eq!(ctx.url, "https://x.dev/a");
        assert_eq!(ctx.summary, "an agent");
        assert_eq!(ctx.reason, "matches");
        assert_eq!(ctx.monitor_prompt, "ai agents"); // joined from monitors

        assert!(get_feed_item(&c, "nope").unwrap().is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml get_feed_item_joins_monitor_prompt`
Expected: FAIL — `cannot find function get_feed_item` / `cannot find type FeedItemContext`.

- [ ] **Step 3: Implement `FeedItemContext` + `get_feed_item`**

In `src-tauri/src/db.rs`, add the struct near `FeedRow` (top of file):

```rust
/// Everything the dig-deeper swarm needs about one feed item: the story fields plus
/// the owning monitor's prompt (so workers know what the user cares about).
#[derive(Debug, Clone)]
pub struct FeedItemContext {
    pub title: String,
    pub url: String,
    pub domain: String,
    pub summary: String,
    pub reason: String,
    pub monitor_prompt: String,
}
```

Add the getter (near `list_feed`):

```rust
/// Load one feed item + its monitor's prompt by feed-item id. `None` if the id is unknown.
pub fn get_feed_item(conn: &Connection, id: &str) -> rusqlite::Result<Option<FeedItemContext>> {
    let mut stmt = conn.prepare(
        "SELECT f.title, f.url, f.domain, f.summary, f.reason, m.prompt
         FROM feed_items f JOIN monitors m ON m.id = f.monitor_id
         WHERE f.id = ?1",
    )?;
    let mut rows = stmt.query_map([id], |r| {
        Ok(FeedItemContext {
            title: r.get(0)?,
            url: r.get(1)?,
            domain: r.get(2)?,
            summary: r.get(3)?,
            reason: r.get(4)?,
            monitor_prompt: r.get(5)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml get_feed_item_joins_monitor_prompt`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "feat(db): FeedItemContext + get_feed_item (story + monitor prompt by id)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `agent.rs` — planner types + pure `build_plan_prompt` / `parse_plan`

**Files:**
- Modify: `src-tauri/src/agent.rs`
- Test: `src-tauri/src/agent.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `crate::db::FeedItemContext` (Task 3).
- Produces: `pub struct PlannedAngle { id, icon, label, focus }` (serde camelCase); consts `MIN_ANGLES`/`MAX_ANGLES`/`ANGLE_ICONS`; `pub fn default_angles() -> Vec<PlannedAngle>`; `pub fn build_plan_prompt(ctx: &FeedItemContext) -> String`; `pub fn parse_plan(text: &str) -> Vec<PlannedAngle>`.

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/src/agent.rs` `#[cfg(test)] mod tests`:

```rust
    fn ctx() -> crate::db::FeedItemContext {
        crate::db::FeedItemContext {
            title: "Orbital (YC W26) files your taxes".into(),
            url: "https://news.ycombinator.com/item?id=1".into(),
            domain: "news.ycombinator.com".into(),
            summary: "an agent that prepares tax returns".into(),
            reason: "AI-agent startup launch".into(),
            monitor_prompt: "AI-agent startup launches".into(),
        }
    }

    #[test]
    fn build_plan_prompt_contains_story_and_interest() {
        let p = build_plan_prompt(&ctx());
        assert!(p.contains("AI-agent startup launches")); // monitor prompt
        assert!(p.contains("Orbital (YC W26) files your taxes")); // title
        assert!(p.contains("2 and 5")); // the 2–5 instruction
    }

    #[test]
    fn parse_plan_accepts_valid_and_assigns_icons_by_index() {
        let text = r#"[
          {"label":"Company","focus":"who founded it"},
          {"label":"Tech","focus":"how it works"},
          {"label":"Market","focus":"competitors"}
        ]"#;
        let angles = parse_plan(text);
        assert_eq!(angles.len(), 3);
        assert_eq!(angles[0].label, "Company");
        assert_eq!(angles[0].icon, ANGLE_ICONS[0]);
        assert_eq!(angles[1].icon, ANGLE_ICONS[1]);
        assert!(!angles[0].id.is_empty()); // uuid assigned
    }

    #[test]
    fn parse_plan_drops_empty_entries_then_may_fall_back() {
        // Only one valid entry survives filtering (< MIN_ANGLES) -> default set.
        let text = r#"[{"label":"","focus":"x"},{"label":"Only","focus":""},{"label":"Keep","focus":"real"}]"#;
        let angles = parse_plan(text);
        assert_eq!(angles.len(), default_angles().len()); // fell back
    }

    #[test]
    fn parse_plan_truncates_to_max() {
        let text = r#"[
          {"label":"a","focus":"1"},{"label":"b","focus":"2"},{"label":"c","focus":"3"},
          {"label":"d","focus":"4"},{"label":"e","focus":"5"},{"label":"f","focus":"6"},{"label":"g","focus":"7"}
        ]"#;
        assert_eq!(parse_plan(text).len(), MAX_ANGLES); // 5
    }

    #[test]
    fn parse_plan_garbage_falls_back() {
        assert_eq!(parse_plan("not json").len(), default_angles().len());
        assert_eq!(parse_plan("[]").len(), default_angles().len());
    }

    #[test]
    fn default_angles_are_wellformed_and_distinct() {
        let a = default_angles();
        assert_eq!(a.len(), 4);
        assert!(a.iter().all(|x| !x.label.is_empty() && !x.focus.is_empty() && !x.id.is_empty()));
        // distinct icons for the first 4
        assert_eq!(a[0].icon, ANGLE_ICONS[0]);
        assert_eq!(a[3].icon, ANGLE_ICONS[3]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — unresolved `PlannedAngle` / `build_plan_prompt` / `parse_plan` / `default_angles` / `ANGLE_ICONS`.

- [ ] **Step 3: Implement the types + pure functions**

In `src-tauri/src/agent.rs`, add (below the `use` lines add `use crate::db::FeedItemContext;` and `use uuid::Uuid;`; the crate already depends on `uuid` — see `commands.rs`):

```rust
/// Number of investigative angles a swarm may run — enforced client- and server-side.
pub const MIN_ANGLES: usize = 2;
pub const MAX_ANGLES: usize = 5;
/// Icon pool, assigned to angles by index (the LLM never emits an emoji).
pub const ANGLE_ICONS: [&str; 5] = ["🏢", "🔧", "📊", "🕵️", "🧭"];

/// One investigative angle: what a single swarm worker will look into.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAngle {
    pub id: String,
    pub icon: String,
    pub label: String,
    pub focus: String,
}

/// Raw planner output before clamping/icon assignment.
#[derive(serde::Deserialize)]
struct RawAngle {
    #[serde(default)]
    label: String,
    #[serde(default)]
    focus: String,
}

/// Build a `PlannedAngle` from a label+focus, assigning the icon at `index` (mod pool size)
/// and a fresh uuid.
fn angle_at(index: usize, label: String, focus: String) -> PlannedAngle {
    PlannedAngle {
        id: Uuid::new_v4().to_string(),
        icon: ANGLE_ICONS[index % ANGLE_ICONS.len()].to_string(),
        label,
        focus,
    }
}

/// Fallback angle set when the planner fails or returns too few usable angles.
pub fn default_angles() -> Vec<PlannedAngle> {
    [
        ("Company & people", "Who is behind the story — founders, team, backers, org."),
        ("Tech & how it works", "The underlying technology and how it actually works."),
        ("Market & rivals", "The market, competitors, and how this compares."),
        ("Skeptic / risks", "Risks, criticisms, and reasons for skepticism."),
    ]
    .iter()
    .enumerate()
    .map(|(i, (l, f))| angle_at(i, (*l).to_string(), (*f).to_string()))
    .collect()
}

pub fn build_plan_prompt(ctx: &FeedItemContext) -> String {
    format!(
        "You are planning a research swarm to dig deeper into one Hacker News story, \
         for a user whose monitor is interested in:\n\"{prompt}\"\n\n\
         Story: \"{title}\" ({domain}, {url})\n\
         Why it matched: {reason}\n\
         Initial summary: {summary}\n\n\
         Decide between 2 and 5 distinct investigative angles for THIS SPECIFIC STORY. \
         Each angle should pull from genuinely different context or sources — do not force a \
         generic template if it doesn't fit.\n\n\
         Return ONLY a JSON array (2 to 5 elements, no prose, no markdown fences) of objects \
         with exactly these keys: \"label\" (short 2-4 word angle name) and \"focus\" (one \
         sentence telling an investigator exactly what to look into).",
        prompt = ctx.monitor_prompt,
        title = ctx.title,
        domain = ctx.domain,
        url = ctx.url,
        reason = ctx.reason,
        summary = ctx.summary,
    )
}

/// Parse the planner's JSON array into clamped, icon-assigned angles. Tolerant like
/// `parse_verdict` (finds the first `[ … ]`). Drops entries with an empty label/focus;
/// if fewer than `MIN_ANGLES` survive (or the text is unparseable), returns `default_angles()`;
/// truncates to `MAX_ANGLES`.
pub fn parse_plan(text: &str) -> Vec<PlannedAngle> {
    let slice = match (text.find('['), text.rfind(']')) {
        (Some(s), Some(e)) if e > s => &text[s..=e],
        _ => return default_angles(),
    };
    let raw: Vec<RawAngle> = serde_json::from_str(slice).unwrap_or_default();
    let cleaned: Vec<PlannedAngle> = raw
        .into_iter()
        .filter(|a| !a.label.trim().is_empty() && !a.focus.trim().is_empty())
        .take(MAX_ANGLES)
        .enumerate()
        .map(|(i, a)| angle_at(i, a.label.trim().to_string(), a.focus.trim().to_string()))
        .collect();
    if cleaned.len() < MIN_ANGLES {
        default_angles()
    } else {
        cleaned
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — all six new planner tests green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/agent.rs
git commit -m "feat(agent): PlannedAngle + build_plan_prompt + parse_plan (clamped, fallback)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `agent.rs` — pure worker/synthesis prompts + `parse_brief` + `parse_stream_line`

**Files:**
- Modify: `src-tauri/src/agent.rs`
- Test: `src-tauri/src/agent.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `FeedItemContext`, `PlannedAngle` (Task 4).
- Produces: `pub struct Brief { summary, sections }` + `pub struct BriefSection { heading, body }` (serde camelCase); `pub fn build_investigate_prompt(ctx, angle) -> String`; `pub fn build_synthesis_prompt(ctx, results: &[(PlannedAngle, Option<String>)]) -> String`; `pub fn parse_brief(text: &str) -> Option<Brief>`; `pub enum StreamLine { Progress(String), Final { text, is_error }, Ignore }`; `pub fn parse_stream_line(line: &str) -> StreamLine`.

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/src/agent.rs` `#[cfg(test)] mod tests` (reuses the `ctx()` helper from Task 4):

```rust
    fn sample_angle() -> PlannedAngle {
        angle_at(0, "Funding".into(), "the funding round and investors".into())
    }

    #[test]
    fn build_investigate_prompt_contains_focus_and_story() {
        let p = build_investigate_prompt(&ctx(), &sample_angle());
        assert!(p.contains("the funding round and investors")); // focus
        assert!(p.contains("Orbital (YC W26) files your taxes")); // title
        assert!(p.contains("AI-agent startup launches")); // monitor interest
    }

    #[test]
    fn build_synthesis_prompt_notes_failures() {
        let results = vec![
            (sample_angle(), Some("Raised $4M seed.".to_string())),
            (angle_at(1, "Market".into(), "rivals".into()), None), // failed
        ];
        let p = build_synthesis_prompt(&ctx(), &results);
        assert!(p.contains("Raised $4M seed.")); // succeeded angle's output
        assert!(p.contains("Funding")); // its label as a heading
        assert!(p.contains("could not be completed")); // the failure note
    }

    #[test]
    fn parse_brief_reads_summary_and_sections() {
        let text = r#"Sure:
        {"summary":"A tax agent.","sections":[{"heading":"What","body":"It files taxes."}]}"#;
        let b = parse_brief(text).expect("parses");
        assert_eq!(b.summary, "A tax agent.");
        assert_eq!(b.sections.len(), 1);
        assert_eq!(b.sections[0].heading, "What");
    }

    #[test]
    fn parse_brief_garbage_is_none() {
        assert!(parse_brief("no json").is_none());
        assert!(parse_brief("{}").map(|b| b.summary).unwrap_or_default().is_empty());
    }

    #[test]
    fn parse_stream_line_classifies() {
        // assistant text block -> Progress
        let text = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Looking into funding"}]}}"#;
        assert_eq!(parse_stream_line(text), StreamLine::Progress("Looking into funding".into()));

        // tool_use -> Progress with a tool label
        let tool = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"WebSearch","input":{"query":"orbital yc"}}]}}"#;
        match parse_stream_line(tool) {
            StreamLine::Progress(s) => assert!(s.contains("WebSearch")),
            other => panic!("expected Progress, got {other:?}"),
        }

        // result -> Final
        let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"Final findings."}"#;
        assert_eq!(
            parse_stream_line(result),
            StreamLine::Final { text: "Final findings.".into(), is_error: false }
        );

        // system + non-json -> Ignore
        assert_eq!(parse_stream_line(r#"{"type":"system","subtype":"init"}"#), StreamLine::Ignore);
        assert_eq!(parse_stream_line("not json"), StreamLine::Ignore);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: FAIL — unresolved `Brief` / `build_investigate_prompt` / `build_synthesis_prompt` / `parse_brief` / `StreamLine` / `parse_stream_line`.

- [ ] **Step 3: Implement the types + pure functions**

In `src-tauri/src/agent.rs`, add:

```rust
/// Compiled research brief — matches the frontend `Brief` (summary + sections). The panel
/// supplies itemId/angles itself, so the payload only needs these two.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub summary: String,
    pub sections: Vec<BriefSection>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefSection {
    pub heading: String,
    pub body: String,
}

/// Deserialize target for `parse_brief` (Brief is serialize-only for the event).
#[derive(serde::Deserialize)]
struct RawBrief {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    sections: Vec<BriefSection>,
}

pub fn build_investigate_prompt(ctx: &FeedItemContext, angle: &PlannedAngle) -> String {
    format!(
        "You are one investigator in a research swarm looking into a single HN story, \
         focused ONLY on this angle:\n\"{focus}\"\n\n\
         Story: \"{title}\" ({url})\n\
         Context: this matched a monitor interested in \"{prompt}\" because: {reason}\n\n\
         Investigate strictly from your assigned angle — don't try to cover the whole story. \
         Use web search / fetch to look into the story and related context. Produce a concise \
         3-6 sentence findings write-up that stands on its own — it will be compiled into a \
         combined brief.",
        focus = angle.focus,
        title = ctx.title,
        url = ctx.url,
        prompt = ctx.monitor_prompt,
        reason = ctx.reason,
    )
}

pub fn build_synthesis_prompt(ctx: &FeedItemContext, results: &[(PlannedAngle, Option<String>)]) -> String {
    let mut body = String::new();
    for (angle, output) in results {
        match output {
            Some(text) => body.push_str(&format!("\n### {}\n{}\n", angle.label, text)),
            None => body.push_str(&format!(
                "\n[Note: the \"{}\" angle could not be completed (timed out or failed).]\n",
                angle.label
            )),
        }
    }
    format!(
        "Compile a combined research brief from {n} investigators who each looked at one HN \
         story from a different angle.\n\n\
         Story: \"{title}\" ({url})\n{body}\n\
         Write: a 2-3 sentence overview, then sections (reuse or reorganize the angle labels as \
         headings). Return ONLY JSON (no prose, no markdown fences): \
         {{\"summary\": \"...\", \"sections\": [{{\"heading\": \"...\", \"body\": \"...\"}}]}}",
        n = results.len(),
        title = ctx.title,
        url = ctx.url,
        body = body,
    )
}

/// Parse the synthesis JSON object (tolerant: finds the first `{ … }`). `None` if no object
/// is found; an object missing keys yields empty defaults.
pub fn parse_brief(text: &str) -> Option<Brief> {
    let slice = match (text.find('{'), text.rfind('}')) {
        (Some(s), Some(e)) if e > s => &text[s..=e],
        _ => return None,
    };
    let raw: RawBrief = serde_json::from_str(slice).ok()?;
    Some(Brief { summary: raw.summary, sections: raw.sections })
}

/// One parsed line of `--output-format stream-json`.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamLine {
    /// A human-readable progress line for the live lane.
    Progress(String),
    /// The terminal result event: the authoritative final output for the angle.
    Final { text: String, is_error: bool },
    /// system / user / unknown / non-JSON — nothing to show.
    Ignore,
}

/// Truncate a progress line so a chatty model can't flood the UI.
fn truncate_progress(s: &str) -> String {
    const MAX: usize = 160;
    let s = s.trim();
    if s.chars().count() > MAX {
        format!("{}…", s.chars().take(MAX).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Map one stream-json line to a `StreamLine`. Never panics on malformed input.
/// NOTE: field names verified in Task 1 — adjust here if the real CLI differs.
pub fn parse_stream_line(line: &str) -> StreamLine {
    let v: serde_json::Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => return StreamLine::Ignore,
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => {
            let blocks = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array());
            if let Some(blocks) = blocks {
                for b in blocks {
                    match b.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                                if !t.trim().is_empty() {
                                    return StreamLine::Progress(truncate_progress(t));
                                }
                            }
                        }
                        Some("tool_use") => {
                            let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                            // surface the most useful input value (query or url) if present
                            let detail = b
                                .get("input")
                                .and_then(|i| i.get("query").or_else(|| i.get("url")))
                                .and_then(|x| x.as_str())
                                .unwrap_or("");
                            let line = if detail.is_empty() {
                                format!("⚙ {name}")
                            } else {
                                format!("⚙ {name}: {detail}")
                            };
                            return StreamLine::Progress(truncate_progress(&line));
                        }
                        _ => {}
                    }
                }
            }
            StreamLine::Ignore
        }
        Some("result") => {
            let text = v.get("result").and_then(|r| r.as_str()).unwrap_or("").to_string();
            let is_error = v.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
            StreamLine::Final { text, is_error }
        }
        _ => StreamLine::Ignore,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — all five new tests green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/agent.rs
git commit -m "feat(agent): worker/synthesis prompts + parse_brief + parse_stream_line (pure)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `agent.rs` — async runtime helpers (`plan_angles`, `stream_investigate`, `synthesize`)

**Files:**
- Modify: `src-tauri/src/agent.rs`
- Test: none new (process I/O — verified live). Deliverable = crate compiles clean.

**Interfaces:**
- Consumes: all Task 4/5 pure fns; `swarm_sem`; `claude_command`; `AgentError`; `is_auth_failure`.
- Produces: `pub async fn plan_angles(ctx: &FeedItemContext) -> Vec<PlannedAngle>`; `pub async fn stream_investigate(ctx: &FeedItemContext, angle: &PlannedAngle, on_progress: impl Fn(String)) -> Result<String, AgentError>`; `pub async fn synthesize(ctx: &FeedItemContext, results: &[(PlannedAngle, Option<String>)]) -> Result<Brief, AgentError>`.

- [ ] **Step 1: Add the buffered swarm helpers (`plan_angles`, `synthesize`)**

In `src-tauri/src/agent.rs`, add. `run_buffered_swarm` factors the shared "acquire swarm permit → run a buffered `claude -p` with the given args → return stdout or classified error" path:

```rust
/// Run one buffered swarm `claude -p` call (planner / synthesis). Acquires a `swarm_sem`
/// permit, applies `timeout_secs`, and classifies failures like `judge()`. `extra_args` lets
/// callers add flags (none today — planner + synthesis are closed-book, no `--allowedTools`).
async fn run_buffered_swarm(prompt: &str, timeout_secs: u64) -> Result<String, AgentError> {
    let _permit = swarm_sem()
        .acquire()
        .await
        .map_err(|e| AgentError::Failed(format!("semaphore closed: {e}")))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        claude_command()
            .arg("-p")
            .arg("--safe-mode")
            .arg("--model")
            .arg("claude-sonnet-5")
            .arg(prompt)
            .output(),
    )
    .await
    .map_err(|_| AgentError::Timeout)?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AgentError::NotFound
        } else {
            AgentError::Failed(format!("failed to spawn claude: {e}"))
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if is_auth_failure(&stderr) {
            AgentError::NotAuthenticated
        } else {
            AgentError::Failed(format!("claude exited with status {}: {stderr}", output.status))
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Plan the investigative angles for a story. Never errors: any failure (missing/logged-out
/// claude, timeout, garbage output) resolves to `default_angles()` so the confirm UI always
/// has a proposal to show.
pub async fn plan_angles(ctx: &FeedItemContext) -> Vec<PlannedAngle> {
    match run_buffered_swarm(&build_plan_prompt(ctx), PLAN_TIMEOUT_SECS).await {
        Ok(text) => parse_plan(&text),
        Err(_) => default_angles(),
    }
}

/// Compile the combined brief from the per-angle results (`Some` = output, `None` = failed).
pub async fn synthesize(
    ctx: &FeedItemContext,
    results: &[(PlannedAngle, Option<String>)],
) -> Result<Brief, AgentError> {
    let text = run_buffered_swarm(&build_synthesis_prompt(ctx, results), SYNTHESIS_TIMEOUT_SECS).await?;
    parse_brief(&text).ok_or_else(|| AgentError::Failed("could not parse brief JSON".into()))
}
```

Add the timeout constants near the other swarm consts:

```rust
const PLAN_TIMEOUT_SECS: u64 = 45;
const ANGLE_TIMEOUT_SECS: u64 = 150;
const SYNTHESIS_TIMEOUT_SECS: u64 = 90;
```

- [ ] **Step 2: Add the streaming worker (`stream_investigate`)**

Add these imports at the top of `agent.rs` (with the existing `use` block):

```rust
use tokio::io::{AsyncBufReadExt, BufReader};
```

Then the streaming helper:

```rust
/// Run one investigative worker with live streaming. Acquires a `swarm_sem` permit, spawns
/// `claude -p --output-format stream-json …` with web tools allow-listed (least privilege),
/// reads stdout line-by-line, forwards each progress line via `on_progress`, and returns the
/// authoritative final text from the terminal `result` event. Times out at `ANGLE_TIMEOUT_SECS`.
///
/// NOTE (Task 1): if web tools are unavailable in headless `-p`, drop the two `--allowedTools`
/// args to run closed-book — the rest is unchanged.
pub async fn stream_investigate(
    ctx: &FeedItemContext,
    angle: &PlannedAngle,
    on_progress: impl Fn(String),
) -> Result<String, AgentError> {
    let _permit = swarm_sem()
        .acquire()
        .await
        .map_err(|e| AgentError::Failed(format!("semaphore closed: {e}")))?;
    let prompt = build_investigate_prompt(ctx, angle);

    let mut child = claude_command()
        .arg("-p")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--allowedTools")
        .arg("WebSearch")
        .arg("WebFetch")
        .arg("--model")
        .arg("claude-sonnet-5")
        .arg(&prompt)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AgentError::NotFound
            } else {
                AgentError::Failed(format!("failed to spawn claude: {e}"))
            }
        })?;

    let stdout = child.stdout.take().ok_or_else(|| AgentError::Failed("no stdout".into()))?;

    // Drive the whole read under one timeout; on timeout the `child` drops (kill_on_drop) at fn exit.
    let read = async {
        let mut lines = BufReader::new(stdout).lines();
        let mut final_text: Option<String> = None;
        while let Ok(Some(line)) = lines.next_line().await {
            match parse_stream_line(&line) {
                StreamLine::Progress(p) => on_progress(p),
                StreamLine::Final { text, is_error } => {
                    if is_error {
                        return Err(AgentError::Failed("agent reported an error".into()));
                    }
                    final_text = Some(text);
                }
                StreamLine::Ignore => {}
            }
        }
        // Reap the process; a non-zero exit with no result line is a failure.
        let status = child.wait().await.map_err(|e| AgentError::Failed(format!("wait failed: {e}")))?;
        match final_text {
            Some(t) if status.success() => Ok(t),
            _ => Err(AgentError::Failed(format!("worker produced no result (status {status})"))),
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(ANGLE_TIMEOUT_SECS), read)
        .await
        .map_err(|_| AgentError::Timeout)?
}
```

- [ ] **Step 3: Verify it compiles cleanly**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds, no errors, no warnings. (`ANGLE_TIMEOUT_SECS`, `plan_angles`, `stream_investigate`, `synthesize` are consumed in Task 7; if the linter flags them unused before then, that's expected — proceed to Task 7 in the same batch, or add `#[allow(dead_code)]` temporarily and remove it in Task 7.)

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — existing suite unaffected.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/agent.rs
git commit -m "feat(agent): async plan_angles + streaming stream_investigate + synthesize

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: `swarm.rs` — orchestration, events, registry, cancellation

**Files:**
- Create: `src-tauri/src/swarm.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod swarm;`)
- Test: none new (I/O orchestration — verified live). Deliverable = crate compiles clean.

**Interfaces:**
- Consumes: `agent::{plan_angles, stream_investigate, synthesize, PlannedAngle, Brief}`; `db::get_feed_item`.
- Produces: `pub struct SwarmRegistry` (`new`, `cancel`, internal `insert`); `pub fn run_swarm(app: AppHandle, db: Arc<Mutex<Connection>>, registry: &SwarmRegistry, item_id: String, angles: Vec<PlannedAngle>)`.

- [ ] **Step 1: Create `src-tauri/src/swarm.rs`**

```rust
use crate::agent::{self, Brief, PlannedAngle};
use crate::db;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter};

/// Tracks the currently-running dig-deeper orchestration per feed item, so a panel close
/// (or switching items) can abort it. Mirrors `Scheduler.handles`.
pub struct SwarmRegistry {
    handles: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl SwarmRegistry {
    pub fn new() -> Self {
        SwarmRegistry { handles: Mutex::new(HashMap::new()) }
    }

    fn insert(&self, item_id: String, handle: JoinHandle<()>) {
        // Replacing an existing run for the same item aborts the old one first.
        if let Some(old) = self.handles.lock().unwrap().insert(item_id, handle) {
            old.abort();
        }
    }

    /// Abort + forget a running swarm. Aborting the orchestration task unwinds its worker
    /// child-tasks, each dropping its `swarm_sem` permit AND its `kill_on_drop(true)` `Child`
    /// (SIGKILL to the OS `claude` process) — no leaked permit, no orphan process.
    pub fn cancel(&self, item_id: &str) {
        if let Some(handle) = self.handles.lock().unwrap().remove(item_id) {
            handle.abort();
        }
    }
}

impl Default for SwarmRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---- event payloads (camelCase, mirroring the tick events) ----

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmProgress {
    item_id: String,
    angle_id: String,
    line: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmAngleDone {
    item_id: String,
    angle_id: String,
    output: Option<String>,
    error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmBriefReady {
    item_id: String,
    brief: Brief,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SwarmFailed {
    item_id: String,
    error: String,
}

/// Start (or restart) the swarm for `item_id` with the confirmed `angles`. Spawns one
/// orchestration task and registers it for cancellation. The task: loads the item context,
/// fans out one streaming worker per angle (all start at once — SWARM_PERMITS == MAX_ANGLES),
/// forwards progress, joins, then synthesizes and emits the brief. All angles failing → `swarm-failed`.
pub fn run_swarm(
    app: AppHandle,
    db: Arc<Mutex<Connection>>,
    registry: &SwarmRegistry,
    item_id: String,
    angles: Vec<PlannedAngle>,
) {
    // Capture the registry key before `item_id` is moved into the task.
    let registry_key = item_id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        // Load the story context (lock, read, drop — never held across an await).
        let ctx = {
            let conn = match db.lock() {
                Ok(c) => c,
                Err(_) => {
                    let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "db unavailable".into() });
                    return;
                }
            };
            db::get_feed_item(&conn, &item_id).ok().flatten()
        };
        let ctx = match ctx {
            Some(c) => Arc::new(c),
            None => {
                let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "feed item not found".into() });
                return;
            }
        };

        // Fan out: one worker task per angle, all concurrent.
        let mut workers: Vec<JoinHandle<(PlannedAngle, Option<String>)>> = Vec::new();
        for angle in angles {
            let app = app.clone();
            let ctx = Arc::clone(&ctx);
            let item_id = item_id.clone();
            workers.push(tauri::async_runtime::spawn(async move {
                let angle_id = angle.id.clone();
                let progress_app = app.clone();
                let progress_item = item_id.clone();
                let progress_angle = angle_id.clone();
                let result = agent::stream_investigate(&ctx, &angle, move |line| {
                    let _ = progress_app.emit(
                        "swarm-progress",
                        SwarmProgress {
                            item_id: progress_item.clone(),
                            angle_id: progress_angle.clone(),
                            line,
                        },
                    );
                })
                .await;
                match &result {
                    Ok(output) => {
                        let _ = app.emit("swarm-angle-done", SwarmAngleDone {
                            item_id: item_id.clone(),
                            angle_id,
                            output: Some(output.clone()),
                            error: None,
                        });
                    }
                    Err(e) => {
                        let _ = app.emit("swarm-angle-done", SwarmAngleDone {
                            item_id: item_id.clone(),
                            angle_id,
                            output: None,
                            error: Some(e.message()),
                        });
                    }
                }
                (angle, result.ok())
            }));
        }

        // Join all workers (they run concurrently; this just gathers results).
        let mut results: Vec<(PlannedAngle, Option<String>)> = Vec::new();
        for w in workers {
            if let Ok(pair) = w.await {
                results.push(pair);
            }
        }

        // Degraded-vs-failed: if every angle failed, don't synthesize from nothing.
        if results.iter().all(|(_, out)| out.is_none()) {
            let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "all research angles failed".into() });
            return;
        }

        match agent::synthesize(&ctx, &results).await {
            Ok(brief) => {
                let _ = app.emit("swarm-brief-ready", SwarmBriefReady { item_id, brief });
            }
            Err(e) => {
                let _ = app.emit("swarm-failed", SwarmFailed { item_id, error: e.message() });
            }
        }
    });

    registry.insert(registry_key, handle);
}
```

- [ ] **Step 2: Declare the module**

In `src-tauri/src/lib.rs`, add `mod swarm;` to the module list at the top (after `mod scheduler;`):

```rust
mod scheduler;
mod swarm;
mod tick;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds, no errors, no warnings. (`run_swarm`/`SwarmRegistry` are consumed in Task 8; if flagged unused, proceed to Task 8 in the same batch.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/swarm.rs src-tauri/src/lib.rs
git commit -m "feat(swarm): orchestration + SwarmRegistry + streaming events + cancellation

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: `commands.rs` + `lib.rs` — three Tauri commands wired to the swarm

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `swarm` to `AppState` + three commands)
- Modify: `src-tauri/src/lib.rs` (register the commands)
- Test: none new. Deliverable = `cargo build` clean, full suite green.

**Interfaces:**
- Consumes: `agent::{plan_angles, PlannedAngle, MIN_ANGLES, MAX_ANGLES}`; `swarm::{SwarmRegistry, run_swarm}`; `db::get_feed_item`.
- Produces: commands `start_dig_deeper`, `confirm_dig_deeper`, `cancel_dig_deeper`.

- [ ] **Step 1: Add `SwarmRegistry` to `AppState`**

In `src-tauri/src/commands.rs`, extend the imports and struct. Change the top `use` for swarm:

```rust
use crate::swarm::{self, SwarmRegistry};
```

Add the field to `AppState` (after `claude_health`):

```rust
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub scheduler: Scheduler,
    pub claude_health: Arc<Mutex<ClaudeHealth>>,
    pub swarm: SwarmRegistry,
}
```

In `init_state`, construct it (near `let scheduler = Scheduler::new();`):

```rust
    let swarm = SwarmRegistry::new();
```

And add `swarm` to the returned `AppState` (the final expression of `init_state`):

```rust
    AppState { db, scheduler, claude_health, swarm }
```

- [ ] **Step 2: Add the three commands**

In `src-tauri/src/commands.rs`, add (near the other `#[tauri::command]` fns):

```rust
/// Start dig-deeper on a feed item: load its context and run the planner, returning the
/// proposed angles. Nothing runs yet — the frontend edits the list and calls `confirm_dig_deeper`.
/// Stateless across the two calls (the proposal lives in the frontend, not the backend).
#[tauri::command]
pub async fn start_dig_deeper(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Vec<agent::PlannedAngle>, String> {
    // Load context (lock, read, drop) before the await — never hold the guard across it.
    let ctx = {
        let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
        db::get_feed_item(&conn, &item_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "feed item not found".to_string())?
    };
    Ok(agent::plan_angles(&ctx).await)
}

/// Confirm the (edited) angle list and fire the swarm. Re-clamps to MIN..=MAX server-side.
#[tauri::command]
pub fn confirm_dig_deeper(
    app: AppHandle,
    state: State<'_, AppState>,
    item_id: String,
    angles: Vec<agent::PlannedAngle>,
) -> Result<(), String> {
    let mut angles = angles;
    angles.truncate(agent::MAX_ANGLES);
    if angles.len() < agent::MIN_ANGLES {
        return Err(format!("need at least {} angles", agent::MIN_ANGLES));
    }
    swarm::run_swarm(app, Arc::clone(&state.db), &state.swarm, item_id, angles);
    Ok(())
}

/// Cancel a running swarm (panel closed / switched items). Idempotent.
#[tauri::command]
pub fn cancel_dig_deeper(state: State<'_, AppState>, item_id: String) -> Result<(), String> {
    state.swarm.cancel(&item_id);
    Ok(())
}
```

- [ ] **Step 3: Register the commands**

In `src-tauri/src/lib.rs`, add the three to `tauri::generate_handler!` (after `commands::recheck_claude,`):

```rust
            commands::recheck_claude,
            commands::start_dig_deeper,
            commands::confirm_dig_deeper,
            commands::cancel_dig_deeper,
```

- [ ] **Step 4: Verify build + full suite**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds, no errors, no warnings (all Task 6/7 items now consumed).

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS — the full suite (37 prior + the new pure tests) green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): start/confirm/cancel dig-deeper commands + AppState.swarm

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Frontend — `types.ts` + `api.ts`

**Files:**
- Modify: `src/types.ts`
- Modify: `src/api.ts`
- Test: `npm run build` (tsc typecheck).

**Interfaces:**
- Produces: `PlannedAngle` type; `AngleStatus` gains `"error"`; `SwarmAngle.error?`; `startDigDeeper`/`confirmDigDeeper`/`cancelDigDeeper`; `onSwarmProgress`/`onSwarmAngleDone`/`onSwarmBriefReady`/`onSwarmFailed`.

- [ ] **Step 1: Extend `types.ts`**

In `src/types.ts`, change `AngleStatus` and `SwarmAngle`, and add `PlannedAngle`:

```ts
export type AngleStatus = "queued" | "running" | "done" | "error";

export interface SwarmAngle {
  id: string;
  icon: string;
  label: string;
  status: AngleStatus;
  lines: string[]; // streamed progress lines from the agent
  error?: string; // failure reason when status === "error"
}

export interface PlannedAngle {
  id: string;
  icon: string;
  label: string;
  focus: string;
}
```

- [ ] **Step 2: Add command wrappers + event listeners to `api.ts`**

In `src/api.ts`, add the import for `PlannedAngle` and `BriefSection`:

```ts
import type { Monitor, FeedItem, ClaudeHealth, PlannedAngle, BriefSection } from "./types";
```

Append:

```ts
// --- Dig-deeper research swarm ---

// Run the planner for a feed item; returns the proposed (editable) angles.
export const startDigDeeper = (itemId: string) =>
  invoke<PlannedAngle[]>("start_dig_deeper", { itemId });

// Confirm the edited angle list and start the swarm.
export const confirmDigDeeper = (itemId: string, angles: PlannedAngle[]) =>
  invoke<void>("confirm_dig_deeper", { itemId, angles });

// Cancel a running swarm (panel closed / item switched).
export const cancelDigDeeper = (itemId: string) =>
  invoke<void>("cancel_dig_deeper", { itemId });

export interface SwarmProgress { itemId: string; angleId: string; line: string }
export interface SwarmAngleDone {
  itemId: string;
  angleId: string;
  output: string | null;
  error: string | null;
}
export interface SwarmBriefReady {
  itemId: string;
  brief: { summary: string; sections: BriefSection[] };
}
export interface SwarmFailed { itemId: string; error: string }

export const onSwarmProgress = (cb: (p: SwarmProgress) => void) =>
  listen<SwarmProgress>("swarm-progress", (e) => cb(e.payload));
export const onSwarmAngleDone = (cb: (p: SwarmAngleDone) => void) =>
  listen<SwarmAngleDone>("swarm-angle-done", (e) => cb(e.payload));
export const onSwarmBriefReady = (cb: (p: SwarmBriefReady) => void) =>
  listen<SwarmBriefReady>("swarm-brief-ready", (e) => cb(e.payload));
export const onSwarmFailed = (cb: (p: SwarmFailed) => void) =>
  listen<SwarmFailed>("swarm-failed", (e) => cb(e.payload));
```

- [ ] **Step 3: Typecheck**

Run: `npm run build`
Expected: tsc + vite build clean. (`BriefSection` is already exported from `types.ts`; `PlannedAngle` now is too.)

- [ ] **Step 4: Commit**

```bash
git add src/types.ts src/api.ts
git commit -m "feat(ui): swarm types + dig-deeper command/event bindings

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: Frontend — live `DigDeeperPanel` (plan → confirm → run → brief) + `App.tsx` wiring

**Files:**
- Rewrite: `src/components/DigDeeperPanel.tsx`
- Modify: `src/App.tsx` (drop `BRIEF_F1` mock; `key`-remount the panel per item)
- Test: `npm run build`, then live (Task 11).

**Interfaces:**
- Consumes: Task 9 api. `DigDeeperPanel` new props: `{ item: FeedItem; onClose: () => void }` (no more `brief`).

- [ ] **Step 1: Rewrite `DigDeeperPanel.tsx`**

Replace the entire file with:

```tsx
import { useEffect, useRef, useState } from "react";
import type { AngleStatus, FeedItem, PlannedAngle, SwarmAngle } from "../types";
import {
  startDigDeeper,
  confirmDigDeeper,
  cancelDigDeeper,
  onSwarmProgress,
  onSwarmAngleDone,
  onSwarmBriefReady,
  onSwarmFailed,
} from "../api";

const STATUS_STYLE: Record<AngleStatus, { chip: string; label: string }> = {
  queued: { chip: "bg-paper text-faint", label: "queued" },
  running: { chip: "bg-hn-soft text-rust", label: "running" },
  done: { chip: "bg-[#eaf3ea] text-ok", label: "done" },
  error: { chip: "bg-hn-soft text-rust", label: "failed" },
};

type Brief = { summary: string; sections: { heading: string; body: string }[] };
type Phase = "planning" | "confirm" | "running";

function AngleLane({ angle }: { angle: SwarmAngle }) {
  const s = STATUS_STYLE[angle.status];
  return (
    <div className="rounded-lg border border-line bg-card p-3">
      <div className="flex items-center gap-2">
        <span className="text-[14px]">{angle.icon}</span>
        <span className="text-[13px] font-semibold text-ink">{angle.label}</span>
        <span
          className={`ml-auto flex items-center gap-1 rounded-full px-2 py-0.5 font-mono text-[10px] ${s.chip}`}
        >
          {angle.status === "running" && (
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-rust" />
          )}
          {s.label}
        </span>
      </div>
      {angle.lines.length > 0 && (
        <div className="mt-2 space-y-1 border-l-2 border-line pl-3">
          {angle.lines.map((line, i) => (
            <p key={i} className="font-mono text-[11px] leading-snug text-soft">
              {line}
            </p>
          ))}
        </div>
      )}
      {angle.error && (
        <p className="mt-2 text-[11px] text-rust">{angle.error}</p>
      )}
    </div>
  );
}

export function DigDeeperPanel({ item, onClose }: { item: FeedItem; onClose: () => void }) {
  const [phase, setPhase] = useState<Phase>("planning");
  const [planned, setPlanned] = useState<PlannedAngle[]>([]);
  const [angles, setAngles] = useState<SwarmAngle[]>([]);
  const [brief, setBrief] = useState<Brief | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const started = useRef(false);

  // Mount: run the planner, subscribe to swarm events. Unmount: cancel + unlisten.
  // The panel is keyed by item id in App, so each item gets a fresh mount.
  useEffect(() => {
    let alive = true;
    startDigDeeper(item.id)
      .then((a) => {
        if (!alive) return;
        setPlanned(a);
        setPhase("confirm");
      })
      .catch((e) => alive && setFailed(String(e)));

    const subs = [
      onSwarmProgress((p) => {
        if (p.itemId !== item.id) return;
        setAngles((prev) =>
          prev.map((a) => (a.id === p.angleId ? { ...a, lines: [...a.lines, p.line] } : a)),
        );
      }),
      onSwarmAngleDone((p) => {
        if (p.itemId !== item.id) return;
        setAngles((prev) =>
          prev.map((a) =>
            a.id === p.angleId
              ? {
                  ...a,
                  status: p.error ? ("error" as const) : ("done" as const),
                  error: p.error ?? undefined,
                }
              : a,
          ),
        );
      }),
      onSwarmBriefReady((p) => p.itemId === item.id && setBrief(p.brief)),
      onSwarmFailed((p) => p.itemId === item.id && setFailed(p.error)),
    ];

    return () => {
      alive = false;
      cancelDigDeeper(item.id);
      subs.forEach((s) => s.then((un) => un()));
    };
  }, [item.id]);

  const removeAngle = (id: string) =>
    setPlanned((p) => (p.length > 2 ? p.filter((a) => a.id !== id) : p));

  const addAngle = () => {
    const focus = draft.trim();
    if (!focus || planned.length >= 5) return;
    const label = focus.length > 22 ? `${focus.slice(0, 22)}…` : focus;
    setPlanned((p) => [
      ...p,
      { id: `u-${Date.now()}`, icon: "🧭", label, focus },
    ]);
    setDraft("");
  };

  const start = () => {
    // All confirmed angles start at once (SWARM_PERMITS == MAX_ANGLES), so mark them running.
    setAngles(
      planned.map((a) => ({
        id: a.id,
        icon: a.icon,
        label: a.label,
        status: "running" as const,
        lines: [],
      })),
    );
    setPhase("running");
    started.current = true;
    confirmDigDeeper(item.id, planned).catch((e) => setFailed(String(e)));
  };

  const doneCount = angles.filter((a) => a.status === "done" || a.status === "error").length;

  return (
    <div className="fixed inset-0 z-40 flex justify-end">
      <div className="absolute inset-0 bg-black/20" onClick={onClose} aria-hidden />
      <div className="relative flex h-full w-[440px] flex-col border-l border-line bg-paper shadow-2xl">
        <header className="flex items-start gap-3 border-b border-line px-5 py-4">
          <div className="min-w-0">
            <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-rust">
              Research swarm
            </div>
            <h2 className="mt-1 line-clamp-2 text-[14px] font-bold leading-snug">{item.title}</h2>
          </div>
          <button
            onClick={onClose}
            className="ml-auto shrink-0 rounded-md px-2 py-1 text-[16px] text-faint hover:bg-card"
            aria-label="Close"
          >
            ✕
          </button>
        </header>

        <div className="flex-1 overflow-y-auto px-5 py-4">
          {failed ? (
            <div className="mt-16 text-center text-[13px] text-rust">{failed}</div>
          ) : phase === "planning" ? (
            <div className="mt-16 text-center text-[13px] text-faint">Planning angles…</div>
          ) : phase === "confirm" ? (
            <>
              <p className="mb-2 text-[12px] text-soft">
                Proposed angles — remove any, or add your own (2–5). Type a word or a full sentence
                for a specific focus.
              </p>
              <div className="flex flex-wrap gap-2">
                {planned.map((a) => (
                  <span
                    key={a.id}
                    className="flex items-center gap-1 rounded-full bg-hn-soft px-2.5 py-1 text-[12px] text-rust"
                    title={a.focus}
                  >
                    {a.icon} {a.label}
                    <button
                      onClick={() => removeAngle(a.id)}
                      disabled={planned.length <= 2}
                      className="ml-1 text-faint hover:text-rust disabled:opacity-30"
                      aria-label={`Remove ${a.label}`}
                    >
                      ✕
                    </button>
                  </span>
                ))}
              </div>
              <div className="mt-3 flex gap-2">
                <input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addAngle()}
                  placeholder="Add an angle…"
                  disabled={planned.length >= 5}
                  className="flex-1 rounded-md border border-line bg-card px-3 py-1.5 text-[12.5px] text-ink placeholder:text-faint focus:border-hn-border focus:outline-none disabled:opacity-40"
                />
                <button
                  onClick={addAngle}
                  disabled={!draft.trim() || planned.length >= 5}
                  className="rounded-md border border-line px-3 py-1.5 text-[12.5px] text-soft hover:bg-card disabled:opacity-40"
                >
                  Add
                </button>
              </div>
              <button
                onClick={start}
                className="mt-4 w-full rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white hover:opacity-90"
              >
                Start research ({planned.length} {planned.length === 1 ? "agent" : "agents"})
              </button>
            </>
          ) : (
            <>
              <div className="mb-2 flex items-center justify-between">
                <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                  Agents
                </span>
                <span className="font-mono text-[11px] text-faint">
                  {doneCount}/{angles.length} done
                </span>
              </div>
              <div className="space-y-2">
                {angles.map((a) => (
                  <AngleLane key={a.id} angle={a} />
                ))}
              </div>

              {brief && (
                <>
                  <div className="mt-6 mb-2 flex items-center gap-2">
                    <span className="text-[14px]">🧩</span>
                    <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                      Combined brief
                    </span>
                  </div>
                  <div className="rounded-xl border border-line bg-card p-4">
                    <p className="text-[13px] leading-relaxed text-soft">{brief.summary}</p>
                    <div className="mt-4 space-y-3">
                      {brief.sections.map((sec) => (
                        <div key={sec.heading}>
                          <h3 className="text-[12.5px] font-bold text-ink">{sec.heading}</h3>
                          <p className="mt-0.5 text-[12.5px] leading-relaxed text-soft">
                            {sec.body}
                          </p>
                        </div>
                      ))}
                    </div>
                  </div>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Update `App.tsx`**

In `src/App.tsx`, remove the mock import (delete line `import { BRIEF_F1 } from "./mock/data";`). Replace the panel render block (the `{digItem && ( … )}`) with:

```tsx
        {digItem && (
          <DigDeeperPanel key={digItem.id} item={digItem} onClose={() => setDigItem(null)} />
        )}
```

- [ ] **Step 3: Typecheck / build**

Run: `npm run build`
Expected: clean. No remaining references to `BRIEF_F1` (grep to confirm: `grep -rn BRIEF_F1 src` returns only `src/mock/data.ts`, which is now unused by the app but may stay for reference).

- [ ] **Step 4: Commit**

```bash
git add src/components/DigDeeperPanel.tsx src/App.tsx
git commit -m "feat(ui): live dig-deeper panel — plan/confirm/run/brief, replaces mock

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: Live verification in the native window + push

**Files:** none (verification only — real native window per `docs/TESTING.md`, never localhost).

- [ ] **Step 1: Build + launch**

Run: `npm run tauri build`, then launch the bundled app (or `npm run tauri dev`) and drive it with computer-use per `docs/TESTING.md`. Ensure at least one monitor has feed items (create one if needed and let a tick land).

- [ ] **Step 2: Plan + confirm flow**

Click **Dig deeper** on a feed item. Confirm: panel shows "Planning angles…", then a set of story-appropriate angle pills. Remove one (confirm the ✕ disables at 2 remaining); add a custom sentence angle (confirm it disables at 5). Click **Start research**.

- [ ] **Step 3: Live streaming + degraded brief**

Confirm each lane flips `running`, streams progress lines (tool-use / text), and settles to `done`. Then compiles a **combined brief** (summary + sections). Force a partial failure — either add a nonsense angle likely to error, or set `HN_WATCH_CLAUDE_BIN` to a fake that fails, on one run — and confirm the failing lane shows `failed` with a reason while the others still produce a brief that notes the gap. Confirm the all-failed case shows the panel error state.

- [ ] **Step 4: Two-pools + cancellation**

While a swarm is running, confirm a monitor tick still fires promptly (watch its sidebar `Checking…` chip appear without waiting for the swarm) — evidence the reserved pools hold. Close the panel mid-run; in a terminal confirm the child processes are gone: `ps aux | grep -c "[c]laude -p --output-format stream-json"` drops to 0 shortly after — evidence `abort()` + `kill_on_drop` killed them.

- [ ] **Step 5: Update `STATUS.md`**

Add a Session entry summarizing what shipped (swarm topology, two reserved pools, planner + human-in-the-loop confirm, streaming, degraded brief), the empirical Task-1 finding (tool flag + whether web tools work in `-p`), and the live-verification results. Move the "Next — Dig-deeper research swarm" section to done.

- [ ] **Step 6: Commit + push (keep the branch on origin per CLAUDE.md)**

```bash
git add STATUS.md
git commit -m "docs: STATUS — dig-deeper research swarm shipped

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push -u origin feat/dig-deeper-swarm
```

- [ ] **Step 7: Whole-branch review**

Before merging, run a whole-branch code review (per the project's established workflow — `superpowers:requesting-code-review` or `/code-review`). Address findings, then merge `feat/dig-deeper-swarm` → `main` (`--no-ff`) and keep the branch on origin.

---

## Notes for the implementer

- **Task 1 gates the flag literals.** If web tools don't work in headless `-p`, drop the two `--allowedTools` args in `stream_investigate` (Task 6, Step 2) — everything else is unchanged — and record the closed-book limitation in `STATUS.md`.
- **Never hold a `MutexGuard` across `.await`.** `start_dig_deeper` and `run_swarm` both load context by locking, reading into an owned `FeedItemContext`, and dropping the guard *before* awaiting — mirror `recheck_claude`'s existing pattern.
- **`SWARM_PERMITS == MAX_ANGLES == 5`** is deliberate: every confirmed angle acquires a permit immediately, so all lanes truly run in parallel. The frontend marks them all `running` on Start for the same reason.
- **Cancellation correctness rests on ownership:** each worker task owns its `OwnedSemaphorePermit` and its `kill_on_drop(true)` `Child`; `abort()` drops both. Do not restructure workers to share a `Child` or move the permit out of the task.
- **Don't touch the tick path** beyond the `agent_sem` → `tick_sem` rename (Task 2). `judge()`'s buffered `--safe-mode` behavior is unchanged; only its pool changed.
- **Batch Tasks 6–8** if a linter rejects the interim "unused" warnings between them — they form one compile-clean unit (async helpers → orchestration → commands).
```
