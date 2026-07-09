import type { ClaudeHealth } from "../types";

export function ClaudeBanner({
  health,
  onRecheck,
  rechecking,
}: {
  health: ClaudeHealth;
  onRecheck: () => void;
  rechecking: boolean;
}) {
  if (health.status === "ok") return null;
  return (
    <div className="flex items-center gap-3 border-b border-hn-border bg-hn-soft px-6 py-2.5">
      <span className="h-2 w-2 shrink-0 rounded-full bg-rust" />
      <p className="min-w-0 flex-1 text-[12.5px] leading-snug text-soft">{health.message}</p>
      <button
        onClick={onRecheck}
        disabled={rechecking}
        className="shrink-0 rounded-md border border-hn-border bg-card px-3 py-1 text-[12px] font-semibold text-rust transition-colors hover:bg-paper disabled:opacity-50"
      >
        {rechecking ? "Checking…" : "Re-check"}
      </button>
    </div>
  );
}
