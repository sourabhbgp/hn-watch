import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Monitor, FeedItem, ClaudeHealth, PlannedAngle, BriefSection } from "./types";

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

// Current Claude availability (drives the top banner + paused status).
export const getClaudeHealth = () => invoke<ClaudeHealth>("claude_health");

// Re-run the startup preflight on demand (banner "Re-check" button).
export const recheckClaude = () => invoke<ClaudeHealth>("recheck_claude");

// Fires when Claude health changes (preflight, recheck, or a tick flip).
export const onClaudeHealth = (cb: (h: ClaudeHealth) => void) =>
  listen<ClaudeHealth>("claude-health", (e) => cb(e.payload));

// --- Dig-deeper research swarm ---

// Run the planner for a feed item; returns the proposed (editable) angles.
export const startDigDeeper = (itemId: string) =>
  invoke<PlannedAngle[]>("start_dig_deeper", { itemId });

// Confirm the edited angle list and start the swarm.
export const confirmDigDeeper = (itemId: string, angles: PlannedAngle[]) =>
  invoke<void>("confirm_dig_deeper", { itemId, angles });

// Cancel a running swarm (panel closed / item switched).
export const cancelDigDeeper = (itemId: string) =>
  invoke<void>("cancel_dig_deeper", { itemId });

export interface SavedAngle {
  id: string;
  icon: string;
  label: string;
  focus: string;
  status: "done" | "failed";
  findings: string | null;
  error: string | null;
}
export interface SavedResearch {
  summary: string;
  sections: BriefSection[];
  angles: SavedAngle[];
  createdAt: number; // epoch seconds
}

// Load saved research for a feed item (null if never dug into). Spawns no claude.
export const getResearch = (itemId: string) =>
  invoke<SavedResearch | null>("get_research", { itemId });

export interface SwarmProgress { itemId: string; angleId: string; line: string }
export interface SwarmAngleDone {
  itemId: string;
  angleId: string;
  output: string | null;
  error: string | null;
}
export interface SwarmBriefReady {
  itemId: string;
  brief: { summary: string; sections: BriefSection[] };
}
export interface SwarmFailed { itemId: string; error: string }

export const onSwarmProgress = (cb: (p: SwarmProgress) => void) =>
  listen<SwarmProgress>("swarm-progress", (e) => cb(e.payload));
export const onSwarmAngleDone = (cb: (p: SwarmAngleDone) => void) =>
  listen<SwarmAngleDone>("swarm-angle-done", (e) => cb(e.payload));
export const onSwarmBriefReady = (cb: (p: SwarmBriefReady) => void) =>
  listen<SwarmBriefReady>("swarm-brief-ready", (e) => cb(e.payload));
export const onSwarmFailed = (cb: (p: SwarmFailed) => void) =>
  listen<SwarmFailed>("swarm-failed", (e) => cb(e.payload));
