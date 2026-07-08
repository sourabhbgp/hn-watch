# HN Watch Brand Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the default Tauri placeholder icon with one custom bold-"W" mark on HN orange, applied consistently to the app bundle and the in-app sidebar logo, and add a `docs/design.md` design-system reference.

**Architecture:** A single source SVG (`assets/brand/icon.svg`) is the master mark. It is rasterized to a 1024px PNG and fed to `tauri icon`, which regenerates the entire `src-tauri/icons/` set. The same geometry (orange squircle + white stroked "W") is mirrored as an inline SVG in the sidebar so the app-bundle icon and in-app logo are visually identical.

**Tech Stack:** Tauri 2 (CLI 2.11.4), React + TypeScript, Tailwind (design tokens in `src/index.css`), `qlmanage` for SVG→PNG rasterization (no ImageMagick/rsvg on this machine — verified).

## Global Constraints

- **Brand color:** squircle fill is exactly `#ff6600` (the `--color-hn` token). Glyph is `#ffffff`.
- **No YC "Y":** never reintroduce the Y Combinator / Hacker News "Y" mark anywhere.
- **No new color tokens.** Reuse the existing `--color-*` palette in `src/index.css` as-is.
- **The "W" is a stroked vector path**, not a `<text>` element — font-independent, identical on every renderer.
- **Rasterizer:** `qlmanage -t -s 1024 -o <dir> <svg>` (verified to emit an exact 1024×1024 RGBA PNG on this machine). It names output `<svg-basename>.png`.
- **Leave pre-existing uncommitted changes alone:** do not stage `STATUS.md` or `docs/TESTING.md` in any task's commit.
- **Real-app verification** happens against the native window per `docs/TESTING.md`, never localhost.

---

## File Structure

- **Create** `assets/brand/icon.svg` — the master mark (single source of truth).
- **Regenerate** `src-tauri/icons/*` — full icon set produced by `tauri icon`.
- **Modify** `src/components/Sidebar.tsx:68-79` — swap the "Y" `<div>` for the inline "W" mark.
- **Create** `docs/design.md` — design-system reference (brand, tokens, typography, components).

---

### Task 1: Author the master mark SVG

**Files:**
- Create: `assets/brand/icon.svg`

**Interfaces:**
- Produces: `assets/brand/icon.svg` — a 1024×1024 SVG, orange squircle (`#ff6600`, `rx=184`) + white stroked "W" path. Consumed by Task 2 (rasterized) and mirrored by Task 3.

- [ ] **Step 1: Create the SVG file**

Create `assets/brand/icon.svg` with exactly:

```xml
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <rect x="112" y="112" width="800" height="800" rx="184" fill="#ff6600"/>
  <path d="M300 356 L376 668 L512 470 L648 668 L724 356"
        fill="none" stroke="#ffffff" stroke-width="88"
        stroke-linecap="round" stroke-linejoin="round"/>
</svg>
```

- [ ] **Step 2: Verify it rasterizes to an exact 1024×1024 RGBA PNG**

Run:
```bash
TMP=$(mktemp -d)
qlmanage -t -s 1024 -o "$TMP" assets/brand/icon.svg >/dev/null 2>&1
sips -g pixelWidth -g pixelHeight -g hasAlpha "$TMP/icon.svg.png"
```
Expected output includes:
```
  pixelWidth: 1024
  pixelHeight: 1024
  hasAlpha: yes
```
If `pixelWidth`/`pixelHeight` are not 1024, or `hasAlpha` is `no`, stop and fix the SVG before continuing.

- [ ] **Step 3: Commit**

```bash
git add assets/brand/icon.svg
git commit -m "feat(brand): add master W-mark source SVG"
```

---

### Task 2: Generate the app bundle icon set

**Files:**
- Create (build artifact): `scratch/icon-master.png` (temporary, not committed)
- Modify (regenerated): `src-tauri/icons/32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, `icon.png`, and all `Square*Logo.png` / `StoreLogo.png`

**Interfaces:**
- Consumes: `assets/brand/icon.svg` from Task 1.
- Produces: a regenerated `src-tauri/icons/` set referenced by `src-tauri/tauri.conf.json:29-34`.

- [ ] **Step 1: Rasterize the master SVG to a 1024px PNG**

Run:
```bash
mkdir -p scratch
qlmanage -t -s 1024 -o scratch assets/brand/icon.svg >/dev/null 2>&1
mv scratch/icon.svg.png scratch/icon-master.png
sips -g pixelWidth -g pixelHeight scratch/icon-master.png
```
Expected: `pixelWidth: 1024` and `pixelHeight: 1024`.

- [ ] **Step 2: Capture the current icon hash (to prove it changes)**

Run:
```bash
shasum src-tauri/icons/128x128.png
```
Note the hash — this is the old blue/yellow placeholder. Step 4 must show a different hash.

- [ ] **Step 3: Regenerate the full icon set**

Run:
```bash
npm run tauri icon scratch/icon-master.png
```
Expected: Tauri prints lines like `Appx logo generated` / writes `icon.icns`, `icon.ico`, and the PNG sizes into `src-tauri/icons/`. No error.

- [ ] **Step 4: Verify the icons actually changed**

Run:
```bash
shasum src-tauri/icons/128x128.png
sips -g pixelWidth -g pixelHeight src-tauri/icons/128x128.png
```
Expected: the hash **differs** from Step 2, and dimensions are `128 x 128`. Optionally open `src-tauri/icons/128x128.png` and confirm it is the orange "W", not the blue/yellow swirl.

- [ ] **Step 5: Remove the scratch master and commit the icon set**

Run:
```bash
rm -rf scratch
git add src-tauri/icons
git commit -m "feat(brand): regenerate app icon set from W mark"
```
Note: `scratch/` is temporary; do not commit it. If the repo has no `.gitignore` entry for it, deleting it (above) is sufficient since it is never staged.

---

### Task 3: Mirror the mark in the in-app sidebar

**Files:**
- Modify: `src/components/Sidebar.tsx:68-79`

**Interfaces:**
- Consumes: the geometry from `assets/brand/icon.svg` (Task 1), rendered inline at 32px.
- Produces: the sidebar brand header showing the same orange "W" mark.

- [ ] **Step 1: Replace the "Y" block with the inline "W" mark**

In `src/components/Sidebar.tsx`, find the current brand block:

```tsx
      {/* brand */}
      <div className="flex items-center gap-2.5 px-4 py-4">
        <div className="grid h-8 w-8 place-items-center rounded-lg bg-hn text-[16px] font-extrabold text-white">
          Y
        </div>
