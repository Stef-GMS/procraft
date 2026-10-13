# ProCraft Tier-A Roadmap: near-Procreate for daily work

Goal: a credible hobbyist alternative covering ~60–70% of the features
people touch daily. Ranked by user-visible value per effort.

## Cadence (agreed 2026-10-13)

- Work in milestone batches. Local commit per milestone (protects work);
  push only when asked.
- **PC check every 5 milestones** (desktop verify + user verdict).
- **iPad check every 10 milestones** (IPA + sideload verdict).
- Batch 1 (PC check #1): M23–M27. Batch 2 (PC check #2 + iPad check #1):
  M28–M32 (exact slice decided at M27).

## Batch 1: M23–M27

- [ ] **M23 — Layer groups + group ops.** Model (nested groups with
  visible flag), panel tree UI (collapse, rename, visibility, disband),
  project-file persistence with backward-compat open. Deferred: group
  opacity, PSD group round-trip (documented gaps).
- [ ] **M24 — Smudge tool.** Finger/glove smear riding existing dab
  machinery (pick-up + push mixing), strength slider.
- [ ] **M25 — Canvas rotate.** Rotation gesture + slider + reset; overlays
  and mapping follow (same pattern as pan offset).
- [ ] **M26 — Finger-tap gestures.** Two-finger tap = undo, three-finger
  tap = redo (standard muscle memory).
- [ ] **M27 — Autosave + crash recovery.** Periodic snapshot to a recovery
  file, restore prompt on launch. **→ PC checkpoint #1.**

## P0 backlog (batches 2–3)

- [ ] Gallery/file browser (new/open/recent/duplicates).
- [ ] Crop + canvas resize (pad/trim).
- [ ] JPEG/TIFF export (PNG/PSD/GIF exist).
- [ ] Group opacity + PSD group round-trip (M23 follow-ups).

## P1 — depth on what exists

- [ ] Freeform transform: distort + warp mesh (scale/rotate/flip exist).
- [ ] Liquify-lite (push field on layer).
- [ ] Adjustments pack: Curves, HSL, Levels, Color Balance, Recolor.
- [ ] Selection upgrades: save/load, transform selection, on-canvas
  color-fill tolerance.
- [ ] Typography: import TTF/OTF, leading/tracking, outline text.
- [ ] Brush depth: dual-tip, wet-edge/glaze, per-brush blend mode.
- [ ] Color from image + loupe picker.
- [ ] Reference companion window.
- [ ] Animation frame management UI (duplicate, per-frame hold, onion
  strength per side).

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
