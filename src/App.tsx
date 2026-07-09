import { useEffect, useMemo, useState } from "react";
import type { FeedItem, Monitor } from "./types";
import { BRIEF_F1 } from "./mock/data";
import { Sidebar } from "./components/Sidebar";
import { Feed } from "./components/Feed";
import { DigDeeperPanel } from "./components/DigDeeperPanel";
import { listMonitors, listFeed, createMonitor, deleteMonitor, onFeedUpdated } from "./api";

function App() {
  const [monitors, setMonitors] = useState<Monitor[]>([]);
  const [feed, setFeed] = useState<FeedItem[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState<string | null>(null);
  const [digItem, setDigItem] = useState<FeedItem | null>(null);

  const refresh = async () => {
    setMonitors(await listMonitors());
    setFeed(await listFeed());
  };

  useEffect(() => {
    refresh();
    const un = onFeedUpdated(() => refresh());
    return () => {
      un.then((f) => f());
    };
  }, []);

  const activeMonitor = useMemo(
    () => monitors.find((m) => m.id === selectedMonitorId) ?? null,
    [monitors, selectedMonitorId],
  );

  const visibleFeed = useMemo(
    () => (selectedMonitorId ? feed.filter((f) => f.monitorId === selectedMonitorId) : feed),
    [feed, selectedMonitorId],
  );

  const handleCreate = async (name: string, prompt: string, intervalSecs: number) => {
    await createMonitor(name, prompt, intervalSecs);
    await refresh();
  };

  const handleDelete = async (id: string) => {
    await deleteMonitor(id);
    if (selectedMonitorId === id) setSelectedMonitorId(null);
    await refresh();
  };

  return (
    <div className="flex h-full w-full overflow-hidden">
      <Sidebar
        monitors={monitors}
        selectedId={selectedMonitorId}
        onSelect={setSelectedMonitorId}
        onCreate={handleCreate}
        onDelete={handleDelete}
      />

      <Feed
        items={visibleFeed}
        monitors={monitors}
        activeMonitor={activeMonitor}
        onDigDeeper={setDigItem}
      />

      {digItem && (
        <DigDeeperPanel
          item={digItem}
          brief={digItem.id === BRIEF_F1.itemId ? BRIEF_F1 : null}
          onClose={() => setDigItem(null)}
        />
      )}
    </div>
  );
}

export default App;
