# Text Layers (Procreate-style editable text) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Text tool create an editable text layer (Procreate-style): place → re-edit → rasterize on demand, instead of baking bitmap letters onto the current layer immediately.

**Architecture:** A text layer is a normal `Layer` plus `text: Option<TextBlock>`; its pixel buffer is always re-rendered from the block by a single writer (`rerender_text_layer`), so compositing/merge/flatten/export/thumbnails keep working on pixels. Undo (per-active-layer pixel snapshots) gains the block. Project format gets a v11 per-layer text record; caps + sanitization on load.

**Tech Stack:** Rust workspace; `crates/engine` (`procraft-engine`, fontdue) + `apps/procraft` (egui 0.36.2). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-18-text-layers-design.md` (read it — this plan argues from it)

## Global Constraints

- No new crate dependencies; engine stays UI-free.
- Gate battery after every task: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`-equivalent (repo default clippy); `cargo test --workspace`; `cargo check -p procraft` for `aarch64-apple-ios`, `aarch64-linux-android`, `x86_64-linux-android`. All exit 0. Cargo.lock unchanged.
- Version magic: `PCRAFT11`; older builds refuse it at the magic (existing pattern). v10 and older files MUST still open (text-less).
- Caps (verbatim): `MAX_FONT_BYTES = 2 * 1024 * 1024`; text bytes cap 16 KiB; `MAX_TEXT_CHARS` 1024 / `MAX_TEXT_LINES` 64 / `MAX_LAYOUT_SIDE` 2048 / `MAX_OUTLINE_PX` 32 (existing consts re-used).
- Single-writer invariant: only `rerender_text_layer` writes a text layer's pixels; the app gates every other active-layer pixel writer.
- Creation (Render-new) is NOT undoable (matches Add layer; do NOT checkpoint there). Edits and rasterize ARE undoable.
- Commits: one per task, message style matches repo history. Non-ASCII in messages → `git commit -F <file>` (PowerShell mojibake gotcha).

## Review Focus

Inputs/conditions the spec implies but no test's happy path exercises; each gets the listed check in its owning task:
1. Undo pressed right after Render-new (nothing should restore — and nothing must corrupt). → engine test, Task 1.
2. A rerender/edit that fails validation (blank text) after an anchor move — old pixels must NOT be wiped. → engine test, Task 2.
3. A `.procraft` v11 text record with an oversize font blob or non-finite floats (hostile file) → engine tests, Task 5.
4. Tapping existing text while `paint_target == Mask` (must hint, not silently no-op) → device checklist, Task 7 (app wiring has no harness — see plan note).
5. Dialog open while the edited layer is deleted/reordered/merged (stale index) → Done identity re-check; device checklist, Task 7.

**Plan note (app tasks):** this repo's app crate has no egui interaction harness (its 42 tests cover pure helpers, e.g. `shape_preview_points`). Engine tasks are strict TDD; app tasks (7–8) are wiring verified by the full gates + scoped review + device pass, matching the wave-2/shapes precedent. No new app unit test is possible for button/menu behavior — do not fake one.

---

### Task 1: Engine — TextBlock, Layer.text, add_text_layer

**Files:**
- Modify: `crates/engine/src/text.rs` (TextBlock struct + MAX_TEXT_BYTES + add_text_layer + stamp refactor)
- Modify: `crates/engine/src/document.rs` (Layer.text field, is_text, construction literals at :3088)
- Modify: `crates/engine/src/animation.rs:333`, `crates/engine/src/psd.rs:462+492` (`text: None` at the other Layer literals)
- Test: `crates/engine/src/text.rs` (tests mod), `crates/engine/src/document.rs` (tests mod if Layer literal tests need it)

**Interfaces:**
- Produces: `pub struct TextBlock { pub text: String, pub size_px: f32, pub color: [u8;4], pub style: TextStyle, pub x: f32, pub y: f32, pub w: u32, pub h: u32 }` (Clone, Debug, PartialEq) in `text.rs`; `pub const MAX_TEXT_BYTES: usize = 16 * 1024`; `pub fn Layer::is_text(&self) -> bool`; `pub fn Document::add_text_layer(&mut self, name: String, block: TextBlock) -> Result<(usize, u32, u32), String>`; `fn stamp_layout_at(&mut self, index: usize, respect_selection: bool, layout: &TextLayout, x: f32, y: f32, color: Color) -> Result<(), String>` (private).

