# Text Layers (Procreate-style editable text) — Design

- Status: APPROVED — user 2026-10-18 (message: "In procreate it first create a Vector Text layer so we can edit it and then we can rasterize. fix this text tool while i check this build") — explicit directive; this doc records the interpretation of that directive. Builder-check applied. Any deviation surfaced in chat.
- Date: 2026-10-18
- Owner: Saran (product), agent (implementation)

## What it looks like when it's done

The Text tool no longer bakes letters onto the active layer at Render time. Instead:

1. Pick Text, tap the canvas → the Text window opens anchored where you tapped (same dialog UI as today: body, size, face, leading, tracking, outline, Import font).
2. Press **Render** → a NEW layer is created, named "Text 1", "Text 2", … **The text stays editable.** The layer renders like any other layer (visibility, opacity, blend, groups, merge, export all work on it).
3. With the Text tool, tap ON the text → the Text window opens pre-loaded with that text's content and style. Edit anything, press **Done** → the text re-renders in place (old pixels cleared, new stamped; anchor can be moved by tapping the canvas while the window is open).
4. The Layers panel gets a **Rasterize** button (enabled when the active layer is a text layer). Press it → the text data is dropped; the layer becomes a plain pixel layer. After that it is no longer editable as text (undoable).
5. Painting/erasing/smudging/filling/shapes/moving on a text layer is blocked with a hint: "Text layer — tap Rasterize (Layers) to paint." Merge down onto/involving a text layer rasterizes it first.
6. Text layers show a small "T" badge in the layer row.
7. Save/open round-trips text layers in `.procraft` files (format v11; v10 and older files still open — their layers have no text data).

## Who it is for

Saran, on iPad (LiveContainer), using the app like Procreate. Success = type, place, notice a typo, tap it, fix it, move it — without the text having been destroyed at placement time.

## Success criteria

