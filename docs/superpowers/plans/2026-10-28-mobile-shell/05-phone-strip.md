# Plan 05 — Phone bottom strip

**Goal:** on `Narrow` the phone gets a bottom strip instead of the top-left
cluster, so the thumb reaches everything and the canvas keeps the top.

**Files:** `apps/procraft/src/lib.rs` (`mobile_chrome`, safe-inset helper).

**Steps**

- [ ] Add `fn safe_bottom(mobile: bool) -> f32` (iOS safe-area bottom via
  `rfd::ios_safe_area_insets`, Android 0 for now) and use it for the strip and
  drawers.
- [ ] On `BarLayout::Narrow`: hide the top-left/top-right clusters. Draw a
  bottom strip anchored to the bottom inset:
  - Gallery, File, Undo, Redo, Tools, Layers, Color, Reference;
  - one horizontally scrollable row with 44 pt targets and ellipsis/scroll cue;
  - a compact quick-tool segment (Brush, Eraser, Smudge) so the three paint
    tools are one tap away, with the current tool highlighted;
  - active tool label and the size readout.
- [ ] Stack the options capsule above the strip (reuse Plan 04's capsule; add
  the strip height + gap to its anchor).
- [ ] Drawers open above the strip (bottom = strip height + inset), width
  `min(360, 0.92 * width)`, and close when the other drawer opens.
- [ ] Make the strip itself non-painting: touches on it never reach the canvas.
- [ ] Tests: strip item list/tool segment mapping, strip height math with and
  without a bottom inset, drawers' bottom clamp.
- [ ] Screenshots: `PROCRAFT_FORCE_SHELL=mobile`, `PROCRAFT_FORCE_LAYOUT=narrow`
  at 390x844 (portrait) and 844x390 (landscape); brush, selection and text
  options rows; both drawers; File popup; dark/light/paper.
- [ ] **User verdict checkpoint:** ask for the iPhone verdict before Plan 06.
- [ ] Commit: `feat(ui): phone bottom strip replaces the corner cluster`.

**Exit:** every action reachable with one thumb; canvas has no reserved top
row; drawers never underlap the strip.

**Log:** append a session to `chunk-26.json`.
