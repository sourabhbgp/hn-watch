# Persist Dig-Deeper Research Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist a completed dig-deeper run per story so reopening a researched feed item shows the saved brief + angles instantly (zero `claude` processes), with an explicit "Dig deeper again" to re-run.

**Architecture:** A new single `research` table (JSON columns, latest-wins, keyed by `feed_item_id` with `ON DELETE CASCADE`). `run_swarm` upserts the brief + per-angle results on successful synthesis (never on start/cancel/total-failure). `DigDeeperPanel`'s mount effect becomes saved-first: `getResearch` runs before any planner, and the planner is reached only when nothing is saved.

**Tech Stack:** Rust (rusqlite, serde_json, tauri commands, tokio JoinSet), React 19 + TypeScript, existing Tailwind design tokens.

**Spec:** `docs/superpowers/specs/2026-07-11-persist-dig-deeper-research-design.md`

## Global Constraints

- **Branch:** all work on `feat/persist-dig-deeper-research` (already created + spec committed); push to origin and keep it; merge to `main` with `--no-ff` at the end.
- **Design tokens only** — no hardcoded colors/fonts/spacing; reuse tokens in `src/index.css` / `docs/design.md`.
- **DRY / YAGNI / TDD** — latest-wins only (no history); do **not** retrofit per-angle findings into the *live* view; leave dedup/cancellation/degraded/delivery paths untouched.
- **Save on completion only** — never delete/overwrite saved research on run *start*; a cancelled or fully-failed run must leave prior saved research intact.
- **Verify in the real native window** (`docs/TESTING.md`), not a browser; the acceptance proof is **no `claude` process spawns on reopen**, not "the brief rendered."
- Rust: `cargo test` green + `cargo build` zero warnings. Frontend: `tsc` + `vite build` clean.

---

### Task 1: `research` table + save/load in `db.rs`

The whole persistence layer: schema migration, the `SavedResearch`/`SavedAngle` types, `save_research` (upsert), and `get_research`. Pure SQLite + serde_json, unit-testable with an in-memory DB — no Tauri, no `claude`.

**Files:**
- Modify: `src-tauri/src/db.rs` (add table to `migrate`, add types + two functions + tests)

**Interfaces:**
- Consumes: existing `db::migrate`, `insert_monitor`, `insert_feed_item`, `FeedRow`, `Monitor`, `delete_monitor` (for tests); `agent::Brief` and `agent::BriefSection` (agent.rs:554/561) for the save signature.
- Produces:
  - `pub struct SavedAngle { pub id: String, pub icon: String, pub label: String, pub focus: String, pub status: String, pub findings: Option<String>, pub error: Option<String> }` — derives `Debug, Clone, serde::Serialize, serde::Deserialize`, `#[serde(rename_all = "camelCase")]`.
  - `pub struct SavedResearch { pub summary: String, pub sections: Vec<crate::agent::BriefSection>, pub angles: Vec<SavedAngle>, pub created_at: i64 }` — derives `Debug, Clone`.
  - `pub fn save_research(conn: &Connection, feed_item_id: &str, brief: &crate::agent::Brief, angles: &[SavedAngle], now: i64) -> rusqlite::Result<()>`
  - `pub fn get_research(conn: &Connection, feed_item_id: &str) -> rusqlite::Result<Option<SavedResearch>>`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module at the bottom of `src-tauri/src/db.rs` (reuse the existing `mem()`, `sample_monitor()` helpers). Add a small local helper to insert a feed item and needed imports (`use crate::agent::{Brief, BriefSection};` at the top of the test fn or module).