- Place → edit → re-edit → rasterize → paint over, all without reloading the doc.
- Undo/redo covers: every edit/re-render and rasterize. (Layer *creation* is NOT undoable — deliberately matching Add layer, which the engine's per-layer-pixel undo model has never covered; a falsy "creation undo" step is explicitly avoided.)
- Saving and reopening a project preserves editable text exactly (content, size, color, face incl. imported font, leading/tracking, outline, anchor).
- A v10 project file still opens (text-less); old app builds cleanly refuse a v11 file at the magic (same as they do for v10).
- No visual change to any non-text feature; full gate battery green.

## How it gets made

### Core model (engine)

A text layer is a normal `Layer` plus `text: Option<TextBlock>`. The pixel buffer is ALWAYS kept up to date (rendered from the block). This single decision means compositing, groups, masks, clip, merge, flatten, thumbnails, PSD export, eyedropper, and the timelapse all keep working untouched — they see pixels.

The alternative (a "vector-only" layer rendered at screen resolution for crisp zoom) was rejected for v1: it would fork the entire pipeline. Consequence: text is laid out at canvas resolution, so zooming past 100% shows the same pixel edges a rasterized layer would. Editability is the point; crisp-zoom rendering is a possible follow-up.

- `TextBlock` (in `crates/engine/src/text.rs`):
  - `text: String` (post-`MAX_TEXT_CHARS`/`MAX_TEXT_LINES` validation at render time, like today)
  - `size_px: f32`
  - `color: [u8; 4]` (snapshot of the ink at each Render/Done — same rule as today: the current color swatch is the text color; changing the swatch and pressing Done re-colors)
  - `style: TextStyle` (font bytes, leading, tracking, outline — the exact struct the dialog already fills)
  - `x: f32, y: f32` (anchor: TOP-LEFT of the block, canvas pixels — same convention as `draw_text_styled`)
  - `w: u32, h: u32` (last rendered block size; for hit-testing; refreshed on every render)
- `Layer` gains `text: Option<TextBlock>`; `Layer::blank` = `None`; all existing constructors/importers leave it `None` (clone/duplicate carry it for free).

Single-writer invariant: **only** `render_text_layer` writes a text layer's pixels (it clears the whole buffer first). Every other pixel-writer is gated off in the app while a text layer is active. This is what keeps block and pixels consistent.

### Engine API (new pub fns)

- `Document::add_text_layer(name: String, block: TextBlock) -> Result<(usize, u32, u32), String>`
  Pushes a layer with the block, renders it, returns (index, w, h).
- `Document::rerender_text_layer(index: usize) -> Result<(u32, u32), String>`
  Clears the layer buffer (`pixels.fill(0)` + `mark_full_dirty`), stamps the block (fill + outline passes exactly as `draw_text_styled` does today), updates `block.w/h`. **Ignores the selection mask** (selection is a paint concept; a text layer's content is wholly owned by its block). Requires a small `stamp_layout` refactor: parametrize to `stamp_layout(index, respect_selection)`; `draw_text_styled` delegates with (active, true) so its behavior and its selection test stay identical.
- `Document::rasterize_text_layer(index: usize) -> bool`
  `block = None` (pixels untouched). Returns whether a block was dropped.
- `Document::text_layer_at(x: f32, y: f32) -> Option<usize>`
  Topmost visible layer whose block bounds `(x..x+w, y..y+h)` contain the point. Lives in the ENGINE so it is unit-testable. Respects layer visibility. When `paint_target != Color` the app shows a status hint ("Switch to a color layer to edit text.") instead of silently no-op'ing.
- `Layer::is_text()` helper.

### Undo

Existing `UndoEntry` snapshots pixels + mask + ox/oy of the ACTIVE layer — not the full structure (layer add/remove/duplicate/merge are not undoable today; the app does not checkpoint them). It gains `text: Option<TextBlock>` (cloned on `snapshot()`, restored on undo AND redo) and `byte_size()` counts the text/font bytes. Rationale: without this, rasterize would be un-undoable (pixels don't change!) and edits would desync. Cost: one block clone per history step on a text layer (font bytes included) — bounded by the existing entry/byte caps.

Consequences, made explicit:
- **Render-new does NOT checkpoint.** `add_text_layer` pushes a layer; the snapshot model cannot remove it again, so a checkpoint there would be a silent no-op step (worse than none). Creation is not undoable — same as "Add layer".
- Edit/Done (rerender on an existing index) and Rasterize DO checkpoint — both are pixel/block changes to an existing layer, fully expressible.
- Pre-existing hazard inherited (not made worse): an undo entry whose `layer` index no longer exists consumes the step and returns false. Merge/remove already behave this way.

### Merge down

`merge_down` rasterizes any text layer it touches: if the destination has a block → drop it before compositing (dst pixels are already the rendered text, so the result is identical to today's merge); the source layer disappears with the merge as usual (if the source had a block it vanishes with it). After a merge, nothing editable remains from either side — matching Procreate.

### Project file v11

Magic `PCRAFT11`. The per-layer record gains, after the existing fields (end of the per-layer section, so the v1-v10 parse layouts are untouched):

- flag `u8` (0 = no text)
- if 1: `x f32, y f32, size_px f32`, `color [u8;4]`, `w u32, h u32`, `leading f32, tracking f32`, outline `flag u8` (+ `[u8;4] color, u32 width` if set), font `flag u8` (+ `u32 len, bytes` if set), text `u32 len, utf8 bytes`.

Loader: `v11 = magic == PCRAFT11`; `v10 = v11 || magic == PCRAFT10` (all downstream flags chain as today); text parse only when `v11` and the flag is 1. Save always writes v11. Old builds refuse v11 at the magic (already the pattern). A hand-built v10 fixture test proves older files still open text-less.

Caps and sanitization (the format is written by us but loaded from arbitrary files):
- New `MAX_FONT_BYTES` (2 MiB) enforced at font import (`absorb_font`), at save, and at load (reject, don't truncate silently).
- Text bytes capped (~16 KiB; `MAX_TEXT_CHARS` 1024 chars ≤ 4 KiB UTF-8) and re-validated on load (`MAX_TEXT_CHARS`/`MAX_TEXT_LINES`) so malformed files can't produce text the editor refuses.
- Non-finite `x/y/size_px/leading/tracking` rejected on load; `size_px` clamped to `MIN_TEXT_PX..=MAX_TEXT_PX`, `w/h` to `MAX_LAYOUT_SIDE` (group-opacity sanitization precedent).
- Name/count caps unchanged (project layer name len is already just a u32; loader keeps its existing truncation posture).

### App flow (lib.rs)

State: `editing_text: Option<usize>` (Some = the dialog is editing that layer). Dialog fields (`text_body`, `text_size`, leading/tracking/outline, font bytes/name) are shared between modes — mode just decides what Render/Done does and whether Cancel discards.

- Text tool click (dialog closed):
  - `doc.text_layer_at(x, y)` hit → `active = i`, load block into the dialog fields, `editing_text = Some(i)`, open dialog. Status: "Editing text layer '<name>'."
  - Miss → existing anchor-and-open behavior, `editing_text = None`.
- Canvas click while the dialog is open: re-anchors (existing behavior). Applies in BOTH modes; on Done the block's anchor moves. The hint line changes per mode: new → "Click the canvas to move the anchor, then Render."; edit → "Click the canvas to move the text; edit, then Done."
- **Render** (new mode): **no checkpoint** (see Undo) → `add_text_layer(format!("Text {}", count), block from fields + ink + anchor)` → status "Text layer added (WxH)." → `capture_timelapse()` → close. Errors (blank text, too big, bad font) show in status and the dialog stays open (exactly today's error handling). `add_text_layer` validates/renders BEFORE pushing the layer (no half-added layer on error).
- **Done** (edit mode): `checkpoint()` → copy fields + ink + anchor into the block → `rerender_text_layer` → same status/lapse/close semantics. Identity re-check before acting: the indexed layer still exists, is still text, and its block still matches what was loaded into the dialog (anchor+text) — otherwise status error, close. Guards against reorder/remove/merge index shifts while the dialog sat open.
- **Cancel**: close, no doc change (unchanged).
- Rasterize button in the Layers toolbar row (Add layer / Duplicate / Merge down / …): enabled iff `doc.layers[active].is_text()`; `checkpoint()` → `rasterize_text_layer(active)` → status "Text rasterized."
- Pixel-writer guard — ONE helper (`active_text_layer_paint_blocked() -> Option<&'static str>`), checked at every entry point below. Full list (builder-check verified each writes active-layer pixels):
  - Tools: Brush, Eraser, Smudge, Fill, Move, Shapes, Liquify (`doc.liquify`, ~lib.rs:7526).
  - Layers panel: Clear layer (~3921), Merge down onto a text layer (engine handles: rasterize dst first).
  - Transform-layer buttons (`tform_button` ~2999-3086; rewrites pixels AND ox/oy), Distort/Mesh (`xform_apply` ~2787-2801).
  - Filters panel (~6132-6520: Tone/Blur/Sharpen/Levels/Curves + `apply_lut`) — checked at the SLIDER-CHANGE site (`filter_scrub` ~1917-1984 / `restamp_preview` ~1992-2019) so live previews can't restamp a text layer, not only at Apply.
  - Paint outline (`stroke_selection` ~4640).
  Selection tools, eyedropper, undo/redo, layer visibility/opacity/blend/rename/reorder, export stay allowed. Text tool stays allowed. Any pixel-writer found later that isn't on this list gets the same guard.
- Crop + canvas resize: both funnel through `rehome_canvas` (document.rs:971-1042), which absorbs the shift into each layer's ox/oy — buffer content stays canvas-correct with no rerender. The ONLY text fix needed: in that same per-layer loop, offset `block.x/y` by the same (dx, dy). Engine-side → engine test. Crop clears undo, so no interplay.
- Layer row: "T" badge next to the existing `M`/`C` markers render (~lib.rs:5559-5564) when the layer is text.

### Tests (TDD — red first, always)

Engine:
- `add_text_layer` creates a layer, renders pixels (non-empty coverage at the anchor), stores the block; an invalid block (blank text / oversize) adds NO layer (validate-before-push).
- `rerender_text_layer` clears the old position and stamps the new one (move anchor → old area empty, new area painted); size updates.
- `rasterize_text_layer` keeps pixels, drops block; second call returns false.
- Undo after rasterize restores the block; redo drops it again.
- Undo after an edit-restamp restores both pixels and block (add → edit → undo returns the original render).
- `merge_down` onto a text layer: block gone from the merged result, pixels = normal merge.
- `text_layer_at`: hits bounds, misses outside, respects visibility, picks topmost.
- `rehome_canvas` offsets text block anchors (crop/resize consistency).
- v11 round-trip: block with text/color/anchor/size/leading/tracking/outline/custom font bytes survives save→open byte-perfect; a v10 hand-built fixture opens text-less; save emits `PCRAFT11`; oversized font/text and non-finite fields are rejected on load.
- `Layer.text` default `None` added at the 4 construction literals (document.rs:3088, animation.rs:333, psd.rs:462+492) — existing suites prove no behavior change.

App:
- Follow the existing app-test patterns (42 tests). At minimum: the dialog-mode transitions that don't need a painter (new vs edit field loading is in the click handler; if not constructible, note it and rely on the engine hit test + device pass).

### Known limitations (v1, deliberate)

- Text is canvas-resolution (soft when zoomed past 100%) — crispness follows rasterization behavior.
- No rotation of text blocks; no per-character editing beyond the whole block (same as today's dialog).
- Color = the ink swatch at Render/Done time (documented in the status/hint), matching the current tool.
- Fill/shape/lasso tool interactions with text layers are blocked, not auto-rasterizing (Procreate prompts; we hint — a lighter version of the same behavior).
- Text-layer creation is not undoable (matches Add layer; pre-existing engine model).
- Merging remains not undoable (pre-existing; unchanged by this feature).
- Default layer names can repeat after deletions ("Text 1" collides) — accepted v1, same as "Layer N".

## Open questions (none blocking)

- Row badge shape ("T" glyph vs tinted chip) — cosmetic, decide at implementation.
- Whether a future release adds a one-tap "Rasterize?" prompt on first paint attempt — out of v1 scope.
