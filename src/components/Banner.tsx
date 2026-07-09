import type { ReactNode } from "react";

/// Shared top banner: a rust status dot, a message, and an optional action slot.
/// Rendered by both ClaudeBanner and NotificationBanner (DRY — one visual, two sources).
export function Banner({ message, action }: { message: string; action?: ReactNode }) {
  return (
    <div className="flex items-center gap-3 border-b border-hn-border bg-hn-soft px-6 py-2.5">
      <span className="h-2 w-2 shrink-0 rounded-full bg-rust" />
      <p className="min-w-0 flex-1 text-[12.5px] leading-snug text-soft">{message}</p>
      {action}
    </div>
  );
}