- [ ] **Step 1: Write failing tests** (text.rs tests mod — mirror existing text-test fixture patterns)

```rust
#[test]
fn add_text_layer_creates_renders_and_stores_block() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "Hi".into(), size_px: 48.0, color: [255, 0, 0, 255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let before = doc.layers.len();
    let (idx, w, h) = doc.add_text_layer("Text 1".into(), block).unwrap();
    assert_eq!(doc.layers.len(), before + 1);
    assert_eq!(idx, before);
    assert!(doc.layers[idx].is_text());
    assert!(w > 0 && h > 0);
    assert_eq!(doc.layers[idx].text.as_ref().unwrap().w, w);
    // some red coverage exists near the anchor
    let px = |x: u32, y: u32| doc.layers[idx].pixels[( (y * 200 + x) * 4) as usize + 3];
    assert!((0..64).any(|x| (0..64).any(|y| px(x, y) > 0)));
}

#[test]
fn add_text_layer_invalid_block_adds_no_layer() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "   ".into(), size_px: 48.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 0.0, y: 0.0, w: 0, h: 0 };
    let before = doc.layers.len();
    assert!(doc.add_text_layer("Text 1".into(), block).is_err());
    assert_eq!(doc.layers.len(), before);
}

#[test]
fn undo_after_add_text_layer_does_not_corrupt() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "X".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 5.0, y: 5.0, w: 0, h: 0 };
    let (idx, ..) = doc.add_text_layer("Text 1".into(), block).unwrap();
    let pixels = doc.layers[idx].pixels.clone();
    assert!(!doc.undo()); // creation is not undoable; must be a clean no-op
    assert_eq!(doc.layers[idx].pixels, pixels);
    assert!(doc.layers[idx].is_text());
}
```

- [ ] **Step 2: Run to verify red** — `cargo test -p procraft-engine text` → compile error E0412/E0599 (`TextBlock`/`add_text_layer` not found).

- [ ] **Step 3: Implement.**
  - `text.rs`: `TextBlock` (derive `Clone, Debug, PartialEq`); `pub const MAX_TEXT_BYTES`; refactor `stamp_layout` → `stamp_layout_at(index, respect_selection, ...)`: the layer lookup uses `index` (bounds-check as today), selection checks wrapped in `if respect_selection`; `draw_text_styled` delegates `stamp_layout_at(self.active, true, ...)`; outline pass uses the same index. Add `add_text_layer`: build layout + optional ring FIRST (validate before any mutation — blank/oversize errors return before push), then push `Layer::blank(name, w, h)` with `text = Some(block-with-fresh-w/h)`, stamp fill at `(block.x, block.y)`, set `self.active = idx`, `mark_full_dirty()`, return `(idx, w, h)`. Stamp with `respect_selection = false`. Layer pixels: `Layer::blank` gives the buffer; stamp writes it (respect layer ox/oy = 0 for a fresh layer).
  - `document.rs`: `Layer.text: Option<TextBlock>` field + doc comment; `Layer::blank` sets `None`; `is_text()`; update the literal at :3088; `animation.rs:333`, `psd.rs:462+492` get `text: None`.

- [ ] **Step 4: Run to verify green** — `cargo test -p procraft-engine` (all existing + 3 new pass).

- [ ] **Step 5: Gates + commit** — full battery (see Global Constraints). Commit: `feat(engine): text layers — TextBlock, Layer.text, add_text_layer`.

---

### Task 2: Engine — rerender_text_layer + undo carries the block

**Files:**
- Modify: `crates/engine/src/document.rs` (UndoEntry :363, snapshot :723, undo :734, redo :754), `crates/engine/src/text.rs` (rerender_text_layer)
- Test: `crates/engine/src/text.rs`

