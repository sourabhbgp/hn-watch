import type { FeedItem } from "../types";

/**
 * Client-side feed search. Case-insensitive; multiple whitespace-separated
 * terms must ALL appear (AND) across the card's title + summary + reason.
 * An empty/whitespace query matches everything (no-op filter).
 */
export function matchesQuery(item: FeedItem, query: string): boolean {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return true;
  const haystack = `${item.title} ${item.summary} ${item.reason}`.toLowerCase();
  return terms.every((t) => haystack.includes(t));
}
