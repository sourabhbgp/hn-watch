import type { AngleStatus, Brief, FeedItem, SwarmAngle } from "../types";

const STATUS_STYLE: Record<AngleStatus, { chip: string; label: string }> = {
  queued: { chip: "bg-paper text-faint", label: "queued" },
  running: { chip: "bg-hn-soft text-rust", label: "running" },
  done: { chip: "bg-[#eaf3ea] text-ok", label: "done" },
};

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
    </div>
  );
}

export function DigDeeperPanel({
  item,
  brief,
  onClose,
}: {
  item: FeedItem;
  brief: Brief | null;
  onClose: () => void;
}) {
  const done = brief ? brief.angles.filter((a) => a.status === "done").length : 0;
  const total = brief ? brief.angles.length : 0;

  return (
    <div className="fixed inset-0 z-40 flex justify-end">
      {/* backdrop */}
      <div
        className="absolute inset-0 bg-black/20"
        onClick={onClose}
        aria-hidden
      />

      {/* panel */}
      <div className="relative flex h-full w-[440px] flex-col border-l border-line bg-paper shadow-2xl">
        {/* header */}
        <header className="flex items-start gap-3 border-b border-line px-5 py-4">
          <div className="min-w-0">
            <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-rust">
              Research swarm
            </div>
            <h2 className="mt-1 line-clamp-2 text-[14px] font-bold leading-snug">
              {item.title}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="ml-auto shrink-0 rounded-md px-2 py-1 text-[16px] text-faint hover:bg-card"
            aria-label="Close"
          >
            ✕
          </button>
        </header>

        {/* body */}
        <div className="flex-1 overflow-y-auto px-5 py-4">
          {!brief ? (
            <div className="mt-16 text-center text-[13px] text-faint">
              Spinning up agents…
            </div>
          ) : (
            <>
              {/* swarm status */}
              <div className="mb-2 flex items-center justify-between">
                <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                  Agents
                </span>
                <span className="font-mono text-[11px] text-faint">
                  {done}/{total} done
                </span>
              </div>
              <div className="space-y-2">
                {brief.angles.map((a) => (
                  <AngleLane key={a.id} angle={a} />
                ))}
              </div>

              {/* compiled brief */}
              <div className="mt-6 mb-2 flex items-center gap-2">
                <span className="text-[14px]">🧩</span>
                <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
                  Combined brief
                </span>
              </div>
              <div className="rounded-xl border border-line bg-card p-4">
                <p className="text-[13px] leading-relaxed text-soft">
                  {brief.summary}
                </p>
                <div className="mt-4 space-y-3">
                  {brief.sections.map((sec) => (
                    <div key={sec.heading}>
                      <h3 className="text-[12.5px] font-bold text-ink">
                        {sec.heading}
                      </h3>
                      <p className="mt-0.5 text-[12.5px] leading-relaxed text-soft">
                        {sec.body}
                      </p>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
