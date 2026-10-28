# Plan 04 — Tablet floating clusters and options capsule

**Goal:** the iPad arrangement: top-left actions, top-right layers/undo/color,
per-tool options floating at the bottom, canvas clear between them.

**Files:** `apps/procraft/src/lib.rs` (`mobile_chrome`).

**Steps**

- [ ] Finalize the **top-left cluster**: Gallery, File, Reference as translucent
  capsules in one rounded container, 12 pt from the top inset and leading edge.
- [ ] Finalize the **top-right cluster**: Undo, Redo, Layers, Color, theme.
  - Layers toggles the Layers drawer.
  - Color opens the Tools drawer (best effort scroll to the Color card; if the
    scroll-to needs a new id, add one — do not duplicate the color UI).
- [ ] Replace the temporary top View row with a small View capsule that opens
  the existing `view_controls(ui, true)` in a popup (zoom, Fit, rotate, Reset,
  About, Shortcuts).
- [ ] Add the **options capsule**: an `Area` anchored bottom-centre above the
  safe inset, max width `min(760, 0.94 * width)`, token frame, horizontal
  scroll, rendering `tool_options_bar`'s control row. Hidden when
  `quick_controls(tool)` is empty; keep the current-tool label inside.
- [ ] Keep the drawers from Plan 03; raise their top below the clusters and
  clamp height so the last control is reachable above the options capsule.
- [ ] Tests: options capsule visibility (`quick_controls(tool).is_empty()`),
  cluster anchor points/safe-inset math, drawer height clamp.
- [ ] Screenshots: `PROCRAFT_FORCE_SHELL=mobile`, `PROCRAFT_FORCE_LAYOUT=tablet`,
  all three themes; brush/liquify/selection (long scrolling row), drawer open
  left and right, File and View popups open.
- [ ] **User verdict checkpoint:** ship the Windows-simulated screenshots and
  ask for the iPad verdict before Plan 06 polish.
- [ ] Commit: `feat(ui): tablet floating clusters and options capsule`.

**Exit:** no chrome touches the canvas edges except the floating islands; every
control from the old top bar/tool-options bar is reachable.

**Log:** append a session to `chunk-26.json`.
