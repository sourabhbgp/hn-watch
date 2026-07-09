import type { ClaudeHealth } from "../types";
import { Banner } from "./Banner";

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
    <Banner
      message={health.message}
      action={
        <button
          onClick={onRecheck}
          disabled={rechecking}
          className="shrink-0 rounded-md border border-hn-border bg-card px-3 py-1 text-[12px] font-semibold text-rust transition-colors hover:bg-paper disabled:opacity-50"
        >
          {rechecking ? "Checking…" : "Re-check"}
        </button>
      }
    />
  );
}