```rust
    fn seed_item(c: &Connection) {
        // a monitor + one feed item to hang research off of
        insert_monitor(c, &sample_monitor("m1")).unwrap();
        insert_feed_item(c, &FeedRow {
            id: "f1".into(), monitor_id: "m1".into(), hn_id: "hn1".into(),
            title: "t".into(), url: "u".into(), domain: "d".into(),
            summary: "s".into(), reason: "r".into(), hn_score: 1, hn_comments: 1, created_at: 1,
        }).unwrap();
    }

    fn sample_brief() -> crate::agent::Brief {
        crate::agent::Brief {
            summary: "overview".into(),
            sections: vec![crate::agent::BriefSection { heading: "H1".into(), body: "b1".into() }],
        }
    }

    fn sample_angles() -> Vec<SavedAngle> {
        vec![
            SavedAngle { id: "a1".into(), icon: "🔎".into(), label: "Origin".into(),
                focus: "where it came from".into(), status: "done".into(),
                findings: Some("found stuff".into()), error: None },
            SavedAngle { id: "a2".into(), icon: "⚠️".into(), label: "Risks".into(),
                focus: "risks".into(), status: "failed".into(),
                findings: None, error: Some("claude timed out".into()) },
        ]
    }

    #[test]
    fn save_and_get_research_round_trips() {
        let c = mem();
        seed_item(&c);
        save_research(&c, "f1", &sample_brief(), &sample_angles(), 777).unwrap();

        let got = get_research(&c, "f1").unwrap().expect("saved");
        assert_eq!(got.summary, "overview");
        assert_eq!(got.created_at, 777);
        assert_eq!(got.sections.len(), 1);
        assert_eq!(got.sections[0].heading, "H1");
        assert_eq!(got.angles.len(), 2);
        assert_eq!(got.angles[0].status, "done");
        assert_eq!(got.angles[0].findings.as_deref(), Some("found stuff"));
        assert_eq!(got.angles[1].status, "failed");
        assert_eq!(got.angles[1].error.as_deref(), Some("claude timed out"));
    }

    #[test]
    fn get_research_none_for_unknown_id() {
        let c = mem();
        seed_item(&c);
        assert!(get_research(&c, "nope").unwrap().is_none());
    }

    #[test]
    fn save_research_is_latest_wins_upsert() {
        let c = mem();
        seed_item(&c);
        save_research(&c, "f1", &sample_brief(), &sample_angles(), 1).unwrap();
        let mut b2 = sample_brief();
        b2.summary = "newer".into();
        save_research(&c, "f1", &b2, &[], 2).unwrap();

        let got = get_research(&c, "f1").unwrap().expect("saved");
        assert_eq!(got.summary, "newer");
        assert_eq!(got.created_at, 2);
        assert_eq!(got.angles.len(), 0);
        // exactly one row for f1
        let n: i64 = c.query_row("SELECT COUNT(*) FROM research WHERE feed_item_id='f1'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn deleting_monitor_cascades_research() {
        let c = mem();
        c.execute("PRAGMA foreign_keys = ON", []).unwrap();
        seed_item(&c);
        save_research(&c, "f1", &sample_brief(), &sample_angles(), 1).unwrap();
        delete_monitor(&c, "m1").unwrap(); // cascades feed_items -> research
        assert!(get_research(&c, "f1").unwrap().is_none());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib db::tests::save_and_get_research_round_trips db::tests::get_research_none_for_unknown_id db::tests::save_research_is_latest_wins_upsert db::tests::deleting_monitor_cascades_research`
Expected: FAIL to compile — `save_research`, `get_research`, `SavedAngle`, `SavedResearch` not found.

- [ ] **Step 3: Add the `research` table to `migrate`**

In `src-tauri/src/db.rs`, inside `migrate`, add the table to the `execute_batch` string (append after the `seen` table block, before the closing `"`):

```rust
         CREATE TABLE IF NOT EXISTS research (
             feed_item_id TEXT PRIMARY KEY REFERENCES feed_items(id) ON DELETE CASCADE,
             summary TEXT NOT NULL,
             sections TEXT NOT NULL,
             angles TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
```

- [ ] **Step 4: Add the types + `save_research` + `get_research`**

Add near the other structs (e.g. after `FeedItemContext`) in `src-tauri/src/db.rs`:

