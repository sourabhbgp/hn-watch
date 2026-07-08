# HN Watch — Requirements

**Source of truth.** The block below is the **verbatim** assignment requirement, as received
in the brief email. It is the anchor for scope — check every piece of work against it so we
**neither over-build nor under-build**. Do not paraphrase, trim, or "improve" this section;
if the brief is ever restated, replace this text with the new verbatim wording.

## Requirement (verbatim)

> Build a small, native desktop app in Tauri called HN Watch. The app lets a user create
> "monitors," each made up of a natural-language prompt describing what they care about (e.g.
> "AI-agent startup launches" or "Rust async runtime discussions") and a schedule. Each monitor
> runs as a long-lived background worker in the Rust layer: on every tick it pulls recent content
> from Hacker News, passes those items along with the user's prompt to Claude Code in headless
> mode (claude -p) to judge what's relevant and summarize it, and appends the matches to a single
> Twitter-style feed in the UI. New results should be deduplicated against what's already been
> seen, persisted locally so monitors and their feed survive an app restart, and the app should
> keep running in the system tray with the window closed and fire a native notification when new
> items land. claude -p is the agent runtime throughout, so you'll need Claude Code installed
> locally; how you get Hacker News data and how you structure everything around the agent calls
> is up to you.
>
> On top of that, each item in the feed has a "dig deeper" action that kicks off a small research
> swarm: an orchestrator in the Rust layer spins up several claude -p agents that run in parallel,
> each investigating the story from a different angle, streaming their progress to a live view and
> then compiling into one combined brief. The scheduled monitors and the on-demand swarm exercise
> the same runtime in different ways — one call per tick versus many at once — and we're
> interested in how you handle that. Keep the app lightweight, feel free to stub anything that's
> purely incidental plumbing, and plan to spend roughly a weekend on it. A short README walking
> through your design decisions and trade-offs, and how to run it, is part of what we're
> evaluating.

## Scope guardrails (derived — for over/under-build checks)

Extracted from the verbatim text above; if these ever disagree with it, the verbatim text wins.

**Must have**
- Native desktop app in **Tauri**, called **HN Watch**.
- **Monitors** created by the user: a natural-language prompt + a schedule.
- Each monitor = a **long-lived background worker in the Rust layer**; on every tick it pulls
  recent Hacker News content, passes items + the prompt to **`claude -p`** (headless) to judge
  relevance and summarize, and appends matches to **one Twitter-style feed**.
- **Dedup** against already-seen items; **local persistence** so monitors + feed survive restart.
- Keeps running in the **system tray with the window closed**; **native notification** on new items.
- **`claude -p` is the agent runtime throughout** (Claude Code installed locally).
- **"Dig deeper"** per feed item: a **Rust orchestrator** spins up **several parallel `claude -p`
  agents**, each a different angle, **streaming live → one combined brief**.
- Handle the **same runtime at two tempos** — one call per tick (monitors) vs. many at once
  (swarm). *How this is handled is explicitly what's being evaluated.*
- A short **README** covering design decisions, trade-offs, and how to run.

**Explicitly left to us / allowed**
- How Hacker News data is obtained, and how everything around the agent calls is structured.
- **Stub anything that's purely incidental plumbing.**
- Keep it **lightweight** — roughly a **weekend** of scope.

**Don't over-build:** nothing beyond the above; incidental plumbing may be stubbed rather than
fully built.
