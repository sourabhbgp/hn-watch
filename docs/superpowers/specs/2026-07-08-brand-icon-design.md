# HN Watch — Unified brand icon + design system doc

**Date:** 2026-07-08
**Branch:** `feat/brand-icon`
**Status:** Approved design, ready for implementation plan

## Problem

The app presents two unrelated identities:

- **App bundle icon** (dock, Spotlight, window) is the **default Tauri placeholder** —
  a blue/yellow swirl in `src-tauri/icons/`, never replaced since the scaffold commit.
- **In-app logo** (sidebar header, `src/components/Sidebar.tsx`) is a white **"Y"** on an
  orange squircle — a hand-built homage to the Y Combinator / Hacker News mark.

Result: the product looks like two different apps, and the in-app logo borrows YC's
trademarked "Y". We want **one custom mark used consistently everywhere**, plus a
written design-system reference so the brand and tokens don't drift again.

## Goals

1. A single custom brand mark: a bold white **"W"** (for *Watch*) on the HN-orange squircle.
2. That mark applied to **every surface** — app bundle icon set *and* the in-app sidebar logo —
   from one source asset.
3. A human-readable **`docs/design.md`** design-system doc (brand + tokens + typography +
   components) that pairs with the existing `src/index.css` tokens.

## Non-goals

- No redesign of the UI layout, feed, or interaction patterns.
- No new color tokens — reuse the existing `--color-*` palette as-is.
- No reintroduction of the YC/HN "Y" mark anywhere.

## Design

### The mark

- **Glyph:** geometric, bold **"W"**, white (`--color-card` / `#ffffff`).
- **Background:** the existing HN-orange token **`--color-hn` (`#ff6600`)** — so the icon and
  the in-app logo are literally the same color value.
- **Shape:** rounded square. In-app it matches the current `rounded-lg` treatment; at bundle
  sizes it uses a macOS-appropriate squircle/rounded-rect so it sits correctly in the dock.
- **Format:** authored as **SVG** so it stays razor-sharp from 16px (menu bar) to 1024px.
- **Legibility:** the "W" must remain readable at 16px — bold strokes, generous padding,
  no fine detail.

### Single source of truth

One authored vector: **`assets/brand/icon.svg`** (orange squircle + white "W").
Everything else is derived from it:

| Surface | Mechanism | Output |
|---|---|---|
| App bundle (dock, Spotlight, window, installers) | render `icon.svg` → 1024px PNG → `npm run tauri icon <png>` | regenerates the full `src-tauri/icons/` set: `icon.icns`, `icon.ico`, all `*.png`, `Square*Logo.png`, `StoreLogo.png` |
| In-app sidebar header | React component renders the same "W" mark | `src/components/Sidebar.tsx` (replaces the `Y`) |

The in-app mark stays a lightweight inline render (styled element or inline SVG) rather than an
`<img>`, matching the current approach — but its geometry and color match `icon.svg` so the two
read as identical.

### `docs/design.md` — design-system reference

A single Markdown doc, the human-readable companion to `src/index.css`. Sections:

1. **Brand** — the "W" mark: meaning (*Watch* Hacker News), construction (orange squircle +
   white W), the single-source workflow, and do/don't rules (notably: **do not** reintroduce
   the YC "Y"; **do not** recolor the squircle off `--color-hn`).
2. **Color tokens** — the full `--color-*` palette from `index.css` with each role:
   `paper`, `card`, `line`, `ink`, `soft`, `faint`, `hn`, `hn-soft`, `hn-border`, `rust`, `ok`.
   Each row: token, hex, where it's used.
3. **Typography** — the `--font-sans` and `--font-mono` stacks and where each applies.
4. **Components** — the existing UI patterns: sidebar, monitor list + status dots, feed card,
   "MATCHED" reason block, "Dig deeper" (🔬) action, the "live" indicator. Documented as-is,
   not redesigned.

`docs/design.md` is authoritative for humans; `src/index.css` remains authoritative for code.
The doc notes this relationship so they stay in sync (as `index.css` already gestures at).

## Files touched

- **New:** `assets/brand/icon.svg` — the source mark.
- **New:** `docs/design.md` — the design-system doc.
- **New/regenerated:** `src-tauri/icons/*` — full icon set from `tauri icon`.
- **Edited:** `src/components/Sidebar.tsx` — swap the "Y" for the "W" mark.
- **Possibly edited:** `src/index.css` — only if a small brand comment/token reference helps;
  no palette changes.

## Verification

Per the project's real-app testing workflow (native window, not localhost —
see `docs/TESTING.md`):

1. Build the app (`npm run tauri build` or the debug bundle).
2. Launch the native window and confirm:
   - **Dock / Spotlight / window** icon is the orange "W" (no blue/yellow swirl).
   - **Sidebar header** shows the same orange "W" (no "Y").
   - The two marks read as identical (same orange, same shape, same glyph).
3. Confirm the "W" is legible at small sizes (menu bar / Spotlight result).

## Risks / notes

- `tauri icon` overwrites the entire `src-tauri/icons/` directory — expected; the old
  placeholder set is intentionally replaced.
- Keep the pre-existing uncommitted changes (`STATUS.md`, `docs/TESTING.md`) out of this
  feature's commits unless separately relevant.
