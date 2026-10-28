# Plan 04 — Tablet floating clusters and options capsule

**Goal:** the iPad arrangement: top-left actions, top-right layers/undo/color,
per-tool options floating at the bottom, canvas clear between them.

**Files:** `apps/procraft/src/lib.rs` (`mobile_chrome`).

**Steps**

- [x] Finalize the **top-left cluster**: Gallery, File, Reference as translucent
  capsules in one rounded container, 12 pt from the top inset and leading edge.
- [x] Finalize the **top-right cluster**: Undo, Redo, Layers, Color, theme.
  - Layers toggles the Layers drawer.
  - Color opens the Tools drawer (best effort scroll to the Color card; if the
    scroll-to needs a new id, add one — do not duplicate the color UI).
- [x] Replace the temporary top View row with a small View capsule that opens
  the existing `view_controls(ui, true)` in a popup (zoom, Fit, rotate, Reset,
  About, Shortcuts).
- [x] Add the **options capsule**: an `Area` anchored bottom-centre above the
  safe inset, max width `min(760, 0.94 * width)`, token frame, horizontal
  scroll, rendering `tool_options_bar`'s control row. Hidden when
  `quick_controls(tool)` is empty; keep the current-tool label inside.
- [x] Keep the drawers from Plan 03; raise their top below the clusters and
  clamp height so the last control is reachable above the options capsule.
- [x] Tests: options capsule visibility (`quick_controls(tool).is_empty()`),
  cluster anchor points/safe-inset math, drawer height clamp.
- [x] Screenshots: `PROCRAFT_FORCE_SHELL=mobile`, `PROCRAFT_FORCE_LAYOUT=tablet`,
  all three themes; brush/liquify/selection (long scrolling row), drawer open
  left and right, File and View popups open.
- [x] **User verdict checkpoint:** ship the Windows-simulated screenshots and
  ask for the iPad verdict before Plan 06 polish.
- [x] Commit: `feat(ui): tablet floating clusters and options capsule`.

**Exit:** no chrome touches the canvas edges except the floating islands; every
control from the old top bar/tool-options bar is reachable.

**Log:** append a session to `chunk-26.json`.

**Result (2026-10-28):** implemented. Top-left island: Gallery, File,
Reference plus a Clear action (the mobile shell had dropped the old top bar's
Clear layer; it now calls the shared `clear_layer` helper, which the desktop
pill uses too). Top-right island: undo, redo, Tools, Layers, a Color dot that
shows the current paint color and opens the Tools drawer scrolled to the Color
card (`mobile_scroll_color`, one-shot `scroll_to_rect`), theme toggle and the
View popup. Islands are translucent capsules (`mobile_island_height`); the
options capsule is bottom-centred, `min(760, 94% width)` wide, horizontally
scrolling and hidden for tools with no quick controls. Drawers now start below
the islands and end above the capsule via `mobile_drawer_span`.

95 app tests (+3: capsule width, island height, drawer span), 323 engine
tests, fmt and clippy clean. Screenshots: `plan04-tablet-{dark,light,paper}.png`,
`plan04-tools-open.png`, `plan04-layers-open.png`, `plan04-select.png` (long
row), `plan04-hand.png` (capsule hidden), `plan04-color-dot.png` (drawer on the
Color card), `plan04-view-menu.png`, `plan04-file-menu.png`,
`plan04-wide-desktop.png` (desktop unchanged).

**Open verdict:** the tablet arrangement needs the user's iPad verdict before
Plan 06 polish; Plan 05 (phone strip) can land first.
