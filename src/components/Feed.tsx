import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { FeedItem, Monitor } from "../types";
import { FeedCard } from "./FeedCard";

function emptyMessage(m: Monitor | null, query: string): string {
  if (query.trim()) return `No matches for “${query.trim()}”.`;
  if (m && m.lastError) return "Last check failed — see the monitor’s status.";
  if (m && m.lastCheckedAt != null) {
    return `Checked ${m.lastCheckedCount ?? 0} stories, nothing matched yet.`;
  }
  if (m) return "Checking…";
  return "No matches yet.";
}

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
  // Windowed rendering: only the cards in (or near) the viewport are mounted,
  // so the DOM stays a constant size no matter how large the feed grows. Card
  // heights vary (summary/reason length), so we measure dynamically rather than
  // assume a fixed row height.
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 200,
    overscan: 6,
    gap: 12, // matches the previous gap-3 between cards
    getItemKey: (index) => items[index].id,
  });

  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
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
          {query.trim() && (
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

      {/* scrolling feed */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto py-5">
        {items.length === 0 ? (
          <div className="mt-20 text-center text-[13px] text-faint">
            {emptyMessage(activeMonitor, query)}
          </div>
        ) : (
          <div
            className="relative w-full"
            style={{ height: virtualizer.getTotalSize() }}
          >
            {virtualizer.getVirtualItems().map((row) => {
              const item = items[row.index];
              return (
                <div
                  key={row.key}
                  data-index={row.index}
                  ref={virtualizer.measureElement}
                  className="absolute left-0 top-0 w-full"
                  style={{ transform: `translateY(${row.start}px)` }}
                >
                  <div className="mx-auto max-w-2xl px-6">
                    <FeedCard item={item} query={query} onDigDeeper={onDigDeeper} />
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </section>
  );
}
