import { useState } from "react";
import type { Monitor, MonitorStatus } from "../types";

const STATUS_DOT: Record<MonitorStatus, string> = {
  active: "bg-ok",
  paused: "bg-faint",
  error: "bg-rust",
};

const INTERVAL_OPTIONS: { label: string; secs: number }[] = [
  { label: "every 15m", secs: 900 },
  { label: "every 30m", secs: 1800 },
  { label: "every 1h", secs: 3600 },
  { label: "every 6h", secs: 21600 },
];

function fmtClock(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function fmtCountdown(nextCheckAt: number | null, now: number): string {
  if (nextCheckAt == null) return "scheduling…";
  const rem = nextCheckAt - now;
  // Past its wall-clock due time — the check runs on a monotonic timer that
  // pauses across laptop sleep, so it can be a little behind here. Show a calm
  // "checking soon…" rather than a stuck "due now".
  if (rem <= 0) return "checking soon…";
  if (rem < 60) return "next in <1m";
  return `next in ${Math.round(rem / 60)}m`;
}

function MonitorRow({
  monitor,
  selected,
  now,
  checking,
  onSelect,
  onDelete,
}: {
  monitor: Monitor;
  selected: boolean;
  now: number;
  checking: boolean;
  onSelect: () => void;
  onDelete: () => void;
}) {
  // Quiet "heartbeat" pill on the name row: grey countdown by default, alive
  // (hn-soft + pulse) while a tick runs, rust when the last tick errored.
  const chip = checking ? (
    <span className="flex shrink-0 items-center gap-1 rounded-full bg-hn-soft px-2 py-0.5 font-mono text-[10px] text-rust">
      <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-rust" />
      Checking…
    </span>
  ) : monitor.status === "paused" ? (
    <span className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[10px] text-faint">
      Paused
    </span>
  ) : monitor.status === "error" ? (
    <span
      title={monitor.lastError ?? ""}
      className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[10px] text-rust"
    >
      error
    </span>
  ) : (
    <span className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[10px] text-faint">
      {fmtCountdown(monitor.nextCheckAt, now)}
    </span>
  );

  // One calm meta line: total matches, then either the failure note or the
  // last-checked time. "· N new" only appears (in brand orange) when a tick
  // actually brought new matches — the scanned count lives in the hover title.
  const matches = `${monitor.matchCount} ${monitor.matchCount === 1 ? "match" : "matches"}`;
  const newCount = monitor.lastNewCount ?? 0;
  const checkedAt = monitor.lastCheckedAt;

  return (
    <div
      className={`group w-full rounded-lg border px-3 py-2.5 transition-colors ${
        selected ? "border-hn-border bg-hn-soft" : "border-line bg-card/70 hover:bg-card"
      }`}
    >
      {/* name row — status dot · name · countdown/Checking…/error · delete on hover */}
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 shrink-0 rounded-full ${STATUS_DOT[monitor.status]}`} />
        <button
          onClick={onSelect}
          className="min-w-0 flex-1 truncate text-left text-[13.5px] font-semibold text-ink"
        >
          {monitor.name}
        </button>
        {chip}
        <button
          onClick={onDelete}
          title="Delete monitor"
          className="shrink-0 text-faint opacity-0 transition-opacity hover:text-rust group-hover:opacity-100"
        >
          ×
        </button>
      </div>

      <button onClick={onSelect} className="block w-full text-left">
        <p className="mt-1 line-clamp-2 text-[11.5px] leading-snug text-faint">{monitor.prompt}</p>

        {checkedAt != null && (
          <p
            title={
              monitor.lastError ?? `scanned ${monitor.lastCheckedCount ?? 0} stories this check`
            }
            className="mt-1.5 truncate font-mono text-[10.5px] text-faint"
          >
            {matches}
            {monitor.lastError ? (
              <span className="text-rust"> · last check failed</span>
            ) : (
              <>
                {newCount > 0 && <span className="font-medium text-hn"> · {newCount} new</span>}
                {` · checked ${fmtClock(checkedAt)}`}
              </>
            )}
          </p>
        )}
      </button>
    </div>
  );
}

export function Sidebar({
  monitors,
  selectedId,
  onSelect,
  onCreate,
  onDelete,
  now,
  checkingIds,
}: {
  monitors: Monitor[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onCreate: (name: string, prompt: string, intervalSecs: number) => void;
  onDelete: (id: string) => void;
  now: number;
  checkingIds: Set<string>;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [secs, setSecs] = useState(1800);

  const submit = () => {
    if (!name.trim() || !prompt.trim()) return;
    onCreate(name.trim(), prompt.trim(), secs);
    setName("");
    setPrompt("");
    setSecs(1800);
    setOpen(false);
  };

  return (
    <aside className="flex h-full w-72 shrink-0 flex-col border-r border-line bg-card/40">
      <div className="flex items-center gap-2.5 px-4 py-4">
        <div className="h-8 w-8 shrink-0 rounded-lg bg-hn grid place-items-center">
          <svg viewBox="216 216 592 592" className="h-6 w-6" aria-hidden="true">
            <path
              d="M300 356 L376 668 L512 470 L648 668 L724 356"
              fill="none"
              stroke="#ffffff"
              strokeWidth={88}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>
        <div>
          <div className="text-[15px] font-bold leading-none tracking-tight">HN Watch</div>
          <div className="mt-1 font-mono text-[10px] text-faint">watching Hacker News</div>
        </div>
      </div>

      <div className="flex items-center justify-between px-4 pb-2 pt-1">
        <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-faint">Monitors</span>
        <span className="font-mono text-[10px] text-faint">{monitors.length}</span>
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
            now={now}
            checking={checkingIds.has(m.id)}
            onSelect={() => onSelect(m.id)}
            onDelete={() => onDelete(m.id)}
          />
        ))}
      </div>

      <div className="border-t border-line p-3">
        {open ? (
          <div className="space-y-2">
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Monitor name"
              maxLength={100}
              className="w-full rounded-md border border-line bg-paper px-2 py-1.5 text-[12.5px] text-ink placeholder:text-faint"
            />
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="What do you care about? (natural language)"
              rows={3}
              maxLength={1000}
              className="w-full resize-none rounded-md border border-line bg-paper px-2 py-1.5 text-[12.5px] text-ink placeholder:text-faint"
            />
            <select
              value={secs}
              onChange={(e) => setSecs(Number(e.target.value))}
              className="w-full rounded-md border border-line bg-paper px-2 py-1.5 text-[12.5px] text-ink"
            >
              {INTERVAL_OPTIONS.map((o) => (
                <option key={o.secs} value={o.secs}>
                  {o.label}
                </option>
              ))}
            </select>
            <div className="flex gap-2">
              <button
                onClick={submit}
                className="flex-1 rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white transition-opacity hover:opacity-90"
              >
                Create
              </button>
              <button
                onClick={() => setOpen(false)}
                className="rounded-lg border border-line px-3 py-2 text-[13px] font-semibold text-soft hover:bg-card"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setOpen(true)}
            className="w-full rounded-lg bg-hn px-3 py-2 text-[13px] font-semibold text-white transition-opacity hover:opacity-90"
          >
            + New monitor
          </button>
        )}
      </div>
    </aside>
  );
}
