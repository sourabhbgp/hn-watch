import type { ReactNode } from "react";
import { parseTerms } from "./search";

/** Escape regex metacharacters so a term like "c++" is matched literally. */
function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Return `text` with every occurrence of any search term wrapped in a
 * highlighted <mark> (subtle on-brand orange). Case-insensitive; terms are
 * matched via a single alternation regex (non-overlapping, left-to-right).
 * An empty query returns the text unchanged (no <mark>, no overhead).
 */
export function highlight(text: string, query: string): ReactNode {
  const terms = parseTerms(query);
  if (terms.length === 0) return text;
  const re = new RegExp(`(${terms.map(escapeRegExp).join("|")})`, "gi");
  // With one capturing group, split() interleaves matches at odd indices.
  return text.split(re).map((part, i) =>
    i % 2 === 1 ? (
      <mark key={i} className="rounded-[2px] bg-hn-soft text-ink">
        {part}
      </mark>
    ) : (
      part
    ),
  );
}
