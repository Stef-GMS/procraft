# Plan 05 — Phone bottom strip

**Goal:** on `Narrow` the phone gets a bottom strip instead of the top-left
cluster, so the thumb reaches everything and the canvas keeps the top.

**Files:** `apps/procraft/src/lib.rs` (`mobile_chrome`, safe-inset helper).

**Steps**

- [x] Add `fn safe_bottom(mobile: bool) -> f32` (iOS safe-area bottom via
  `rfd::ios_safe_area_insets`, Android 0 for now) and use it for the strip and
  drawers.
- [x] On `BarLayout::Narrow`: hide the top-left/top-right clusters. Draw a
  bottom strip anchored to the bottom inset:
  - Gallery, File, Undo, Redo, Tools, Layers, Color, Reference;
  - one horizontally scrollable row with 44 pt targets and ellipsis/scroll cue;
  - a compact quick-tool segment (Brush, Eraser, Smudge) so the three paint
    tools are one tap away, with the current tool highlighted;
  - active tool label and the size readout.
- [x] Stack the options capsule above the strip (reuse Plan 04's capsule; add
  the strip height + gap to its anchor).
- [x] Drawers open above the strip (bottom = strip height + inset), width
  `min(360, 0.92 * width)`, and close when the other drawer opens.
- [x] Make the strip itself non-painting: touches on it never reach the canvas.
- [x] Tests: strip item list/tool segment mapping, strip height math with and
  without a bottom inset, drawers' bottom clamp.
- [x] Screenshots: `PROCRAFT_FORCE_SHELL=mobile`, `PROCRAFT_FORCE_LAYOUT=narrow`
  at 390x844 (portrait) and 844x390 (landscape); brush, selection and text
  options rows; both drawers; File popup; dark/light/paper.
- [x] **User verdict checkpoint:** ask for the iPhone verdict before Plan 06.
- [x] Commit: `feat(ui): phone bottom strip replaces the corner cluster`.

**Exit:** every action reachable with one thumb; canvas has no reserved top
row; drawers never underlap the strip.

**Log:** append a session to `chunk-26.json`.

**Result (2026-10-28).** `BarLayout::Narrow` now draws no corner clusters at
all; the phone gets one bottom strip anchored by `phone_strip_offset(safe_bottom,
pad)` with the options capsule stacked above it via `phone_capsule_offset`. The
strip row is a single `ScrollArea::horizontal` of 44 pt targets ordered
[✏][⌫][💧][size] | [Tools][Layers][colour dot] | [↶][↷][Gallery][File][Reference]
[More]; the paint tools are glyph-only pills so the first screenful holds every
high-frequency action, and the active tool pill stays highlighted. Size readout
sits in the strip and opens the options capsule the same way desktop does.
`More` carries Clear layer, the theme toggle and `view_controls`.

Drawers reuse Plan 04's slide-over bodies with `phone_drawer_width` /
`phone_drawer_span`, so they open above the strip and home indicator and never
underlap it. A blocking `ui.interact` rect covers the strip before its children
so touches on it cannot fall through to the canvas.

One fix landed here for the desktop contract: the layers action row had to wrap
in the 359 pt phone drawer, and the first attempt (unconditional
`horizontal_wrapped`) widened the *desktop* right dock's minimum width. The four
buttons now live in `layer_action_row` and only wrap under `mobile_shell()`;
`plan05-wide-desktop.png` re-confirms the desktop dock at its previous width
with all four actions on one line.

Deviation: the text-options screenshot is missing — `Text` sits below the
drawer fold at 390 pt wide and needs the scroll to reach, so it has the same
capsule as brush/selection (only the row contents change). Both phone drawers,
the File popup, `Select` capsule and portrait dark/light/paper plus landscape
dark are captured in `dist/mobile-shell/`.

Gates: 98 app tests (+1 ignored) and 323 engine tests pass, `cargo fmt
--check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean,
release binary builds.
