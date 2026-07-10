# Feed Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a live, client-side keyword search box to the feed that filters the visible cards by title + summary + reason, stacking on top of the existing monitor filter.

**Architecture:** Pure frontend. One dependency-free matcher helper (`matchesQuery`), a `query` state lifted into `App.tsx` that composes with the existing monitor filter in the `visibleFeed` memo, and a search input + clear button + `X of Y` count + query-aware empty state in `Feed.tsx`. No Rust, no schema, no new dependency, no `types.ts` change.

**Tech Stack:** React 19 + TypeScript, Tailwind v4 (design tokens in `src/index.css`), `@tanstack/react-virtual` (already wired in `Feed.tsx` — unaffected).

## Global Constraints

- **Scope source of truth:** `docs/REQUIREMENTS.md`. Search is a user-requested enhancement, not in the brief — keep it small and frontend-only.
- **Design tokens only** — no hardcoded colors/fonts/spacing. Available tokens (from `src/index.css`): `paper`, `card`, `line`, `ink`, `soft`, `faint`, `hn`, `hn-soft`, `hn-border`, `rust`, `ok`; fonts `font-sans`, `font-mono`.
- **No JS test runner exists** in this repo (all unit tests are Rust). Frontend verification is `tsc` + `vite build` (via `npm run build`) plus live testing in the **real native window** per `docs/TESTING.md` — never a browser at localhost.
- **DRY:** reuse the existing `useMemo` filter pattern in `App.tsx`; do not duplicate filtering logic.
- **Branch:** `feat/feed-search` (already created). Push to origin and keep it; merge into `main` with `--no-ff`.

---

### Task 1: Pure `matchesQuery` helper

**Files:**
- Create: `src/lib/search.ts`

**Interfaces:**
- Consumes: `FeedItem` from `../types` (fields used: `title`, `summary`, `reason` — all `string`).
- Produces: `export function matchesQuery(item: FeedItem, query: string): boolean` — consumed by `App.tsx` in Task 2.

**Behavior contract (verified live in Task 3, since there is no JS runner):**
- Empty / whitespace-only query → `true` (everything passes).
- Query lowercased and split on whitespace into terms; **every** term must be a substring of the lowercased `` `${title} ${summary} ${reason}` `` haystack (AND semantics).
- Case-insensitive.

- [ ] **Step 1: Create the helper**

Create `src/lib/search.ts`:

```ts
import type { FeedItem } from "../types";

/**
 * Client-side feed search. Case-insensitive; multiple whitespace-separated
 * terms must ALL appear (AND) across the card's title + summary + reason.
 * An empty/whitespace query matches everything (no-op filter).
 */
export function matchesQuery(item: FeedItem, query: string): boolean {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return true;
  const haystack = `${item.title} ${item.summary} ${item.reason}`.toLowerCase();
  return terms.every((t) => haystack.includes(t));
}
```

- [ ] **Step 2: Typecheck**

