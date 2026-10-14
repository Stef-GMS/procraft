# ProCraft Tier-A Roadmap: near-Procreate for daily work

Goal: a credible hobbyist alternative covering ~60–70% of the features
people touch daily. Ranked by user-visible value per effort.

## Cadence (agreed 2026-10-13)

- Work in milestone batches. Local commit per milestone (protects work);
  push only when asked.
- **PC check every 5 milestones** (desktop verify + user verdict).
- **iPad check every 10 milestones** (IPA + sideload verdict).
- Batch 1 (PC check #1): M23–M27. Batch 2 (PC check #2 + iPad check #1):
  M28–M32 (exact slice decided at M27). Batch 3 (PC check #3): M33–M37
  (Gallery + Adjustments start, decided at v0.7.5).

## Batch 1: M23–M27 (done, awaiting PC check #1)

- [x] **M23 — Layer groups + group ops.** Nested model with effective
  visibility, panel tree (collapse/eye/rename/+Add/Ungroup), v8 format
  with v1–v7 compat open. Deferred: group opacity, PSD group round-trip.
- [x] **M24 — Smudge tool.** Core-biased finger pickup, stroke walker,
  Strength slider, mask-target hint.
- [x] **M25 — Canvas rotate.** Twist gesture + slider/Reset, GPU-rotated
  mesh view, all overlays mapped (ants march rotated edges).
- [x] **M26 — Finger-tap gestures.** Two-finger tap undo, three-finger
  tap redo (clean quick release only).
- [x] **M27 — Autosave + crash recovery.** Silent v8 snapshot every
  120 s, centered Recover/Discard prompt, cleared on save/load/clean
  exit. **→ PC checkpoint #1 (you are here).**

## Batch 2: M28–M32 (done, awaiting PC check #2 + iPad check #1)

- [x] **M28 — JPEG/TIFF export.** Quality slider for JPEG (1–100),
  plain lossless TIFF (PNG/PSD/GIF existed).
- [x] **M29 — Crop + canvas resize.** Drag-crop rect + pad/trim resize
  with anchor (lossless offsets carry content, history clears).
- [x] **M30 — Group opacity + PSD groups.** Pass-through group opacity
  in compositing/frames/GIF, v9 project format, PSD section-divisor
  round-trip (nested, open/closed, empty groups).
- [x] **M31 — Color from image + loupe.** Pick tool + magnifier ring on
  pick/Alt (touch-first), palette from image file or canvas.
- [x] **M32 — Animation frame UI.** Duplicate frame, per-frame hold
  (v10 format, GIF round-trips timing), onion strength per side.
  **→ PC checkpoint #2 + iPad check #1 (IPA) (you are here).**

## Batch 3: M33–M37 (in progress — Gallery + Adjustments start)

- [ ] **M33 — Gallery/file browser.** New/open/recent/duplicates with
  thumbnails; replaces OS-dialog-only flow (dialogs stay as fallback).
- [ ] **M34 — Reference companion window.** Pinned image alongside canvas.
- [ ] **M35 — Selection upgrades.** Save/load selection, transform
  selection, on-canvas color-fill tolerance UI.
- [ ] **M36 — Adjustments I.** Curves + Levels.
- [ ] **M37 — Adjustments II.** HSL + Color Balance + Recolor.
  **→ PC checkpoint #3 (you are here when done).**

## P0 backlog (done)

- [x] Gallery/file browser — moved into Batch 3 above.

## P1 — depth on what exists

- [ ] Freeform transform: distort + warp mesh (scale/rotate/flip exist).
- [ ] Liquify-lite (push field on layer).
- [ ] Adjustments pack: Curves, HSL, Levels, Color Balance, Recolor.
- [ ] Selection upgrades: save/load, transform selection, on-canvas
  color-fill tolerance.
- [ ] Typography: import TTF/OTF, leading/tracking, outline text.
- [ ] Brush depth: dual-tip, wet-edge/glaze, per-brush blend mode.
- [ ] Reference companion window.

## P2 — performance + platform (when P0 hurts)

- [ ] 8K canvas, 100+ layers with lazy compositing.
- [ ] Palm rejection + Pencil double-tap/hover.
- [ ] Time-lapse recording.
- [ ] iPad layout polish pass.
- [ ] Stability hardening: fuzz project open, OOM guards on import.

## Explicit non-goals (until Tier A ships)

GPU render path · CMYK/P3 color management · cloud sync · App Store
submission process · ML filters · 3D / text-on-path.

## Shipped context (v0.7.1, milestones 1–22)

Layered model + masks + 10 blends, brush engine (dynamics, library,
abr/brushset/kpp/gbr interop), rect/lasso + Selection Studio (weighted
masks, bbox ants), flood + ColorDrop, shapes, symmetry/kaleido + movable
center, guides + Drawing Assist, palettes + harmonies, animation
(frames/onion/GIF), baked text, lossless offsets, PSD interchange,
compressed projects (v7), undo byte-budget, iOS sideload builds.
