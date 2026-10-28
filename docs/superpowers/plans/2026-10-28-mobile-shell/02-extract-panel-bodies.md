# Plan 02 — Extract panel and File-menu bodies

**Goal:** the exact same UI can be presented docked (desktop) or inside a
slide-over drawer (mobile), without duplicating ~2,200 lines.

**Files:** `apps/procraft/src/lib.rs`.

**Steps**

- [ ] Move the left panel's inner `ScrollArea` body (currently inside
  `Panel::left("tools-compact").show_collapsible`, `:6354-7234`) into
  `fn tools_panel_body(&mut self, ui: &mut egui::Ui)`.
- [ ] Move the right panel's inner `ScrollArea` body (`:7247-8594`) into
  `fn layers_panel_body(&mut self, ui: &mut egui::Ui)`.
- [ ] Move the File menu body (`:5504-5720`) into
  `fn file_menu_ui(&mut self, ui: &mut egui::Ui, layout: BarLayout, bar_t: &design::Tokens)`;
  the caller keeps `MenuButton`/`MenuConfig` and the `ui.close()` calls stay
  inside the menu items.
- [ ] Rewire the three docked call sites to the methods. No ids, widths,
  content, ordering, state flags or close behaviour change.
- [ ] Build, run all tests, clippy and fmt.
- [ ] Screenshot `wide` dark before/after with the screenshot harness and
  compare: the two images should be identical apart from timestamps/noise.
  Also open File and confirm every item still closes the menu.
- [ ] Commit: `refactor(ui): extract panel and file-menu bodies for reuse`.

**Exit:** desktop dock renders pixel-identically; mobile can call the same
methods.

**Log:** append a session to `chunk-26.json`.
