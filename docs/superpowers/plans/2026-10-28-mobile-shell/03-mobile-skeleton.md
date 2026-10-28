# Plan 03 — Canvas-first mobile shell skeleton

**Goal:** on Mobile the canvas owns the viewport; all chrome floats and the
tools/layers sidebars become slide-over drawers. This plan keeps every existing
control reachable; visual polish comes later.

**Files:** `apps/procraft/src/lib.rs` (new `MobileDrawer` state + `mobile_chrome`).

**Steps**

- [ ] Add `enum MobileDrawer { None, Tools, Layers }` and a `drawer` field
  (initialised from `show_tools`/`show_layers`; session-only).
- [ ] In `ui()`, when `shell == Shell::Mobile`, skip `Panel::top("menu")`,
  `Panel::top("tool-options")`, `Panel::left`, `Panel::right` and
  `Panel::bottom("status")`, and skip the tools/layers FABs. Desktop code paths
  stay byte-identical.
- [ ] Implement `fn mobile_chrome(&mut self, ctx: &egui::Context, class: BarLayout)`:
  - scrim `Area` (`Order::Middle`) while a drawer is open; a click closes it;
  - drawer `Area` (`Order::Foreground`) anchored left/right, width
    `min(360.0, 0.88 * viewport.width())`, height below the top safe inset,
    solid token frame, and **the extracted `tools_panel_body` /
    `layers_panel_body`** (no nested scroll areas);
  - a temporary top-left cluster with Gallery, File (extracted menu), Reference;
  - a temporary top-right cluster with Undo, Redo, Tools, Layers, theme toggle
    and a View menu (`view_controls(ui, true)`);
  - the panel bodies' own close buttons set `show_tools`/`show_layers` false;
    after each body, sync the drawer state.
- [ ] Respect top safe insets (iOS readout; Android cached height) for the
  clusters/drawers.
- [ ] Tests: pure helpers for drawer width (`min(360, 88%)`), scrim visibility,
  and `MobileDrawer` open/close toggling (no egui context needed).
- [ ] Gates, then screenshots with `PROCRAFT_FORCE_SHELL=mobile` +
  `PROCRAFT_FORCE_LAYOUT=tablet`: canvas starts at the viewport top; gallery,
  file menu, tools, brushes, color, swatches, undo, layers and every layer
  action work; opening a drawer does not re-fit the canvas; tapping the scrim
  closes it. Repeat with `narrow` (same layout for now).
- [ ] Commit: `feat(ui): canvas-first mobile shell with slide-over drawers`.

**Exit:** a usable mobile build with zero docked chrome; desktop untouched.

**Log:** append a session to `chunk-26.json`.