**Interfaces:**
- Consumes: Task 1's `TextBlock`, `stamp_layout_at`.
- Produces: `pub fn Document::rerender_text_layer(&mut self, index: usize) -> Result<(u32, u32), String>`; `UndoEntry.text: Option<TextBlock>` (private field).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn rerender_moves_text_and_clears_old_pixels() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "Hi".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let (idx, ..) = doc.add_text_layer("T".into(), block).unwrap();
    let alpha_sum = |d: &Document| d.layers[idx].pixels.chunks(4).map(|p| p[3] as u64).sum::<u64>();
    let first = alpha_sum(&doc);
    doc.layers[idx].text.as_mut().unwrap().x = 100.0;
    let (w, h) = doc.rerender_text_layer(idx).unwrap();
    assert!(w > 0 && h > 0);
    // same text+size+font -> deterministic layout -> identical total coverage
    assert_eq!(alpha_sum(&doc), first);
    let px = |x: u32, y: u32| doc.layers[idx].pixels[((y * 200 + x) * 4) as usize + 3];
    assert!((10..40).all(|x| (10..40).all(|y| px(x, y) == 0)));       // old area cleared
    assert!((100..160).any(|x| (10..60).any(|y| px(x, y) > 0)));      // new area painted
}

#[test]
fn rerender_failure_leaves_layer_untouched() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "Hi".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let (idx, ..) = doc.add_text_layer("T".into(), block).unwrap();
    let pixels = doc.layers[idx].pixels.clone();
    doc.layers[idx].text.as_mut().unwrap().text = "  ".into(); // now invalid
    assert!(doc.rerender_text_layer(idx).is_err());
    assert_eq!(doc.layers[idx].pixels, pixels); // validate BEFORE clearing
}

#[test]
fn undo_restores_block_and_pixels_after_edit() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "Hi".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let (idx, ..) = doc.add_text_layer("T".into(), block).unwrap();
    let p0 = doc.layers[idx].pixels.clone();
    let b0 = doc.layers[idx].text.clone().unwrap();
    doc.checkpoint();
    doc.layers[idx].text.as_mut().unwrap().x = 100.0;
    doc.rerender_text_layer(idx).unwrap();
    assert!(doc.undo());
    assert_eq!(doc.layers[idx].pixels, p0);
    assert_eq!(doc.layers[idx].text.as_ref().unwrap().x, b0.x);
    assert!(doc.redo());
    assert_eq!(doc.layers[idx].text.as_ref().unwrap().x, 100.0);
}
```

- [ ] **Step 2: Verify red** — E0599 (`rerender_text_layer`) and the undo test failing on block restore (pixels-only entry).

- [ ] **Step 3: Implement.**
  - `UndoEntry` gains `text: Option<TextBlock>`; `byte_size()` adds `text` bytes: `text.text.len() + style.font_bytes.as_ref().map_or(0, Vec::len) + 64`. `snapshot()` clones `layer.text`; `undo()`/`redo()` restore it alongside pixels/mask/ox/oy.
  - `rerender_text_layer(index)`: `layout_text_styled` (+ ring if outline) FIRST → on error return Err with layer untouched; then `pixels.fill(0)`; `mark_full_dirty()`; stamp ring then fill via `stamp_layout_at(index, false, ...)`; update `block.w/h`; return `(w, h)`. Bounds: `index >= layers.len()` → Err("no such layer").

- [ ] **Step 4: Green** — `cargo test -p procraft-engine`.

- [ ] **Step 5: Gates + commit** — `feat(engine): re-editable text — rerender_text_layer, undo carries the block`.

---

### Task 3: Engine — rasterize_text_layer + merge_down strips text

**Files:**
- Modify: `crates/engine/src/document.rs` (rasterize_text_layer near merge_down :1392; merge_down text handling)
- Test: `crates/engine/src/document.rs` tests mod

**Interfaces:**
- Produces: `pub fn Document::rasterize_text_layer(&mut self, index: usize) -> bool`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn rasterize_keeps_pixels_drops_block_and_undoes() {
    let mut doc = Document::new(200, 120);
    let block = TextBlock { text: "Hi".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let (idx, ..) = doc.add_text_layer("T".into(), block).unwrap();
    let pixels = doc.layers[idx].pixels.clone();
    doc.checkpoint();
    assert!(doc.rasterize_text_layer(idx));
    assert!(!doc.layers[idx].is_text());
    assert_eq!(doc.layers[idx].pixels, pixels);
    assert!(!doc.rasterize_text_layer(idx)); // idempotent
    assert!(doc.undo());                     // rasterize is undoable
    assert!(doc.layers[idx].is_text());
    assert_eq!(doc.layers[idx].pixels, pixels);
}

#[test]
fn merge_down_onto_text_layer_rasterizes_target() {
    let mut doc = Document::new(64, 64);
    let block = TextBlock { text: "Hi".into(), size_px: 24.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 2.0, y: 2.0, w: 0, h: 0 };
    let (idx, ..) = doc.add_text_layer("T".into(), block).unwrap();
    doc.add_layer("Layer over".into()); // becomes index idx+1
    assert!(doc.merge_down(idx + 1));
    assert!(!doc.layers[idx].is_text()); // merged result is a plain pixel layer
}
```

