# Plan 03 — Canvas-first mobile shell skeleton

**Goal:** on Mobile the canvas owns the viewport; all chrome floats and the
tools/layers sidebars become slide-over drawers. This plan keeps every existing
control reachable; visual polish comes later.

**Files:** `apps/procraft/src/lib.rs` (new `MobileDrawer` state + `mobile_chrome`).

**Steps**

- [x] Add `enum MobileDrawer { None, Tools, Layers }` and a `drawer` field
  (initialised from `show_tools`/`show_layers`; session-only).
- [x] In `ui()`, when `shell == Shell::Mobile`, skip `Panel::top("menu")`,
  `Panel::top("tool-options")`, `Panel::left`, `Panel::right` and
  `Panel::bottom("status")`, and skip the tools/layers FABs. Desktop code paths
  stay byte-identical.
- [x] Implement `fn mobile_chrome(&mut self, ctx: &egui::Context, class: BarLayout)`:
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
- [x] Respect top safe insets (iOS readout; Android cached height) for the
  clusters/drawers.
- [x] Tests: pure helpers for drawer width (`min(360, 88%)`), scrim visibility,
  and `MobileDrawer` open/close toggling (no egui context needed).
- [x] Gates, then screenshots with `PROCRAFT_FORCE_SHELL=mobile` +
  `PROCRAFT_FORCE_LAYOUT=tablet`: canvas starts at the viewport top; gallery,
  file menu, tools, brushes, color, swatches, undo, layers and every layer
  action work; opening a drawer does not re-fit the canvas; tapping the scrim
  closes it. Repeat with `narrow` (same layout for now).
- [x] Commit: `feat(ui): canvas-first mobile shell with slide-over drawers`.

**Exit:** a usable mobile build with zero docked chrome; desktop untouched.

**Log:** append a session to `chunk-26.json`.

**Result (2026-10-28):** done. `MobileDrawer { None, Tools, Layers }` plus
`mobile_drawer_width` (88% clamped to 240–360 pt) and `toggled`; `drawer` is a
session field. In `ui()` every docked panel (menu, tool options, tools, layers,
status, FABs) is now behind `shell == Shell::Desktop`; Mobile draws only the
`CentralPanel` canvas plus `mobile_chrome`. The chrome is a temporary top-left
cluster (Gallery/File via `file_menu_ui`/Reference) and top-right cluster
(Undo/Redo, Tools, Layers, theme, View via `view_controls(ui, true)`) plus
scrim and drawer `Area`s that call the extracted panel bodies. Scrim or the
panel's own close arrow closes the drawer and syncs `show_tools`/`show_layers`.
Top safe inset and bottom inset honoured via cached platform heights.

92 app tests (4 new: native default, forced override, toggle semantics, width
clamp), 323 engine tests, fmt and clippy clean. Screenshots: canvas-first
tablet (`plan03-tablet.png`), Tools drawer (`plan03-tools-open.png`), Layers
drawer (`plan03-layers-open.png`), File menu (`plan03-file-menu.png`), scrim
close (`plan03-scrim-close.png`, settings show both flags false), forced narrow
(`plan03-narrow-dark.png`), desktop wide unchanged (`plan03-wide-desktop.png`).
The thin desktop strips at the capture's right/bottom edges are apps behind the
window seen through the non-client border, not canvas chrome.
