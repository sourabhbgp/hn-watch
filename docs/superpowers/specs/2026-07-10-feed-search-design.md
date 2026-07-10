# Feed search — design

**Date:** 2026-07-10
**Ticket:** User-requested enhancement (feed keyword/topic search)
**Branch:** `feat/feed-search`
**Scope source of truth:** [`docs/REQUIREMENTS.md`](../../REQUIREMENTS.md)

## Problem

The feed is one long, recency-ordered list that grows without bound (capped at the newest
1000 items shipped to the UI — see Session 10). The only way to narrow it today is to click a
monitor in the sidebar, which filters to that monitor's matches. There is no way to find a
specific **topic or keyword** — "that Rust story from last week", "anything mentioning
Postgres" — without scrolling the whole list by eye.

Search is **not** part of the verbatim brief (`docs/REQUIREMENTS.md`); it is a deliberate,
user-requested usability enhancement. It is scoped to stay small and frontend-only so it does
not compete with the remaining core work (the dig-deeper swarm).

## Goal (acceptance)

- A search input in the feed header. Typing filters the visible feed **live** to cards whose
  text matches the query.
- Search **stacks on the current view**: if a monitor is selected, it searches that monitor's
  matches; on the unfiltered feed, it searches everything loaded.
- Matching is over **title + AI summary + reason**, case-insensitive, and multi-word (every
  whitespace-separated term must appear).
- The match count reflects the filtered result (e.g. `12 of 340 matches`).
- A query that matches nothing shows a clear empty state (`No matches for "…"`).
- **Zero backend change**: no Rust, no schema, no new Tauri command, no `types.ts` change, no
  new dependency. Nothing about the tick loop, dedup, ingestion, or persistence is touched.

## Decisions (locked in brainstorming)

- **Client-side filter, not backend search.** The feed is already fully in memory in `App.tsx`.
  Search is one more filter step composed with the existing monitor filter — the DRY choice, and
  it reuses the exact `useMemo`-filter pattern already there. A backend `LIKE`/FTS5 command was
  considered and **rejected** as over-build for a weekend feature: its only real advantage is
  searching beyond the 1000-item cap, which is a knowingly-accepted tradeoff already.
- **Fields = title + summary + reason.** The meaningful content of a card. Domain and monitor
  name are excluded to keep results high signal (a monitor filter already exists in the sidebar).
- **Match against the current view.** Search narrows what you are already looking at, rather than
  overriding the selected monitor — predictable composition.
- **Live as you type**, case-insensitive, multi-term AND. No submit button, no debounce needed
  (filtering an in-memory array of ≤1000 is instant).
- **Query clears when the monitor selection changes.** Each view starts fresh; avoids a stale
  query silently hiding a newly-selected monitor's matches.

## Architecture

Two small frontend pieces plus one pure, unit-testable helper. No file outside `src/` changes.

### 1. Pure matcher — new `src/lib/search.ts`

```ts
export function matchesQuery(item: FeedItem, query: string): boolean;
```

- Trim + lowercase the query; empty query → `true` (no-op, everything passes).
- Split the query on whitespace into terms.
- Build the haystack once: `` `${item.title} ${item.summary} ${item.reason}`.toLowerCase() ``.
- Return `true` iff **every** term is a substring of the haystack (AND semantics).

Pure and dependency-free. The repo has **no JS test runner** (all unit tests are Rust; the
frontend is verified in the real native window per `docs/TESTING.md`), so `matchesQuery`'s
behavior — empty query, single term, multi-term AND, case-insensitivity, no-match — is verified
live. It stays pure so a future JS runner could unit-test it directly with no refactor.

### 2. Query state + composition — `src/App.tsx`

- Add `const [query, setQuery] = useState("")` alongside `selectedMonitorId`.
- Extend the existing `visibleFeed` memo to apply `matchesQuery` **after** the monitor filter:

  ```ts
  const visibleFeed = useMemo(() => {
    const byMonitor = selectedMonitorId
      ? feed.filter((f) => f.monitorId === selectedMonitorId)
      : feed;
    const q = query.trim();
    return q ? byMonitor.filter((f) => matchesQuery(f, q)) : byMonitor;
  }, [feed, selectedMonitorId, query]);
  ```

- Clear the query when the selected monitor changes. Simplest: wrap `setSelectedMonitorId` in a
  handler that also calls `setQuery("")` (or a `useEffect` keyed on `selectedMonitorId`). The
  wrapper is preferred — explicit, no effect indirection.
- Pass `query`, `setQuery`, and the **pre-search** count (so the header can show `X of Y`) down to
  `Feed`.

### 3. Search input — `src/components/Feed.tsx`

- A slim `<input type="search">` in the existing header row (`Feed.tsx:43`), placeholder
  `Search this feed…`, styled with existing design tokens only (`border-line`, `text-faint`,
  `hn-soft`/focus ring per the current token set) — **no new colors**.
- A small clear (`×`) affordance shown only when the query is non-empty; clicking it calls
  `setQuery("")`.
- The count span becomes `{shown} of {total} matches` when a query is active, falling back to the
  current `{n} matches` text when it is empty.
- The virtualizer already keys off `items` — it re-windows automatically as the filtered list
  shrinks/grows. No virtualization change needed.

### 4. Empty state — `Feed.tsx` `emptyMessage`

When `items.length === 0` **and** a query is active, show `No matches for "{query}"` instead of
the existing monitor-status messages (which assume "no matches yet from ticking", not "filtered
out"). The query-active branch takes precedence.

## Data flow

```
App: feed (≤1000, in memory)
  → filter by selectedMonitorId   (existing)
  → filter by matchesQuery(query) (new)
  → visibleFeed → Feed → virtualized cards
```

Nothing round-trips to Rust. The feed array is unchanged; search only affects what `visibleFeed`
yields to the render.

## Known limitation (documented, by design)

Search covers the **newest 1000 items** the backend ships (`db::list_feed` `LIMIT 1000`), not the
entire history. This is the same recency-cap tradeoff Session 10 accepted for the feed itself and
is consistent with a watchtower (recency-first). If full-history search is ever wanted, the
follow-up is a backend FTS5 command — noted in `docs/TODO.md`, not built here.

## Non-goals (YAGNI / out of scope)

- No backend / SQL / FTS5 search; no new Tauri command; no `types.ts` change.
- No regex, fuzzy matching, ranking, or highlighting of matched terms.
- No search across domain or monitor name (monitor filtering already exists in the sidebar).
- No debounce, no submit button, no search history / saved searches.
- No persistence of the query across app restarts.

## Testing

- **Matcher behavior** — no JS test runner exists in this repo, so `matchesQuery` is exercised
  through the live checks below rather than a unit test: empty query passes all; single-term
  substring; multi-term AND (all must match); case-insensitivity; a term absent → no match.
- **Live in the real native window** (`docs/TESTING.md`): type a term present in some card
  summaries → feed narrows, count shows `X of Y`; add a second term → narrows further (AND);
  clear → full feed returns; select a monitor then search → searches only that monitor; switch
  monitors → query clears; a nonsense query → `No matches for "…"` empty state.
- `tsc` + `vite build` clean; `cargo test` unaffected (no Rust touched).

## Files touched

| File | Change |
| --- | --- |
| `src/lib/search.ts` | **new** — pure `matchesQuery` helper |
| `src/App.tsx` | `query` state, extend `visibleFeed`, clear-on-monitor-change, pass props |
| `src/components/Feed.tsx` | search input + clear button, `X of Y` count, query-aware empty state |