```

Replace the inner logo `<div>` (the one containing `Y`) with an inline SVG that reuses the master geometry, scaled into the same 32px (`h-8 w-8`) rounded box:

```tsx
      {/* brand */}
      <div className="flex items-center gap-2.5 px-4 py-4">
        <div className="h-8 w-8 shrink-0 rounded-lg bg-hn grid place-items-center">
          <svg viewBox="0 0 1024 1024" className="h-5 w-5" aria-hidden="true">
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
```

Notes:
- The squircle is the `bg-hn rounded-lg` box (matches the app icon's orange + rounding); the inline SVG draws only the white "W" path on top, so no doubled background.
- `bg-hn` resolves to `#ff6600` via the `--color-hn` token — same value as the SVG's `<rect>` fill.
- Leave the adjacent "HN Watch" / "watching Hacker News" text block unchanged.

- [ ] **Step 2: Type-check and build the frontend**

Run:
```bash
npm run build
```
Expected: TypeScript + Vite build completes with no errors. (`tsc` is part of the `build` script; a malformed JSX/SVG attribute would fail here.)

- [ ] **Step 3: Commit**

```bash
git add src/components/Sidebar.tsx
git commit -m "feat(brand): use W mark in sidebar, drop the Y homage"
```

---

### Task 4: Write the design-system doc

**Files:**
- Create: `docs/design.md`

**Interfaces:**
- Consumes: the tokens in `src/index.css:4-17`, the mark from Task 1, and the components in `src/components/`.
- Produces: `docs/design.md`, the human-readable companion to `src/index.css`.

- [ ] **Step 1: Create `docs/design.md`**

Create `docs/design.md` with exactly this content:

````markdown
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
````

- [ ] **Step 2: Verify the doc matches the real tokens**

Run:
```bash
grep -E -- "--color-(paper|card|line|ink|soft|faint|hn|hn-soft|hn-border|rust|ok)" src/index.css
```
Expected: every hex value listed in the doc's token table matches the value printed here. Fix any mismatch in `docs/design.md`.

- [ ] **Step 3: Commit**

```bash
git add docs/design.md
git commit -m "docs: add design-system reference (brand, tokens, typography, components)"
```

---

### Task 5: Verify in the real app

**Files:** none (verification only).

**Interfaces:**
- Consumes: the built app bundle (icons from Task 2, sidebar from Task 3).

- [ ] **Step 1: Build the app bundle**

Run:
```bash
npm run tauri build
```
Expected: build succeeds and produces `src-tauri/target/release/bundle/macos/hn-watch.app`.
(If a full release build is too slow, a debug build via `npm run tauri dev` also shows the
new dock icon and sidebar; the release bundle is preferred for the final check.)

- [ ] **Step 2: Launch and visually confirm (per `docs/TESTING.md`)**

Open the native window (not localhost). Confirm all three:

1. **Dock / Spotlight / window icon** is the orange "W" — the blue/yellow Tauri swirl is gone.
2. **Sidebar header** shows the same orange "W" (no "Y").
3. The dock mark and the sidebar mark read as the **same** icon (same orange `#ff6600`, same rounded square, same "W").

- [ ] **Step 3: Confirm small-size legibility**

Search the app in Spotlight (as in the original screenshot) and confirm the "W" is legible at the small result-icon size. If it looks muddy, revisit `stroke-width` in `assets/brand/icon.svg` and re-run Task 2.

- [ ] **Step 4: Final status**

No commit needed (verification only). If any check fails, return to the relevant task, fix, and re-verify.

---

## Self-Review

- **Spec coverage:**
  - Custom "W" mark on HN orange → Task 1. ✓
  - Applied to app bundle icon set → Task 2. ✓
  - Applied to in-app sidebar logo → Task 3. ✓
  - Single source of truth (`assets/brand/icon.svg`) → Task 1 produces it; Tasks 2 & 3 derive from it. ✓
  - `docs/design.md` (brand + tokens + typography + components) → Task 4. ✓
  - No YC "Y" / no new tokens → Global Constraints + Task 3 removes the "Y". ✓
  - Real-app verification per `docs/TESTING.md` → Task 5. ✓
- **Placeholder scan:** no TBD/TODO; every code and command step is concrete and was validated on this machine (SVG rasterization, dimensions, token values). ✓
- **Type/name consistency:** the "W" path `d`, `stroke-width` (88), and `#ff6600`/`#ffffff` are identical across the SVG (Task 1), the icon generation (Task 2), and the inline sidebar SVG (Task 3). ✓