Run: `npm run build`
Expected: PASS — `tsc` clean, `vite build` succeeds. (`matchesQuery` is unused so far; TypeScript does not error on an unused exported function.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/search.ts
git commit -m "feat(search): pure matchesQuery helper (title+summary+reason, AND, case-insensitive)"
```

---

### Task 2: Wire search into the feed (state + UI)

App owns the query state and composes it with the monitor filter; Feed renders the input and reflects the filtered count. These land together because Feed's new props must exist for App to compile.

**Files:**
- Modify: `src/App.tsx` (add `query` state; split the feed memo; clear query on monitor change; pass props)
- Modify: `src/components/Feed.tsx` (search input, clear button, `X of Y` count, query-aware empty state)

**Interfaces:**
- Consumes: `matchesQuery` from `../lib/search` (Task 1).
- Produces (Feed's new props): `totalCount: number`, `query: string`, `onQueryChange: (q: string) => void` added to the existing `Feed` props.

- [ ] **Step 1: App — import the matcher**

In `src/App.tsx`, add to the imports near the top (after the `./components/*` imports):

```ts
import { matchesQuery } from "./lib/search";
```

- [ ] **Step 2: App — add query state**

In `src/App.tsx`, add alongside the other `useState` calls (after the `selectedMonitorId` line, `App.tsx:24`):

```ts
  const [query, setQuery] = useState("");
```

- [ ] **Step 3: App — split the feed memo so search stacks on the monitor filter**

Replace the existing `visibleFeed` memo (`src/App.tsx:71-74`):

```ts
  const visibleFeed = useMemo(
    () => (selectedMonitorId ? feed.filter((f) => f.monitorId === selectedMonitorId) : feed),
    [feed, selectedMonitorId],
  );
```

with:

```ts
  // Feed filtered by the selected monitor only — used both for the search
  // haystack and for the "X of Y" total in the header.
  const monitorFeed = useMemo(
    () => (selectedMonitorId ? feed.filter((f) => f.monitorId === selectedMonitorId) : feed),
    [feed, selectedMonitorId],
  );

  // Search stacks on top of the monitor filter (empty query = no-op).
  const visibleFeed = useMemo(() => {
    const q = query.trim();
    return q ? monitorFeed.filter((f) => matchesQuery(f, q)) : monitorFeed;
  }, [monitorFeed, query]);
```

- [ ] **Step 4: App — clear the query when the monitor selection changes**

In `src/App.tsx`, add a handler near `handleDelete` (after `App.tsx:85`):

```ts
  const handleSelectMonitor = (id: string | null) => {
    setSelectedMonitorId(id);
    setQuery(""); // each view starts with a fresh search
  };
```

Then in `handleDelete`, the existing `if (selectedMonitorId === id) setSelectedMonitorId(null);` line (`App.tsx:83`) should also clear the query — change it to:

```ts
    if (selectedMonitorId === id) {
      setSelectedMonitorId(null);
      setQuery("");
    }
```

- [ ] **Step 5: App — pass the new props to Sidebar and Feed**

In `src/App.tsx`, change the `Sidebar`'s `onSelect` (`App.tsx:104`) from `onSelect={setSelectedMonitorId}` to:

```tsx
          onSelect={handleSelectMonitor}
```

And extend the `Feed` element (`App.tsx:111-116`) to:

```tsx
        <Feed
          items={visibleFeed}
          totalCount={monitorFeed.length}
          query={query}
          onQueryChange={setQuery}
          monitors={monitors}
          activeMonitor={activeMonitor}
          onDigDeeper={setDigItem}
        />
```

- [ ] **Step 6: Feed — accept the new props**

In `src/components/Feed.tsx`, extend the `Feed` function's prop destructuring and type (`Feed.tsx:15-25`) to include `totalCount`, `query`, `onQueryChange`:

```tsx
export function Feed({
  items,
  totalCount,
  query,
  onQueryChange,
  monitors,
  activeMonitor,
  onDigDeeper,
}: {
  items: FeedItem[];
  totalCount: number;
  query: string;
  onQueryChange: (q: string) => void;
  monitors: Monitor[];
  activeMonitor: Monitor | null;
  onDigDeeper: (item: FeedItem) => void;
}) {
```

- [ ] **Step 7: Feed — query-aware empty message**

In `src/components/Feed.tsx`, replace the `emptyMessage` helper (`Feed.tsx:6-13`) so an active query takes precedence:

```tsx
function emptyMessage(m: Monitor | null, query: string): string {
  if (query.trim()) return `No matches for “${query.trim()}”.`;
  if (m && m.lastError) return "Last check failed — see the monitor’s status.";
  if (m && m.lastCheckedAt != null) {
    return `Checked ${m.lastCheckedCount ?? 0} stories, nothing matched yet.`;
  }
  if (m) return "Checking…";
  return "No matches yet.";
}
```

And update its call site (`Feed.tsx:57`) from `{emptyMessage(activeMonitor)}` to:

```tsx
            {emptyMessage(activeMonitor, query)}
```

- [ ] **Step 8: Feed — search input + clear button + `X of Y` count in the header**

In `src/components/Feed.tsx`, replace the entire `<header>` block (`Feed.tsx:42-51`) with:

```tsx
      {/* header */}
      <header className="flex items-center gap-3 border-b border-line px-6 py-4">
        <h1 className="text-[17px] font-bold tracking-tight">
          {activeMonitor ? activeMonitor.name : "Feed"}
        </h1>
        <span className="font-mono text-[11.5px] text-faint">
          {query.trim() ? (
            <>
              {items.length} of {totalCount}{" "}
              {totalCount === 1 ? "match" : "matches"}
            </>
          ) : (
            <>
              {items.length} {items.length === 1 ? "match" : "matches"}
              {!activeMonitor && ` across ${monitors.length} monitors`}
            </>
          )}
        </span>
        <div className="relative ml-auto">
          <input
            type="search"
            value={query}
            onChange={(e) => onQueryChange(e.target.value)}
            placeholder="Search this feed…"
            className="w-56 rounded-md border border-line bg-card py-1.5 pl-3 pr-7 text-[12.5px] text-ink placeholder:text-faint focus:border-hn-border focus:outline-none"
          />
          {query && (
            <button
              type="button"
              onClick={() => onQueryChange("")}
              aria-label="Clear search"
              className="absolute right-2 top-1/2 -translate-y-1/2 text-faint hover:text-soft"
            >
              ×
            </button>
          )}
        </div>
      </header>
