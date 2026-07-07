import type { FeedItem } from "../types";

export function FeedCard({
  item,
  onDigDeeper,
}: {
  item: FeedItem;
  onDigDeeper: (item: FeedItem) => void;
}) {
  return (
    <article className="rounded-xl border border-line bg-card p-4 transition-shadow hover:shadow-[0_1px_0_rgba(0,0,0,0.04)]">
      {/* meta row */}
      <div className="flex items-center gap-2 text-[11px]">
        <span className="rounded-full bg-hn-soft px-2 py-0.5 font-medium text-rust">
          {item.monitorName}
        </span>
        <span className="font-mono text-faint">{item.domain}</span>
        <span className="ml-auto font-mono text-faint">{item.timeAgo} ago</span>
      </div>

      {/* title */}
      <a
        href={item.url}
        target="_blank"
        rel="noreferrer"
        className="mt-2 block text-[15px] font-semibold leading-snug text-ink hover:text-hn"
      >
        {item.title}
      </a>

      {/* summary */}
      <p className="mt-1.5 text-[13.5px] leading-relaxed text-soft">
        {item.summary}
      </p>

      {/* why it matched */}
      <div className="mt-2.5 flex gap-2 rounded-lg bg-paper px-3 py-2 text-[12px] text-soft">
        <span className="font-mono text-[10.5px] uppercase tracking-wide text-faint">
          matched
        </span>
        <span className="leading-snug">{item.reason}</span>
      </div>

      {/* footer */}
      <div className="mt-3 flex items-center gap-4 text-[12px] text-faint">
        <span className="font-mono">▲ {item.hnScore}</span>
        <span className="font-mono">💬 {item.hnComments}</span>
        <button
          onClick={() => onDigDeeper(item)}
          className="ml-auto rounded-lg border border-hn-border bg-hn-soft px-3 py-1.5 text-[12px] font-semibold text-rust transition-colors hover:bg-hn hover:text-white"
        >
          🔬 Dig deeper
        </button>
      </div>
    </article>
  );
}
