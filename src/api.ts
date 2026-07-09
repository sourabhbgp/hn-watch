import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Monitor, FeedItem } from "./types";

export const listMonitors = () => invoke<Monitor[]>("list_monitors");
export const listFeed = () => invoke<FeedItem[]>("list_feed");

export const createMonitor = (name: string, prompt: string, intervalSecs: number) =>
  invoke<Monitor>("create_monitor", { name, prompt, intervalSecs });

export const deleteMonitor = (id: string) => invoke<void>("delete_monitor", { id });

// Fires whenever a tick inserts new matches. Returns an unlisten function.
export const onFeedUpdated = (cb: () => void) => listen("feed-updated", cb);

export interface TickFinished {
  monitorId: string;
  checkedCount: number;
  newCount: number;
  error: string | null;
}

// Fires when a monitor begins a tick. Returns an unlisten function.
export const onTickStarted = (cb: (monitorId: string) => void) =>
  listen<{ monitorId: string }>("tick-started", (e) => cb(e.payload.monitorId));

// Fires when a monitor finishes a tick (even with 0 new). Returns an unlisten function.
export const onTickFinished = (cb: (p: TickFinished) => void) =>
  listen<TickFinished>("tick-finished", (e) => cb(e.payload));