```

Note: the header's flex alignment changed from `items-baseline` to `items-center` so the input aligns vertically with the title; `ml-auto` pushes the search box to the right edge.

- [ ] **Step 9: Typecheck the whole wiring**

Run: `npm run build`
Expected: PASS — `tsc` clean (all new props typed and supplied), `vite build` succeeds, zero errors.

- [ ] **Step 10: Commit**

```bash
git add src/App.tsx src/components/Feed.tsx
git commit -m "feat(search): live feed search box — stacks on monitor filter, X of Y count, clear + empty state"
```

---

### Task 3: Live verification + STATUS + merge

**Files:**
- Modify: `STATUS.md` (add a Session entry)

- [ ] **Step 1: Build and launch the native app**

Run: `npm run tauri dev` (or a release build per `docs/TESTING.md`). Drive it with computer-use — **not** a browser at localhost.

- [ ] **Step 2: Verify each behavior live (checklist)**

Confirm in the real window:
- Typing a term present in some card summaries narrows the feed; the count reads `X of Y matches`.
- Adding a second term narrows further (AND — both terms must appear).
- Case-insensitivity: an uppercase query matches lowercase text.
- The `×` clear button appears when non-empty and restores the full feed when clicked.
- With a monitor selected, search filters only that monitor's matches; the total in `X of Y` equals that monitor's match count.
- Switching monitors (and deleting the selected monitor) clears the query.
- A nonsense query shows `No matches for “…”.`
- The virtualized list still scrolls correctly with a filtered subset.

- [ ] **Step 3: Update STATUS.md**

Add a new session entry to `STATUS.md` summarizing: client-side feed search (title+summary+reason, AND, case-insensitive), stacks on the monitor filter, `X of Y` count, clear button, query-aware empty state; frontend-only (no Rust/schema/deps); known 1000-cap limit tracked as TODO #7; verified live in the native window.

- [ ] **Step 4: Commit the STATUS update**

```bash
git add STATUS.md
git commit -m "docs: STATUS — feed search (client-side, frontend-only)"
```

- [ ] **Step 5: Push the branch and merge**

```bash
git push -u origin feat/feed-search
git checkout main
git merge --no-ff feat/feed-search -m "Merge feat/feed-search: client-side keyword search over the feed"
git push origin main
```

Keep the `feat/feed-search` branch on origin (do not delete).

---

## Self-Review

**Spec coverage** (against `docs/superpowers/specs/2026-07-10-feed-search-design.md`):
- Search input in header → Task 2 Step 8. ✅
- Live filter, stacks on current view → Task 2 Steps 3–5. ✅
- Match title+summary+reason, case-insensitive, multi-term AND → Task 1. ✅
- `X of Y` count → Task 2 Step 8. ✅
- Query clears on monitor change → Task 2 Step 4 (+ delete path). ✅
- Empty-state `No matches for "…"` → Task 2 Step 7. ✅
- No backend/types/deps change → nothing in the plan touches Rust, `types.ts`, or `package.json`. ✅
- Known 1000-cap limitation / FTS5 follow-up → already recorded as TODO #7 (out of this plan's scope). ✅

**Placeholder scan:** No TBD/TODO/"handle edge cases" — every code step shows complete code. ✅

**Type consistency:** `matchesQuery(item: FeedItem, query: string): boolean` defined in Task 1, called identically in Task 2 Step 3. Feed's new props (`totalCount: number`, `query: string`, `onQueryChange: (q: string) => void`) are defined in Task 2 Step 6 and supplied identically in Task 2 Step 5. `handleSelectMonitor(id: string | null)` matches the `onSelect` type Sidebar already consumed via `setSelectedMonitorId`. ✅
