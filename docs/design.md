# HN Watch — Design System

The human-readable companion to `src/index.css` (which holds the authoritative
design tokens) and `docs/architecture.html`. When code and this doc disagree,
`src/index.css` wins for values; update this doc to match.

## Brand

**The mark:** a bold white **"W"** (for *Watch*) on an HN-orange rounded square.

- **Meaning:** the app *watches* Hacker News. The orange ties it to HN without
  borrowing Y Combinator's trademarked "Y".
- **Source of truth:** `assets/brand/icon.svg` (1024×1024). Everything is derived
  from it — never hand-edit generated PNGs.
- **Construction:** squircle `rect` fill `#ff6600`, `rx=184` on a 1024 canvas;
  the "W" is a single white stroked path (`stroke-width=88`, round caps/joins),
  not a font glyph, so it renders identically everywhere.

**Regenerating the app icon** after editing `assets/brand/icon.svg`:

```bash
qlmanage -t -s 1024 -o scratch assets/brand/icon.svg && mv scratch/icon.svg.png scratch/icon-master.png
npm run tauri icon scratch/icon-master.png
rm -rf scratch
```

**Do / Don't**

- Do keep the squircle fill exactly `#ff6600` (`--color-hn`) and the glyph white.
- Do keep the "W" a stroked path so it stays crisp from 16px to 1024px.
- Don't reintroduce the YC / HN "Y" anywhere.
- Don't recolor the squircle or add gradients/detail that dies at 16px.

## Color tokens

Defined in `src/index.css` (`@theme`). Use the Tailwind class (e.g. `bg-hn`,
`text-faint`) rather than raw hex.

| Token | Hex | Role |
|---|---|---|
| `--color-paper` | `#faf8f4` | App background |
| `--color-card` | `#ffffff` | Card / panel surfaces |
| `--color-line` | `#e7e1d6` | Borders, dividers, scrollbar thumb |
| `--color-ink` | `#14110d` | Primary text |
| `--color-soft` | `#4b463f` | Secondary text |
| `--color-faint` | `#8a8378` | Muted / metadata text |
| `--color-hn` | `#ff6600` | Brand orange — logo squircle, primary buttons, accents |
| `--color-hn-soft` | `#fff1e6` | Tinted backgrounds for HN-tagged chips |
| `--color-hn-border` | `#ffd2ac` | Borders on HN-tinted elements |
| `--color-rust` | `#b7410e` | Secondary accent (e.g. "Rust async" monitor) |
| `--color-ok` | `#3f7d3f` | Success / live-status green |

## Typography

Both stacks are defined in `src/index.css`.

- **Sans** (`--font-sans`): system UI stack (`-apple-system`, `BlinkMacSystemFont`,
  `Segoe UI`, `Inter`, `system-ui`). Default for all UI text.
- **Mono** (`--font-mono`): `SF Mono` / `ui-monospace` / `Menlo`. Used for
  metadata and counts (e.g. the "watching Hacker News" line, match badges).

## Components

Documented as built in `src/components/`:

- **Sidebar** (`Sidebar.tsx`): brand header (the "W" mark + title), the "All matches"
  row, and the monitor list. Each monitor row shows a status dot, name, description,
  cadence, and a match count.
- **Status dots:** small `rounded-full` indicators — green (`--color-ok`) for live,
  and muted/rust states for paused or attention. The "live" feed indicator reuses green.
- **Feed card** (`FeedCard.tsx`): monitor tag chip (HN-tinted), source domain,
  relative time, title, summary, a "MATCHED" reason block, score/comment counts, and
  the **🔬 Dig deeper** action.
- **Primary button:** `bg-hn` fill, white text (e.g. "+ New monitor", "Dig deeper").