```rust
/// One angle as persisted in a saved research run. `findings` set when `status == "done"`,
/// `error` set (with the reason) when `status == "failed"`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAngle {
    pub id: String,
    pub icon: String,
    pub label: String,
    pub focus: String,
    pub status: String,
    pub findings: Option<String>,
    pub error: Option<String>,
}

/// A completed dig-deeper run reloaded from disk (latest-wins, one per feed item).
#[derive(Debug, Clone)]
pub struct SavedResearch {
    pub summary: String,
    pub sections: Vec<crate::agent::BriefSection>,
    pub angles: Vec<SavedAngle>,
    pub created_at: i64,
}
```

Add the two functions (near the other `feed_items` helpers):

```rust
/// Upsert the completed research for a feed item (latest-wins). `sections`/`angles`
/// are stored as JSON. Called only after a successful synthesis — never on run start.
pub fn save_research(
    conn: &Connection,
    feed_item_id: &str,
    brief: &crate::agent::Brief,
    angles: &[SavedAngle],
    now: i64,
) -> rusqlite::Result<()> {
    let sections = serde_json::to_string(&brief.sections).unwrap_or_else(|_| "[]".into());
    let angles = serde_json::to_string(angles).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO research (feed_item_id, summary, sections, angles, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(feed_item_id) DO UPDATE SET
             summary = excluded.summary,
             sections = excluded.sections,
             angles = excluded.angles,
             created_at = excluded.created_at",
        rusqlite::params![feed_item_id, brief.summary, sections, angles, now],
    )?;
    Ok(())
}

/// Load the saved research for a feed item, or `None` if it has never been dug into.
pub fn get_research(conn: &Connection, feed_item_id: &str) -> rusqlite::Result<Option<SavedResearch>> {
    let mut stmt = conn.prepare(
        "SELECT summary, sections, angles, created_at FROM research WHERE feed_item_id = ?1",
    )?;
    let mut rows = stmt.query_map([feed_item_id], |r| {
        let summary: String = r.get(0)?;
        let sections_json: String = r.get(1)?;
        let angles_json: String = r.get(2)?;
        let created_at: i64 = r.get(3)?;
        Ok((summary, sections_json, angles_json, created_at))
    })?;
    match rows.next() {
        Some(row) => {
            let (summary, sections_json, angles_json, created_at) = row?;
            let sections = serde_json::from_str(&sections_json).unwrap_or_default();
            let angles = serde_json::from_str(&angles_json).unwrap_or_default();
            Ok(Some(SavedResearch { summary, sections, angles, created_at }))
        }
        None => Ok(None),
    }
}
```

Note: `BriefSection` (agent.rs:561) already derives `Serialize + Deserialize`; `serde_json::from_str::<Vec<_>>` with `unwrap_or_default()` yields `vec![]` on any malformed JSON (defensive, never panics).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib db::tests::save_and_get_research_round_trips db::tests::get_research_none_for_unknown_id db::tests::save_research_is_latest_wins_upsert db::tests::deleting_monitor_cascades_research`
Expected: PASS (4 tests). Also run `cargo test --lib` to confirm the migration change didn't break existing db tests, and `cargo build` for zero warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "feat(db): research table + save_research/get_research (latest-wins, cascade)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Retain per-angle errors + save on completion in `swarm.rs`

Widen the `JoinSet` result so the per-angle error text survives to the save site, then upsert the brief + angles right after a successful synthesis. Pure backend; verified by `cargo build` + the existing swarm tests staying green (the save site itself is proven live in Task 5).

**Files:**
- Modify: `src-tauri/src/swarm.rs` (`run_swarm` result type + save call)

**Interfaces:**
- Consumes: `db::save_research`, `db::SavedAngle` (Task 1); `agent::stream_investigate` (returns `Result<String, AgentError>` with `.message()`), `agent::synthesize`, `agent::PlannedAngle`, `crate::tick::now_secs`.
- Produces: no new public surface; `run_swarm` behavior now persists a completed run.

- [ ] **Step 1: Change the worker result type to carry the error**

In `src-tauri/src/swarm.rs` `run_swarm`, change the `JoinSet` type and the worker's return value so a failed angle keeps its message:

```rust
        // Result carries the error text (not just None) so a saved run can show why an angle failed.
        let mut set: JoinSet<(PlannedAngle, Result<String, String>)> = JoinSet::new();
