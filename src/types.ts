// Shared UI types. These mirror what the Rust core will eventually send over
// Tauri events/commands — for now they're populated from mock data.

export type MonitorStatus = "active" | "paused" | "error";

export interface ClaudeHealth {
  status: "ok" | "missing" | "notAuthenticated";
  message: string;
}

export interface Monitor {
  id: string;
  name: string;
  prompt: string;
  intervalLabel: string; // e.g. "every 30m"
  status: MonitorStatus;
  matchCount: number;
  lastCheckedAt: number | null; // epoch seconds
  nextCheckAt: number | null; // epoch seconds
  lastCheckedCount: number | null;
  lastNewCount: number | null;
  lastError: string | null;
}

export interface FeedItem {
  id: string;
  monitorId: string;
  monitorName: string;
  title: string;
  url: string;
  domain: string;
  summary: string;
  reason: string; // why the monitor's prompt considered this a match
  hnScore: number;
  hnComments: number;
  timeAgo: string;
}

export type AngleStatus = "queued" | "running" | "done" | "error";

export interface SwarmAngle {
  id: string;
  icon: string;
  label: string;
  status: AngleStatus;
  lines: string[]; // streamed progress lines from the agent
  error?: string; // failure reason when status === "error"
}

export interface PlannedAngle {
  id: string;
  icon: string;
  label: string;
  focus: string;
}

export interface BriefSection {
  heading: string;
  body: string;
}

export interface Brief {
  itemId: string;
  angles: SwarmAngle[];
  summary: string;
  sections: BriefSection[];
}
