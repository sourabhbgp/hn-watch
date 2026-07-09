import { useEffect, useMemo, useState } from "react";
import type { ClaudeHealth, FeedItem, Monitor } from "./types";
import { BRIEF_F1 } from "./mock/data";
import { Sidebar } from "./components/Sidebar";
import { Feed } from "./components/Feed";
import { DigDeeperPanel } from "./components/DigDeeperPanel";
import { ClaudeBanner } from "./components/ClaudeBanner";
import {
  listMonitors,
  listFeed,
  createMonitor,
  deleteMonitor,
  onFeedUpdated,
  onTickStarted,
  onTickFinished,
  getClaudeHealth,
  recheckClaude,
  onClaudeHealth,
} from "./api";

function App() {
  const [monitors, setMonitors] = useState<Monitor[]>([]);
  const [feed, setFeed] = useState<FeedItem[]>([]);
  const [selectedMonitorId, setSelectedMonitorId] = useState<string | null>(null);
  const [digItem, setDigItem] = useState<FeedItem | null>(null);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [checkingIds, setCheckingIds] = useState<Set<string>>(new Set());
  const [health, setHealth] = useState<ClaudeHealth>({ status: "ok", message: "" });
  const [rechecking, setRechecking] = useState(false);

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
    getClaudeHealth().then(setHealth);
    const uHealth = onClaudeHealth((h) => {
      setHealth(h);
      listMonitors().then(setMonitors);
    });
    const tick = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 15000);
    return () => {
      uFeed.then((f) => f());
      uStart.then((f) => f());
      uFinish.then((f) => f());
      uHealth.then((f) => f());
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

  const handleRecheck = async () => {
    setRechecking(true);
    try {
      setHealth(await recheckClaude());
      await refresh();
    } finally {
      setRechecking(false);
    }
  };

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <ClaudeBanner health={health} onRecheck={handleRecheck} rechecking={rechecking} />
      <div className="flex min-h-0 flex-1 overflow-hidden">
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
    </div>
  );
}

export default App;