```

In the spawned worker, keep the existing `swarm-angle-done` emit unchanged, but return the error text instead of dropping it. Replace the trailing `(angle, result.ok())` with:

```rust
                (angle, result.map_err(|e| e.message()))
```

(The `match &result { Ok(output) => … Err(e) => … }` emit block above it stays exactly as-is.)

- [ ] **Step 2: Update the join + degraded check + synthesize call**

The results vector type changes to `Vec<(PlannedAngle, Result<String, String>)>`. Update the "all failed" guard and the synthesize call (which still needs `Option<String>` findings):

```rust
        // Join all workers (they run concurrently; this just gathers results).
        let mut results: Vec<(PlannedAngle, Result<String, String>)> = Vec::new();
        while let Some(res) = set.join_next().await {
            if let Ok(pair) = res {
                results.push(pair);
            }
        }

        // Degraded-vs-failed: if every angle failed, don't synthesize from nothing.
        if results.iter().all(|(_, out)| out.is_err()) {
            let _ = app.emit("swarm-failed", SwarmFailed { item_id: item_id.clone(), error: "all research angles failed".into() });
            return;
        }

        // synthesize still consumes Option<String> findings.
        let synth_input: Vec<(PlannedAngle, Option<String>)> = results
            .iter()
            .map(|(a, r)| (a.clone(), r.clone().ok()))
            .collect();
        match agent::synthesize(&ctx, &synth_input).await {
```

- [ ] **Step 3: Save the completed run before emitting the brief**

Inside the `Ok(brief) =>` arm of the synthesize match, **before** the `swarm-brief-ready` emit, map results → `SavedAngle` and upsert. Replace the arm body:

```rust
            Ok(brief) => {
                // Persist the completed run (latest-wins) so a reopen shows it without re-running.
                let saved: Vec<db::SavedAngle> = results
                    .iter()
                    .map(|(a, r)| db::SavedAngle {
                        id: a.id.clone(),
                        icon: a.icon.clone(),
                        label: a.label.clone(),
                        focus: a.focus.clone(),
                        status: if r.is_ok() { "done".into() } else { "failed".into() },
                        findings: r.clone().ok(),
                        error: r.clone().err(),
                    })
                    .collect();
                if let Ok(conn) = db.lock() {
                    let _ = db::save_research(&conn, &item_id, &brief, &saved, crate::tick::now_secs());
                }
                let _ = app.emit("swarm-brief-ready", SwarmBriefReady { item_id, brief });
            }
```

Note: `db` (the `Arc<Mutex<Connection>>`) is already moved into this task's async block and is available here; the lock is taken briefly and dropped before the emit. A save failure is best-effort (`let _ =`) — it never blocks showing the brief.

- [ ] **Step 4: Verify it builds and existing tests pass**

Run: `cd src-tauri && cargo build 2>&1 | tail -5` — Expected: compiles, zero warnings.
Run: `cargo test --lib` — Expected: all existing tests still PASS (swarm cancel tests unaffected — the registry/cancel path is untouched).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/swarm.rs
git commit -m "feat(swarm): persist completed run on synthesis; retain per-angle error text

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `get_research` Tauri command + `api.ts` binding

Expose the load path to the frontend. Small wiring task; verified by `cargo build` + `tsc`.

**Files:**
- Modify: `src-tauri/src/commands.rs` (DTOs + command)
- Modify: `src-tauri/src/lib.rs` (register the command in the handler)
- Modify: `src/api.ts` (binding + types)

**Interfaces:**
- Consumes: `db::get_research`, `db::SavedResearch`, `db::SavedAngle` (Task 1); existing `AppState`.
- Produces:
  - Rust command `get_research(item_id: String) -> Result<Option<SavedResearchDto>, String>`.
  - TS `getResearch(itemId: string): Promise<SavedResearch | null>` and types `SavedResearch`, `SavedAngle`.

- [ ] **Step 1: Add the DTOs + command to `commands.rs`**

Add near the other DTOs in `src-tauri/src/commands.rs`:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAngleDto {
    pub id: String,
    pub icon: String,
    pub label: String,
    pub focus: String,
    pub status: String,
    pub findings: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedResearchDto {
    pub summary: String,
    pub sections: Vec<agent::BriefSection>, // BriefSection already serializes camelCase; reuse it
    pub angles: Vec<SavedAngleDto>,
    pub created_at: i64,
}
```

`agent::BriefSection` is reachable via the file's existing `use crate::agent::{self, ClaudeHealth};`.

Add the command (near `cancel_dig_deeper`):

```rust
/// Load the saved dig-deeper run for a feed item, or `null` if it was never dug into.
/// Reading spawns no `claude` — this is the reopen fast-path.
#[tauri::command]
pub fn get_research(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Option<SavedResearchDto>, String> {
    let conn = state.db.lock().map_err(|_| "db poisoned".to_string())?;
    let saved = db::get_research(&conn, &item_id).map_err(|e| e.to_string())?;
    Ok(saved.map(|s| SavedResearchDto {
        summary: s.summary,
        sections: s.sections,
        angles: s
            .angles
            .into_iter()
            .map(|a| SavedAngleDto {
                id: a.id, icon: a.icon, label: a.label, focus: a.focus,
                status: a.status, findings: a.findings, error: a.error,
            })
            .collect(),
        created_at: s.created_at,
    }))
}
```

Ensure `agent::BriefSection` is imported — the file already has `use crate::agent::{self, ClaudeHealth};`, so refer to it as `agent::BriefSection`.

- [ ] **Step 2: Register the command in `lib.rs`**

In `src-tauri/src/lib.rs`, add `commands::get_research` to the `tauri::generate_handler![...]` list (alongside `start_dig_deeper`, `confirm_dig_deeper`, `cancel_dig_deeper`).

- [ ] **Step 3: Verify Rust builds**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: compiles, zero warnings.

- [ ] **Step 4: Add the TS binding + types to `api.ts`**

In `src/api.ts`, add under the dig-deeper section:

```ts
export interface SavedAngle {
  id: string;
  icon: string;
  label: string;
  focus: string;
  status: "done" | "failed";
  findings: string | null;
  error: string | null;
}
export interface SavedResearch {
  summary: string;
  sections: BriefSection[];
  angles: SavedAngle[];
  createdAt: number; // epoch seconds
}

// Load saved research for a feed item (null if never dug into). Spawns no claude.
export const getResearch = (itemId: string) =>
  invoke<SavedResearch | null>("get_research", { itemId });
```

(`BriefSection` is already imported at the top of `api.ts`.)

- [ ] **Step 5: Verify the frontend typechecks**

Run: `npm run build 2>&1 | tail -8` (or `npx tsc --noEmit`)
Expected: no type errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts
git commit -m "feat(api): get_research command + TS binding

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Saved-first reopen flow + saved view in `DigDeeperPanel`

The user-visible payload: reopen shows saved research instantly (no planner), with findings + failed-angle reason, a "researched Xh ago" line, and "Dig deeper again". Verified live in Task 5.

**Files:**
- Modify: `src/components/DigDeeperPanel.tsx`
- Create: `src/lib/timeAgo.ts` (tiny shared relative-time formatter) + `src/lib/timeAgo.test.ts` if a test runner exists; otherwise inline-verify (see note).

**Interfaces:**
- Consumes: `getResearch`, `SavedResearch`, `SavedAngle` (Task 3); existing `startDigDeeper`/`confirmDigDeeper`/`cancelDigDeeper` + `onSwarm*` subscriptions.
- Produces: no new exported surface (panel-internal); `timeAgo(epochSecs, nowSecs)` helper.

- [ ] **Step 1: Add the `timeAgo` helper**

Create `src/lib/timeAgo.ts` (mirrors the backend `time_ago` buckets in `commands.rs:76` so copy reads consistently):

```ts
/** Compact relative time: "3m", "2h", "5d" for the gap between `then` and `now` (epoch secs). */
export function timeAgo(then: number, now: number): string {
  const d = Math.max(0, now - then);
  if (d < 3600) return `${Math.max(1, Math.floor(d / 60))}m`;
  if (d < 86_400) return `${Math.floor(d / 3600)}h`;
  return `${Math.floor(d / 86_400)}d`;
}
```

- [ ] **Step 2: Verify the helper (build check)**

Run: `npx tsc --noEmit`
Expected: no type errors. (No standalone JS test runner is configured in this project — the helper is exercised live in Task 5 via the "researched Xh ago" line.)

- [ ] **Step 3: Extend `AngleLane` to render findings**

In `src/components/DigDeeperPanel.tsx`, extend the `SwarmAngle` usage so a saved lane can show findings prose. Add an optional `findings?: string` to the lane's angle shape (the panel builds these objects itself). In `AngleLane`, after the `lines` block and before/around the `error` block, render findings when present:

```tsx
      {angle.findings && (
        <p className="mt-2 whitespace-pre-wrap text-[12px] leading-relaxed text-soft">
          {angle.findings}
        </p>
      )}
```

Update the local lane type. At the top of the file, define a lane view-model that extends `SwarmAngle` with the optional findings (do not touch the shared `src/types.ts` `SwarmAngle`):

```tsx
type LaneAngle = SwarmAngle & { findings?: string };
```

Change `AngleLane`'s prop and the `angles` state to `LaneAngle`:

```tsx
function AngleLane({ angle }: { angle: LaneAngle }) {
```
```tsx
  const [angles, setAngles] = useState<LaneAngle[]>([]);
```

- [ ] **Step 4: Add the `"saved"` phase + saved-first mount effect**

In `DigDeeperPanel.tsx`:

Add `"saved"` to the `Phase` type:
```tsx
type Phase = "planning" | "confirm" | "running" | "saved";
```

Add state for the saved run + its timestamp:
```tsx
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [now] = useState(() => Math.floor(Date.now() / 1000));
```

Import the new symbols at the top:
```tsx
import { getResearch, /* existing: */ startDigDeeper, confirmDigDeeper, cancelDigDeeper,
  onSwarmProgress, onSwarmAngleDone, onSwarmBriefReady, onSwarmFailed } from "../api";
import { timeAgo } from "../lib/timeAgo";
import type { SavedAngle } from "../api";
```

Restructure the mount effect so `getResearch` runs first; only fall through to the planner when nothing is saved. Replace the `startDigDeeper(item.id).then(...)` block with:

```tsx
    let alive = true;
    getResearch(item.id)
      .then((saved) => {
        if (!alive) return;
        if (saved) {
          // Reopen fast-path: render the saved run, spawn nothing.
          setBrief({ summary: saved.summary, sections: saved.sections });
          setSavedAt(saved.createdAt);
          setAngles(
            saved.angles.map((a: SavedAngle) => ({
              id: a.id, icon: a.icon, label: a.label,
              status: a.status === "failed" ? ("error" as const) : ("done" as const),
              lines: [],
              findings: a.findings ?? undefined,
              error: a.error ?? undefined,
            })),
          );
          setPhase("saved");
        } else {
          // No saved run → the normal planner flow.
          startDigDeeper(item.id)
            .then((a) => { if (alive) { setPlanned(a); setPhase("confirm"); } })
            .catch((e) => alive && setFailed(String(e)));
        }
      })
      .catch((e) => alive && setFailed(String(e)));
```

The `subs` array (the four `onSwarm*` listeners) and the cleanup (`alive = false; cancelDigDeeper(item.id); subs.forEach(...)`) stay exactly as they are — the listeners are harmless in the saved branch.

- [ ] **Step 5: Add "Dig deeper again" + render the saved view**

Add a handler that resets to a fresh planner run:

```tsx
  const digAgain = () => {
    setBrief(null);
    setSavedAt(null);
    setAngles([]);
    setFailed(null);
    setPhase("planning");
    startDigDeeper(item.id)
      .then((a) => { setPlanned(a); setPhase("confirm"); })
      .catch((e) => setFailed(String(e)));
  };
```

In the render, the existing `phase === "running"` branch already renders `angles` + `brief`. Make the saved view reuse that same markup by broadening the final branch condition. Change the running-branch guard so it also covers `"saved"`, and inside it, when `phase === "saved"`, show the "researched Xh ago" line + the "Dig deeper again" button. Concretely, in the block that renders the brief (the `{brief && ( … )}` section), add above the summary:

```tsx
                    {phase === "saved" && savedAt !== null && (
                      <div className="mb-3 flex items-center justify-between">
                        <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                          researched {timeAgo(savedAt, now)} ago
                        </span>
                        <button
                          onClick={digAgain}
                          className="rounded-md border border-line px-2.5 py-1 text-[11px] text-soft hover:bg-card"
                        >
                          Dig deeper again
                        </button>
                      </div>
                    )}
```

And change the branch condition rendering agents+brief from `) : (` (the running fallthrough) to explicitly include saved — i.e. the final `else` currently handles `"running"`; it now also handles `"saved"` since both render `angles`/`brief`. Ensure the "Agents" `doneCount`/header still reads fine for saved (all angles are done/error, so `doneCount === angles.length`). No change needed there — it will show e.g. `2/2 done`.

- [ ] **Step 6: Verify the frontend typechecks + builds**

Run: `npm run build 2>&1 | tail -8`
Expected: `tsc` + `vite build` succeed, no type errors.

- [ ] **Step 7: Commit**

```bash
git add src/components/DigDeeperPanel.tsx src/lib/timeAgo.ts
git commit -m "feat(ui): saved-first reopen — show saved brief+angles, dig-again, researched-ago

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Live verification in the native window

The acceptance proof. Static review + typecheck cannot show whether reopen re-runs the swarm — this is the Session 10 lesson ("rendered" ≠ "didn't re-run"). Prove **no `claude` spawns on reopen**, then the full UX.

**Files:** none (verification only — no code unless a defect is found).

- [ ] **Step 1: Build the release app**

Run: `npm run tauri build 2>&1 | tail -20`
Expected: builds the `.app` bundle (computer-use always launches the release bundle — a stale dev build would mask the branch; see Session 11 gotcha).

- [ ] **Step 2: Prove zero `claude` on reopen**

Launch the app (per `docs/TESTING.md` / the `run` skill). Pick (or create) a monitor with at least one feed item.
1. Click **Dig deeper** on an item → confirm angles → **Start research** → let it complete to a brief. Note the story.
2. In a terminal, start watching for planner/worker processes:
   `while true; do pgrep -fl "claude -p" | grep -c . ; sleep 1; done` (or `ps aux | grep 'claude -p'`).
3. Close the panel, then **reopen Dig deeper on the same story**.
Expected: the saved brief + angle lanes (with findings, and a failed angle's reason if any) appear **instantly**, and the `claude -p` count stays **0** across the reopen — no planner, no workers.
Record the observation (count before/after) in `STATUS.md`.

- [ ] **Step 3: Verify the saved view content + timestamp**

Expected: each angle lane shows its findings prose and `done`/`failed` chip; a failed angle shows its error reason; the brief shows the `researched Xh ago` line; the `Dig deeper again` button is present.

- [ ] **Step 4: Verify "Dig deeper again" overwrites, and cancel-safety**

1. Click **Dig deeper again** → confirm it enters the planner → confirm → let it complete → reopen → the new run is shown (overwritten).
2. Click **Dig deeper again**, then **close the panel mid-run** (cancel) → reopen → the **prior** saved run still loads (cancel did not wipe it). This is the "save on completion, never on start" guarantee.

- [ ] **Step 5: Verify persistence across restart**

Quit the app via the tray **Quit** (fully exits), relaunch, reopen the story → saved research still loads from SQLite.

- [ ] **Step 6: Record results in STATUS.md**

Add a Session entry summarizing: `cargo test` count, `cargo build`/`npm run build` clean, and the live observations (esp. the 0-`claude`-on-reopen proof). Commit:

```bash
git add STATUS.md
git commit -m "docs: STATUS — persist dig-deeper research, live-verified (0 claude on reopen)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Whole-branch review, TODO update, merge

**Files:**
- Modify: `docs/TODO.md` (mark #8 shipped), `STATUS.md` (final touches if the review changes anything).

- [ ] **Step 1: Whole-branch review**

Run the `superpowers:requesting-code-review` skill (or `/code-review high`) over the full branch diff vs `main`. Focus areas: the mount-effect saved-first boundary (no planner when saved), save-on-completion-only (cancel/total-failure save nothing), JSON round-trip robustness, DRY of the lane view-model. Fix any Important/Critical findings (each fix its own commit), then re-verify affected paths.

- [ ] **Step 2: Mark TODO #8 shipped**

In `docs/TODO.md`, add a `✅ SHIPPED (Session N)` note to the `## 8.` heading with a one-paragraph summary (mirroring how #1/#2/#3/#5 are annotated), and update the trailing "Order to tackle" / "Next up" footer.

```bash
git add docs/TODO.md STATUS.md
git commit -m "docs(todo): #8 persist dig-deeper research — shipped

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 3: Push the branch, then merge to main**

```bash
git push -u origin feat/persist-dig-deeper-research
git checkout main
git merge --no-ff feat/persist-dig-deeper-research -m "Merge feat/persist-dig-deeper-research: reopen saved dig-deeper, or dig again"
git push origin main
```

(Keep the branch on origin — never delete it. Per CLAUDE.md the full step-by-step history stays visible to a reviewer.)

---

## Self-Review (plan vs spec)

**Spec coverage:**
- §1 data model (single `research` table, JSON, cascade) → Task 1 (Steps 3–4). ✅
- §2 save on completion + retain per-angle errors → Task 2. ✅
- §3 load command → Task 1 (`get_research`) + Task 3 (Tauri command + api). ✅
- §4 saved-first reopen flow (crux) → Task 4 (Step 4). ✅
- §5 saved view UI (findings, failed reason, "researched Xh ago", "Dig deeper again") → Task 4 (Steps 3, 5). ✅
- §6 testing (Rust round-trip/none/upsert/cascade + live 0-claude proof) → Task 1 tests + Task 5. ✅
- Resolved decisions (single-table, timestamp yes, failed+reason, latest-wins) → all reflected. ✅

**Placeholder scan:** no TBD/TODO/"handle edge cases"; every code step shows full code. ✅

**Type consistency:** `SavedAngle`/`SavedResearch` (db) → `SavedAngleDto`/`SavedResearchDto` (commands, `sections: Vec<agent::BriefSection>`) → `SavedAngle`/`SavedResearch` (TS, `createdAt`). `save_research(conn, feed_item_id, &Brief, &[SavedAngle], now)` consumed correctly in Task 2. `LaneAngle = SwarmAngle & { findings? }` used consistently in Task 4. `get_research` signature matches across Tasks 1/3. ✅

**Note on Task 3:** `SavedResearchDto.sections` is `Vec<agent::BriefSection>` (reuses the existing camelCase `BriefSection`) — not a bespoke section type.
