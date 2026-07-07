import type { Monitor, MonitorStatus } from "../types";

const STATUS_DOT: Record<MonitorStatus, string> = {
  active: "bg-ok",
  paused: "bg-faint",
  error: "bg-rust",
};

const STATUS_LABEL: Record<MonitorStatus, string> = {
  active: "active",
  paused: "paused",
  error: "error",
};

function MonitorRow({
  monitor,
  selected,
  onSelect,
}: {
  monitor: Monitor;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      onClick={onSelect}
      className={`w-full text-left rounded-lg px-3 py-2.5 transition-colors border ${
        selected
          ? "bg-hn-soft border-hn-border"
          : "bg-transparent border-transparent hover:bg-card"
      }`}
    >
      <div className="flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${STATUS_DOT[monitor.status]}`}
          title={STATUS_LABEL[monitor.status]}
        />
        <span className="truncate text-[13.5px] font-semibold text-ink">
          {monitor.name}
        </span>
        <span className="ml-auto shrink-0 rounded-full bg-paper px-1.5 py-0.5 font-mono text-[10px] text-faint">
          {monitor.matchCount}
        </span>
      </div>
      <p className="mt-1 line-clamp-2 pl-4 text-[11.5px] leading-snug text-faint">
        {monitor.prompt}
      </p>
      <p className="mt-1 pl-4 font-mono text-[10.5px] text-faint">
        {monitor.intervalLabel}
      </p>
    </button>
  );
}

export function Sidebar({
  monitors,
  selectedId,
  onSelect,
  onNew,
}: {
  monitors: Monitor[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onNew: () => void;
}) {
  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-line bg-card/40">
      {/* brand */}
      <div className="flex items-center gap-2.5 px-4 py-4">
        <div className="grid h-8 w-8 place-items-center rounded-lg bg-hn text-[16px] font-extrabold text-white">
          Y
        </div>
        <div>
          <div className="text-[15px] font-bold leading-none tracking-tight">
            HN Watch
          </div>
          <div className="mt-1 font-mono text-[10px] text-faint">
            watching Hacker News
          </div>
        </div>
      </div>

      {/* monitors list */}
      <div className="flex items-center justify-between px-4 pb-2 pt-1">
        <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
          Monitors
        </span>
        <span className="font-mono text-[10px] text-faint">
          {monitors.length}
        </span>
      </div>
      <div className="flex-1 space-y-1 overflow-y-auto px-2">
        <button
          onClick={() => onSelect(null)}
          className={`w-full rounded-lg px-3 py-2 text-left text-[13px] font-semibold transition-colors border ${
            selectedId === null
              ? "bg-hn-soft border-hn-border text-ink"
              : "border-transparent text-soft hover:bg-card"
          }`}
        >
          All matches
        </button>
        {monitors.map((m) => (
          <MonitorRow
            key={m.id}
            monitor={m}
            selected={selectedId === m.id}
            onSelect={() => onSelect(m.id)}
          />
        ))}
      </div>

      {/* new monitor */}
      <div className="border-t border-line p-3">
        <button
          onClick={onNew}
          className="w-full rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white transition-opacity hover:opacity-90"
        >
          + New monitor
        </button>
      </div>
    </aside>
  );
}
