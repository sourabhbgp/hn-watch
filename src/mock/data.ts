import type { Monitor, FeedItem, Brief } from "../types";

// Static sample data so the UI shell has something realistic to render.
// Everything here gets replaced by live data from the Rust core in later phases.

export const MONITORS: Monitor[] = [
  {
    id: "m1",
    name: "AI-agent startups",
    prompt: "New AI-agent startup launches, funding rounds, or agent frameworks",
    intervalLabel: "every 30m",
    status: "active",
    matchCount: 12,
    lastCheckedAt: null,
    nextCheckAt: null,
    lastCheckedCount: null,
    lastNewCount: null,
    lastError: null,
  },
  {
    id: "m2",
    name: "Rust async",
    prompt: "Rust async runtime discussions — tokio, io_uring, executors",
    intervalLabel: "every 1h",
    status: "active",
    matchCount: 5,
    lastCheckedAt: null,
    nextCheckAt: null,
    lastCheckedCount: null,
    lastNewCount: null,
    lastError: null,
  },
  {
    id: "m3",
    name: "Local-first & SQLite",
    prompt: "Local-first software, embedded databases, SQLite internals",
    intervalLabel: "every 2h",
    status: "paused",
    matchCount: 3,
    lastCheckedAt: null,
    nextCheckAt: null,
    lastCheckedCount: null,
    lastNewCount: null,
    lastError: null,
  },
  {
    id: "m4",
    name: "Show HN — devtools",
    prompt: "Show HN posts for developer tools I could actually use",
    intervalLabel: "every 15m",
    status: "error",
    matchCount: 0,
    lastCheckedAt: null,
    nextCheckAt: null,
    lastCheckedCount: null,
    lastNewCount: null,
    lastError: null,
  },
];

export const FEED: FeedItem[] = [
  {
    id: "f1",
    monitorId: "m1",
    monitorName: "AI-agent startups",
    title: "Launch HN: Orbital (YC W26) – autonomous agents that file your taxes",
    url: "https://news.ycombinator.com/item?id=42011001",
    domain: "news.ycombinator.com",
    summary:
      "Two ex-Stripe engineers launched an agent that ingests your financial docs and prepares a filed-ready return, keeping a human in the loop for approval. Raised a $4M seed.",
    reason:
      "Direct AI-agent startup launch with funding — squarely matches the prompt.",
    hnScore: 214,
    hnComments: 96,
    timeAgo: "38m",
  },
  {
    id: "f2",
    monitorId: "m2",
    monitorName: "Rust async",
    title: "Why we moved our async runtime off tokio's multi-thread scheduler",
    url: "https://example.dev/blog/async-runtime",
    domain: "example.dev",
    summary:
      "A latency-sensitive service found tail latency wins by pinning tasks to a single-threaded runtime per core and sharding work, avoiding cross-core wakeups.",
    reason:
      "Deep dive on tokio scheduler internals and executor design — matches 'async runtime discussions'.",
    hnScore: 187,
    hnComments: 73,
    timeAgo: "1h",
  },
  {
    id: "f3",
    monitorId: "m1",
    monitorName: "AI-agent startups",
    title: "Show HN: Swarm – orchestrate many coding agents from your terminal",
    url: "https://github.com/example/swarm",
    domain: "github.com",
    summary:
      "An open-source orchestrator that fans out several coding agents in parallel, each on a git worktree, then merges the best result. MIT licensed.",
    reason:
      "New agent framework / orchestration tool — relevant to agent startups & tooling.",
    hnScore: 342,
    hnComments: 128,
    timeAgo: "2h",
  },
  {
    id: "f4",
    monitorId: "m3",
    monitorName: "Local-first & SQLite",
    title: "SQLite is not a toy database",
    url: "https://example.blog/sqlite-not-a-toy",
    domain: "example.blog",
    summary:
      "A survey of production SQLite usage — WAL mode, per-tenant databases, and why it outperforms client/server setups for read-heavy local workloads.",
    reason: "Directly about SQLite internals and local-first patterns.",
    hnScore: 511,
    hnComments: 203,
    timeAgo: "4h",
  },
  {
    id: "f5",
    monitorId: "m2",
    monitorName: "Rust async",
    title: "io_uring in Rust: a year in production",
    url: "https://example.io/io-uring-rust",
    domain: "example.io",
    summary:
      "Lessons from running an io_uring-based networking layer in Rust — completion-based APIs, buffer management, and where it beat epoll (and where it didn't).",
    reason: "io_uring executor discussion — matches the async runtime prompt.",
    hnScore: 156,
    hnComments: 41,
    timeAgo: "6h",
  },
];

// One compiled brief, keyed to feed item f1, shown in the Dig-deeper panel.
export const BRIEF_F1: Brief = {
  itemId: "f1",
  angles: [
    {
      id: "a1",
      icon: "🏢",
      label: "Company & people",
      status: "done",
      lines: [
        "Founders: 2 ex-Stripe engineers (payments infra).",
        "Incorporated 2025; YC W26 batch.",
        "Seed $4M — investors not fully disclosed.",
      ],
    },
    {
      id: "a2",
      icon: "🔧",
      label: "Tech & how it works",
      status: "done",
      lines: [
        "Document ingestion → structured extraction → agent reasoning.",
        "Human-in-the-loop approval before filing.",
        "Claims SOC 2 in progress.",
      ],
    },
    {
      id: "a3",
      icon: "📊",
      label: "Market & rivals",
      status: "running",
      lines: [
        "Comparing against incumbents (TurboTax, H&R Block)…",
        "Scanning for other agent-based tax startups…",
      ],
    },
    {
      id: "a4",
      icon: "🕵️",
      label: "Skeptic / risks",
      status: "queued",
      lines: [],
    },
  ],
  summary:
    "Orbital is an early-stage (YC W26, $4M seed) agent that prepares tax returns from ingested financial documents, keeping a human in the loop before filing. Strongest signal: credible founders and a real wedge; main risks are regulatory exposure and a crowded incumbent market.",
  sections: [
    {
      heading: "What it is",
      body: "An autonomous agent that ingests financial documents and produces a filing-ready tax return, with a mandatory human approval step. Positioned as 'an accountant that never sleeps.'",
    },
    {
      heading: "Who's behind it",
      body: "Two former Stripe engineers with payments-infrastructure backgrounds, going through YC's W26 batch on a $4M seed round.",
    },
    {
      heading: "Why it might work",
      body: "Tax prep is high-pain, structured, and annually recurring — a good fit for an agent with a human checkpoint. The founders' infra pedigree suggests they can handle the compliance plumbing.",
    },
    {
      heading: "Risks & open questions",
      body: "Heavy regulatory and liability exposure; incumbents (TurboTax, H&R Block) own distribution; unclear how errors are insured or who is accountable for a mis-filed return.",
    },
  ],
};