- [ ] **Step 2: Red** — E0599 + merge test failing on `is_text()` still true.

- [ ] **Step 3: Implement.**
  - `rasterize_text_layer(index)`: `self.layers.get_mut(index)` → if `text.is_some()` take it (`= None`), `mark_full_dirty()` not needed (pixels unchanged); return true; else false.
  - `merge_down`: right after the `src` clone / before `let dst = &mut self.layers[index - 1];`, clear the destination's block: `self.layers[index - 1].text = None;` (dst pixels are the rendered text already, so the composite is identical; the result must not pretend to be editable text). If the SOURCE had a block it vanishes with the removed layer — nothing to do.

- [ ] **Step 4: Green** — `cargo test -p procraft-engine`.

- [ ] **Step 5: Gates + commit** — `feat(engine): rasterize_text_layer; merge bakes text layers`.

---

### Task 4: Engine — text_layer_at + rehome_canvas anchor offset

**Files:**
- Modify: `crates/engine/src/document.rs` (text_layer_at near merge_down/sample; rehome_canvas :971-1042)
- Test: `crates/engine/src/document.rs` tests mod (mirror crop/resize tests at ~3597/~3623 for the rehome call shape)

**Interfaces:**
- Produces: `pub fn Document::text_layer_at(&self, x: f32, y: f32) -> Option<usize>`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn text_layer_at_hits_bounds_topmost_visible() {
    let mut doc = Document::new(200, 120);
    let a = TextBlock { text: "A".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let (i, w, h) = doc.add_text_layer("A".into(), a).unwrap();
    assert_eq!(doc.text_layer_at(10.0, 10.0), Some(i));
    assert_eq!(doc.text_layer_at(w as f32 + 9.9, 10.0), Some(i)); // inside edge
    assert_eq!(doc.text_layer_at(w as f32 + 10.1, 10.0), None);  // just outside
    assert_eq!(doc.text_layer_at(-1.0, -1.0), None);
    doc.layers[i].visible = false;
    assert_eq!(doc.text_layer_at(10.0, 10.0), None);
    doc.layers[i].visible = true;
    // topmost wins: overlapping second text layer above
    let b = TextBlock { text: "B".into(), size_px: 32.0, color: [0,0,0,255],
        style: TextStyle::default(), x: 10.0, y: 10.0, w: 0, h: 0 };
    let (j, ..) = doc.add_text_layer("B".into(), b).unwrap();
    assert_eq!(doc.text_layer_at(12.0, 12.0), Some(j));
}
```

Plus a rehome test mirroring the existing crop/resize test pattern at document.rs ~3597/3623 (same call, assert `block.x/y` shifted by the same dx/dy the call applies, and the rendered pixels still align with the block after a rerender).

- [ ] **Step 2: Red** — E0599; rehome test failing.

- [ ] **Step 3: Implement.**
  - `text_layer_at`: iterate `(0..self.layers.len()).rev()`; skip `!self.is_layer_shown(i)`; `let Some(b) = &l.text else { continue }`; hit test `x >= b.x && y >= b.y && x < b.x + b.w as f32 && y < b.y + b.h as f32`; return first hit.
  - `rehome_canvas`: read the existing loop; add, per layer, the same dx/dy it applies to `ox/oy`: `if let Some(b) = &mut layer.text { b.x += dx as f32; b.y += dy as f32; }` (use the loop's actual variable names/values — do not invent); pixels need no rerender (buffer shift is absorbed by ox/oy, and the block is the source of truth for the next rerender).
  - Note: `is_layer_shown` is `document.rs:560` — confirm signature takes `&self, index`.

- [ ] **Step 4: Green** — `cargo test -p procraft-engine`.

- [ ] **Step 5: Gates + commit** — `feat(engine): text hit-test and crop/resize anchor keeping`.

---

### Task 5: Engine — project format v11

**Files:**
- Modify: `crates/engine/src/document.rs` (save_project :2627-2760, open_project :2767+, MAX_FONT_BYTES in text.rs)
- Test: `crates/engine/src/document.rs` tests mod (mirror the hand-built v10 compat test at ~3497)

**Interfaces:**
- Consumes: `TextBlock` (Task 1).
- Produces: `pub const MAX_FONT_BYTES: usize = 2 * 1024 * 1024` in `text.rs` (app imports it in Task 8).

- [ ] **Step 1: Failing tests**
  - `v11_project_roundtrip_keeps_text_block`: doc + text layer (custom font bytes = a real tiny face: reuse the existing font fixture the text tests already use, or `FONT_BYTES`), outline + leading/tracking set; save; open; assert the opened layer `is_text()`, block equals (text, size, color, x/y, style.leading/tracking/outline; font bytes len + first bytes), and pixels equal the pre-save render.
  - `v10_project_opens_text_less`: hand-build a minimal v10 file (start from the existing v10 compat fixture ~3497, extend to one layer); open; `!layer.is_text()`.
  - `save_emits_v11_magic`: 8-byte head == `PCRAFT11` (mirror ~3365).
  - `v11_load_rejects_hostile_text_records`: hand-build v11 variants — font len > MAX_FONT_BYTES, non-finite x, text over 16 KiB — each `open_project` must Err (not panic, not silently accept).

- [ ] **Step 2: Red** — round-trip fails (block lost), magic test fails.

- [ ] **Step 3: Implement.**
  - Save: `b"PCRAFT11"`; at the END of each per-layer record (after the v8 group fields), write `u8` flag; if text: `x f32, y f32, size_px f32, color [u8;4], w u32, h u32, leading f32, tracking f32,` outline `u8` (+ `[u8;4]`, `u32` if 1), font `u8` (+ `u32 len`, bytes if 1), text `u32 len`, utf8 bytes.
  - Load: `let v11 = magic == b"PCRAFT11"; let v10 = v11 || magic == b"PCRAFT10";` chain unchanged below; in the layer loop, parse the text record only when `v11`; validate: font len ≤ `MAX_FONT_BYTES` (Err "font too large"), text len ≤ `MAX_TEXT_BYTES` (Err), floats finite (Err on non-finite), clamp `size_px` to `MIN_TEXT_PX..=MAX_TEXT_PX`, `leading/tracking` to ±`MAX_TEXT_SPACING`, `w/h` to `MAX_LAYOUT_SIDE`, outline width to `MAX_OUTLINE_PX`. Font bytes go through `check_font` (already the app pattern) — reject unparseable.
  - Truncation to the exact byte stream: keep the existing "trailing data" posture (whatever `open_project` does today after layers — read 2760-2775 before implementing).

- [ ] **Step 4: Green** — `cargo test -p procraft-engine`.

- [ ] **Step 5: Gates + commit** — `feat(engine): project format v11 carries text layers`.

---

### Task 6: App — Text tool creates/edits text layers

**Files:**
- Modify: `apps/procraft/src/lib.rs` (state ~753, init ~969; Text click ~7787-7800; dialog ~4277-4430; `text_style` ~2291)

**Interfaces:**
- Consumes: `add_text_layer`, `rerender_text_layer`, `text_layer_at`, `TextBlock`, `MAX_TEXT_BYTES`.
- Produces (app state): `editing_text: Option<usize>`, `editing_loaded: Option<(String, f32, f32)>` (identity check: original text + anchor).

- [ ] **Step 1: State + click handler.**
  - Add `editing_text: Option<usize>` and `editing_loaded: Option<(String, f32, f32)>` (init `None`).
  - Click arm (7787): `if self.text_open` → re-anchor as today (both modes). Else: if `paint_target != PaintTarget::Color` → status "Switch to a color layer to edit text."; else `doc.text_layer_at(cx, cy)` → hit: `doc.active = i`; load the block into the dialog fields (`text_body`, `text_size`, `text_leading`, `text_tracking`, `text_outline*`, `text_font_bytes`; `text_font_name = None`); `editing_text = Some(i)`; `editing_loaded = Some((block.text, block.x, block.y))`; `text_anchor = Some((block.x, block.y))`; `text_open = true`; status "Editing text layer '<name>'." Miss: today's anchor+open, `editing_text = None`.
  - Face label (~4311): show "Imported" when `text_font_bytes.is_some() && text_font_name.is_none()` (loaded custom face) instead of the wrong "Inter (bundled)".

- [ ] **Step 2: Dialog buttons + hint.**
  - Hint line (~4291): new mode → current copy; edit mode → "Tap the canvas to move the text; edit, then Done."
  - Button row: label `Render` in new mode, `Done` in edit mode (same button slot, ~4391).
  - New-mode Render: existing anchor guard ("Click the canvas to anchor the text first."); NO checkpoint; build `TextBlock` from fields + current ink + anchor; `doc.add_text_layer(format!("Text {}", doc.layers.len() + 1), block)`; Ok → status "Text layer added ({w}x{h}).", `text_open = false`, `ghost_dirty = true`, `capture_timelapse()`; Err → status `Text failed: {e}`, dialog stays.
  - Edit-mode Done: identity check first — layer exists, `is_text()`, and its block matches `editing_loaded` (text + x + y); mismatch → status "Text layer changed — edit cancelled.", close. Else `doc.checkpoint()`; copy fields + ink + `text_anchor.unwrap_or(loaded)` into the block; `rerender_text_layer(i)`; Ok → status "Text layer updated ({w}x{h}).", close, `capture_timelapse()`; Err → status, dialog stays (pixels untouched by engine contract).
  - Cancel: close + clear `editing_text`/`editing_loaded` (no doc change). Closing via the window ✕ does the same.

- [ ] **Step 3: Gates + on-device sanity via desktop build** — run the full gate battery; `cargo run -p procraft` on desktop and exercise: place text → re-tap → edit → Done; mask-mode tap hint; stale-index path (delete layer, press Done).

- [ ] **Step 4: Commit** — `feat(text): text tool creates and edits text layers (Procreate-style)`.

---

### Task 7: App — Rasterize button, paint guards, T badge, font cap

**Files:**
- Modify: `apps/procraft/src/lib.rs` — Layers toolbar ~5310 (Rasterize), row markers ~5559 (T badge), `absorb_font` ~2263 (cap), guard call sites: Brush/Eraser/Smudge (~7806), Fill, Move, Shapes (~7587), Liquify (~7526), `tform_button` (~2999), `xform_apply` (~2787), `filter_scrub` (~1917) + `restamp_preview` (~1992), Clear layer (~3921), Paint outline (~4640)

**Interfaces:**
- Consumes: `rasterize_text_layer`, `Layer::is_text`, `MAX_FONT_BYTES`.
- Produces: `fn text_paint_blocked(&mut self) -> bool` — true + status "Text layer — tap Rasterize (Layers) to paint." when the active layer is text.

- [ ] **Step 1: Guards.**
  - Implement `text_paint_blocked`; call at the START of every listed entry point (before drag state/checkbox/slider side effects; for `filter_scrub`/`restamp_preview`, check before the restamp so live previews can't touch a text layer; for `xform_apply`/`tform_button`/liquify/clear/paint-outline, before any mutation). Early-return shapes must match each arm's existing bail pattern (e.g. reset `painting = false` where the arm does).
  - Verify at implementation time that this list matches the builder-check list (design doc "Pixel-writer guard") — no writer left ungated.

- [ ] **Step 2: Rasterize button.**
  - In the Layers toolbar row (~5310, after Merge down): `let is_text = self.doc.layers.get(self.doc.active).is_some_and(|l| l.is_text());` → `add_enabled(is_text, Button::new("Rasterize"))`; click → `doc.checkpoint()`; `rasterize_text_layer(active)` → `ghost_dirty = true`, status "Text rasterized.".

- [ ] **Step 3: T badge + font cap.**
  - Row markers (~5559-5564): add `T` to the badge cluster when the layer `is_text()` (match M/C styling).
  - `absorb_font` (~2263): first line — `if bytes.len() > MAX_FONT_BYTES { return "Font too large (max 2 MB).".to_string(); }`.

- [ ] **Step 4: Gates + desktop sanity** — full battery; desktop: paint on a text layer (every guarded entry point → hint), Rasterize → paint works, badge visible.

- [ ] **Step 5: Commit** — `feat(text): rasterize action, paint guards, layer badge, font size cap`.

---

### Task 8: Ship — full gates, scoped review, push, CI, IPA

**Files:**
- Write: `.superpowers/sdd/2026-10-17-tablet-topbar/review-textlayers.diff` (diff package)
- Modify: `.superpowers/sdd/2026-10-17-tablet-topbar/progress.md` (ledger)

- [ ] **Step 1:** Ledger entries per step (intake note now; task results as they land).
- [ ] **Step 2:** Full gate battery from a clean state; record exact counts (engine grows, app stays 42). `git diff --stat` for the review package; `git diff > review-textlayers.diff` (plus untracked list).
- [ ] **Step 3:** Dispatch scoped reviewer subagent (text-layer scope: engine invariants, format, guards, undo). Triage findings (Critical/Important fixed; Minor accepted-with-ledger-note or fixed).
- [ ] **Step 4:** Push to main; `gh workflow run ios-ipa.yml`; watch; download IPA to `dist\ios-<sha>`; append ship line + device checklist to the ledger.
- [ ] **Step 5: Device checklist (hand to user):**
  1. Text tool → tap canvas → type → Render → NEW layer appears ("Text N"), text visible, layer row shows T.
  2. Text tool → tap ON the text → dialog pre-loaded → fix a typo → Done → text updates in place; tap canvas mid-edit moves it.
  3. Paint tools (brush, eraser, fill, smudge, move, shapes, liquify, filters, transform, clear, paint outline) on the text layer → hint appears, nothing painted.
  4. Rasterize (Layers toolbar, enabled only on text layers) → T badge gone → paint works → undo restores the editable text.
  5. Save project → reopen → text still editable (tap it), including an imported font + outline.
  6. Open an OLD (pre-v11) project → opens fine, no text layers.
  7. Tap text while in mask mode → hint, no silent no-op.
  8. Dialog open → delete/reorder the layer → Done → "changed — cancelled" status, no corruption.
  9. Merge down onto a text layer → result is a plain layer (no T), pixels correct.

---

## Self-Review (done)

- Spec coverage: model/API/undo/merge/format/caps/hit-test/rehome/guards/UX/limitations all mapped to Tasks 1–8; crop/resize anchored in Task 4; guard list fully enumerated in Task 7 (builder-check #2); undo claim softened (builder-check #1). ✓
- Step scan: every engine test block names its assertions with exact values; implement steps name signatures and only the decisions tests don't determine; app steps name file:line entry points. No transcript bodies. ✓
- Type consistency: `TextBlock` field names/types identical across tasks; `stamp_layout_at(index, respect_selection, …)` used by Tasks 1–2; `MAX_TEXT_BYTES`/`MAX_FONT_BYTES` used by Tasks 5/7–8. ✓
- Review Focus: 5 lines, each pinned to a task (4 engine tests, 1 device + identity re-check). ✓
- Proportion: plan ≈ 2× spec, mostly test blocks — acceptable; bodies are signatures. ✓
