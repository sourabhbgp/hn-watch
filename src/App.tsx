import { useEffect, useMemo, useState } from "react";
import type { FeedItem, Monitor } from "./types";
import { BRIEF_F1 } from "./mock/data";
import { Sidebar } from "./components/Sidebar";
import { Feed } from "./components/Feed";
import { DigDeeperPanel } from "./components/DigDeeperPanel";
import {
  listMonitors,
  listFeed,
  createMonitor,
  deleteMonitor,
  onFeedUpdated,
  onTickStarted,
  onTickFinished,
} from "./api";

function App() {
  const [monitors, setMonitors] = useState<Monitor[]>([]);
  const [feed, setFeed] = useState<FeedItem[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState<string | null>(null);
  const [digItem, setDigItem] = useState<FeedItem | null>(null);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [checkingIds, setCheckingIds] = useState<Set<string>>(new Set());

  const refresh = async () => {
    setMonitors(await listMonitors());
    setFeed(await listFeed());
  };

  useEffect(() => {
    refresh();
    const uFeed = onFeedUpdated(() => refresh());
    const uStart = onTickStarted((id) =>
      setCheckingIds((s) => new Set(s).add(id)),
    );
    const uFinish = onTickFinished(({ monitorId }) => {
      setCheckingIds((s) => {
        const n = new Set(s);
        n.delete(monitorId);
        return n;
      });
      // pull the freshly persisted stats for this tick
      listMonitors().then(setMonitors);
    });
    const tick = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 15000);
    return () => {
      uFeed.then((f) => f());
      uStart.then((f) => f());
      uFinish.then((f) => f());
      clearInterval(tick);
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
        now={now}
        checkingIds={checkingIds}
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
