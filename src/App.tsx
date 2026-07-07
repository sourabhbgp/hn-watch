import { useMemo, useState } from "react";
import type { FeedItem } from "./types";
import { MONITORS, FEED, BRIEF_F1 } from "./mock/data";
import { Sidebar } from "./components/Sidebar";
import { Feed } from "./components/Feed";
import { DigDeeperPanel } from "./components/DigDeeperPanel";

function App() {
  // null = "All matches"; otherwise a monitor id
  const [selectedMonitorId, setSelectedMonitorId] = useState<string | null>(null);
  const [digItem, setDigItem] = useState<FeedItem | null>(null);

  const activeMonitor = useMemo(
    () => MONITORS.find((m) => m.id === selectedMonitorId) ?? null,
    [selectedMonitorId],
  );

  const visibleFeed = useMemo(
    () =>
      selectedMonitorId
        ? FEED.filter((f) => f.monitorId === selectedMonitorId)
        : FEED,
    [selectedMonitorId],
  );

  return (
    <div className="flex h-full w-full overflow-hidden">
      <Sidebar
        monitors={MONITORS}
        selectedId={selectedMonitorId}
        onSelect={setSelectedMonitorId}
        onNew={() => {
          /* no-op in the static shell — wired up in a later phase */
        }}
      />

      <Feed
        items={visibleFeed}
        monitors={MONITORS}
        activeMonitor={activeMonitor}
        onDigDeeper={setDigItem}
      />

      {digItem && (
        <DigDeeperPanel
          item={digItem}
          // only f1 has a mock brief; others show the "spinning up" state
          brief={digItem.id === BRIEF_F1.itemId ? BRIEF_F1 : null}
          onClose={() => setDigItem(null)}
        />
      )}
    </div>
  );
}

export default App;
