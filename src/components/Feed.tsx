import type { FeedItem, Monitor } from "../types";
import { FeedCard } from "./FeedCard";

function emptyMessage(m: Monitor | null): string {
  if (m && m.lastCheckedAt != null) {
    return `Checked ${m.lastCheckedCount ?? 0} stories, nothing matched yet.`;
  }
  if (m) return "Checking…";
  return "No matches yet.";
}

export function Feed({
  items,
  monitors,
  activeMonitor,
  onDigDeeper,
}: {
  items: FeedItem[];
  monitors: Monitor[];
  activeMonitor: Monitor | null;
  onDigDeeper: (item: FeedItem) => void;
}) {
  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
      {/* header */}
      <header className="flex items-baseline gap-3 border-b border-line px-6 py-4">
        <h1 className="text-[17px] font-bold tracking-tight">
          {activeMonitor ? activeMonitor.name : "Feed"}
        </h1>
        <span className="font-mono text-[11.5px] text-faint">
          {items.length} {items.length === 1 ? "match" : "matches"}
          {!activeMonitor && ` across ${monitors.length} monitors`}
        </span>
        <span className="ml-auto flex items-center gap-1.5 font-mono text-[11px] text-faint">
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-ok" />
          live
        </span>
      </header>

      {/* scrolling feed */}
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-2xl flex-col gap-3 px-6 py-5">
          {items.length === 0 ? (
            <div className="mt-20 text-center text-[13px] text-faint">
              {emptyMessage(activeMonitor)}
            </div>
          ) : (
            items.map((item) => (
              <FeedCard key={item.id} item={item} onDigDeeper={onDigDeeper} />
            ))
          )}
        </div>
      </div>
    </section>
  );
}
