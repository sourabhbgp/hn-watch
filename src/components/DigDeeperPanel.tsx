import { useEffect, useRef, useState } from "react";
import type { AngleStatus, FeedItem, PlannedAngle, SwarmAngle } from "../types";
import {
  startDigDeeper,
  confirmDigDeeper,
  cancelDigDeeper,
  onSwarmProgress,
  onSwarmAngleDone,
  onSwarmBriefReady,
  onSwarmFailed,
} from "../api";

const STATUS_STYLE: Record<AngleStatus, { chip: string; label: string }> = {
  queued: { chip: "bg-paper text-faint", label: "queued" },
  running: { chip: "bg-hn-soft text-rust", label: "running" },
  done: { chip: "bg-[#eaf3ea] text-ok", label: "done" },
  error: { chip: "bg-hn-soft text-rust", label: "failed" },
};

type Brief = { summary: string; sections: { heading: string; body: string }[] };
type Phase = "planning" | "confirm" | "running";

function AngleLane({ angle }: { angle: SwarmAngle }) {
  const s = STATUS_STYLE[angle.status];
  return (
    <div className="rounded-lg border border-line bg-card p-3">
      <div className="flex items-center gap-2">
        <span className="text-[14px]">{angle.icon}</span>
        <span className="text-[13px] font-semibold text-ink">{angle.label}</span>
        <span
          className={`ml-auto flex items-center gap-1 rounded-full px-2 py-0.5 font-mono text-[10px] ${s.chip}`}
        >
          {angle.status === "running" && (
            <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-rust" />
          )}
          {s.label}
        </span>
      </div>
      {angle.lines.length > 0 && (
        <div className="mt-2 space-y-1 border-l-2 border-line pl-3">
          {angle.lines.map((line, i) => (
            <p key={i} className="font-mono text-[11px] leading-snug text-soft">
              {line}
            </p>
          ))}
        </div>
      )}
      {angle.error && (
        <p className="mt-2 text-[11px] text-rust">{angle.error}</p>
      )}
    </div>
  );
}

