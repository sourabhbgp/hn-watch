/** Compact relative time: "3m", "2h", "5d" for the gap between `then` and `now` (epoch secs). */
export function timeAgo(then: number, now: number): string {
  const d = Math.max(0, now - then);
  if (d < 3600) return `${Math.max(1, Math.floor(d / 60))}m`;
  if (d < 86_400) return `${Math.floor(d / 3600)}h`;
  return `${Math.floor(d / 86_400)}d`;
}