export function DigDeeperPanel({ item, onClose }: { item: FeedItem; onClose: () => void }) {
  const [phase, setPhase] = useState<Phase>("planning");
  const [planned, setPlanned] = useState<PlannedAngle[]>([]);
  const [angles, setAngles] = useState<SwarmAngle[]>([]);
  const [brief, setBrief] = useState<Brief | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const started = useRef(false);

  // Mount: run the planner, subscribe to swarm events. Unmount: cancel + unlisten.
  // The panel is keyed by item id in App, so each item gets a fresh mount.
  useEffect(() => {
    let alive = true;
    startDigDeeper(item.id)
      .then((a) => {
        if (!alive) return;
        setPlanned(a);
        setPhase("confirm");
      })
      .catch((e) => alive && setFailed(String(e)));

    const subs = [
      onSwarmProgress((p) => {
        if (p.itemId !== item.id) return;
        setAngles((prev) =>
          prev.map((a) => (a.id === p.angleId ? { ...a, lines: [...a.lines, p.line] } : a)),
        );
      }),
      onSwarmAngleDone((p) => {
        if (p.itemId !== item.id) return;
        setAngles((prev) =>
          prev.map((a) =>
            a.id === p.angleId
              ? {
                  ...a,
                  status: p.error ? ("error" as const) : ("done" as const),
                  error: p.error ?? undefined,
                }
              : a,
          ),
        );
      }),
      onSwarmBriefReady((p) => p.itemId === item.id && setBrief(p.brief)),
      onSwarmFailed((p) => p.itemId === item.id && setFailed(p.error)),
    ];

    return () => {
      alive = false;
      cancelDigDeeper(item.id);
      subs.forEach((s) => s.then((un) => un()));
    };
  }, [item.id]);

  const removeAngle = (id: string) =>
    setPlanned((p) => (p.length > 2 ? p.filter((a) => a.id !== id) : p));

  const addAngle = () => {
    const focus = draft.trim();
    if (!focus || planned.length >= 5) return;
    const label = focus.length > 22 ? `${focus.slice(0, 22)}…` : focus;
    setPlanned((p) => [
      ...p,
      { id: `u-${Date.now()}`, icon: "🧭", label, focus },
    ]);
    setDraft("");
  };

  const start = () => {
    // All confirmed angles start at once (SWARM_PERMITS == MAX_ANGLES), so mark them running.
    setAngles(
      planned.map((a) => ({
        id: a.id,
        icon: a.icon,
        label: a.label,
        status: "running" as const,
        lines: [],
      })),
    );
    setPhase("running");
    started.current = true;
    confirmDigDeeper(item.id, planned).catch((e) => setFailed(String(e)));
  };

  const doneCount = angles.filter((a) => a.status === "done" || a.status === "error").length;

  return (
    <div className="fixed inset-0 z-40 flex justify-end">
      <div className="absolute inset-0 bg-black/20" onClick={onClose} aria-hidden />
      <div className="relative flex h-full w-[440px] flex-col border-l border-line bg-paper shadow-2xl">
        <header className="flex items-start gap-3 border-b border-line px-5 py-4">
          <div className="min-w-0">
            <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-rust">
              Research swarm
            </div>
            <h2 className="mt-1 line-clamp-2 text-[14px] font-bold leading-snug">{item.title}</h2>
          </div>
          <button
            onClick={onClose}
            className="ml-auto shrink-0 rounded-md px-2 py-1 text-[16px] text-faint hover:bg-card"
            aria-label="Close"
          >
            ✕
          </button>
        </header>

        <div className="flex-1 overflow-y-auto px-5 py-4">
          {failed ? (
            <div className="mt-16 text-center text-[13px] text-rust">{failed}</div>
          ) : phase === "planning" ? (
            <div className="mt-16 text-center text-[13px] text-faint">Planning angles…</div>
          ) : phase === "confirm" ? (
            <>
              <p className="mb-2 text-[12px] text-soft">
                Proposed angles — remove any, or add your own (2–5). Type a word or a full sentence
                for a specific focus.
              </p>
              <div className="flex flex-wrap gap-2">
                {planned.map((a) => (
                  <span
                    key={a.id}
                    className="flex items-center gap-1 rounded-full bg-hn-soft px-2.5 py-1 text-[12px] text-rust"
                    title={a.focus}
                  >
                    {a.icon} {a.label}
                    <button
                      onClick={() => removeAngle(a.id)}
                      disabled={planned.length <= 2}
                      className="ml-1 text-faint hover:text-rust disabled:opacity-30"
                      aria-label={`Remove ${a.label}`}
                    >
                      ✕
                    </button>
                  </span>
                ))}
              </div>
              <div className="mt-3 flex gap-2">
                <input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addAngle()}
                  placeholder="Add an angle…"
                  disabled={planned.length >= 5}
                  className="flex-1 rounded-md border border-line bg-card px-3 py-1.5 text-[12.5px] text-ink placeholder:text-faint focus:border-hn-border focus:outline-none disabled:opacity-40"
                />
                <button
                  onClick={addAngle}
                  disabled={!draft.trim() || planned.length >= 5}
                  className="rounded-md border border-line px-3 py-1.5 text-[12.5px] text-soft hover:bg-card disabled:opacity-40"
                >
                  Add
                </button>
              </div>
              <button
                onClick={start}
                className="mt-4 w-full rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white hover:opacity-90"
              >
                Start research ({planned.length} {planned.length === 1 ? "agent" : "agents"})
              </button>
            </>
          ) : (
            <>
              <div className="mb-2 flex items-center justify-between">
                <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                  Agents
                </span>
                <span className="font-mono text-[11px] text-faint">
                  {doneCount}/{angles.length} done
                </span>
              </div>
              <div className="space-y-2">
                {angles.map((a) => (
                  <AngleLane key={a.id} angle={a} />
                ))}
              </div>

              {brief && (
                <>
                  <div className="mt-6 mb-2 flex items-center gap-2">
                    <span className="text-[14px]">🧩</span>
                    <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                      Combined brief
                    </span>
                  </div>
                  <div className="rounded-xl border border-line bg-card p-4">
                    <p className="text-[13px] leading-relaxed text-soft">{brief.summary}</p>
                    <div className="mt-4 space-y-3">
                      {brief.sections.map((sec) => (
                        <div key={sec.heading}>
                          <h3 className="text-[12.5px] font-bold text-ink">{sec.heading}</h3>
                          <p className="mt-0.5 text-[12.5px] leading-relaxed text-soft">
                            {sec.body}
                          </p>
                        </div>
                      ))}
                    </div>
                  </div>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
