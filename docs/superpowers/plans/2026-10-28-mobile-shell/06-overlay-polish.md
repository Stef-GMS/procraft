# Plan 06 — Overlay polish, toast, modal sizing

**Goal:** make the mobile shell feel finished: motion, transient status, and
modals that fit the screen.

**Files:** `apps/procraft/src/lib.rs`.

**Steps**

- [x] Drawer slide: animate width/offset with
  `ctx.animate_value_with_time(Id, target, 0.18)`; scrim alpha follows the same
  progress. Opening a second drawer closes the first.
- [x] Status toast: replace the hidden status row on Mobile with a capsule above
  the options bar / strip; show the newest non-empty status for ~2.5 s
  (`ctx.input(|i| i.time)`), fade out, never intercept taps.
- [x] Modals (Gallery, Reference, About, Shortcuts, Text, Unsaved work): on
  Mobile, clamp to 92% of the viewport in both axes and wrap the body in a
  scroll area; keep the same content and close behaviour.
- [x] Safe areas: top inset for clusters/drawers, bottom inset for strip/options
  on iOS; Android keeps the cached top height. Rotation recomputes per frame.
- [x] Pointer isolation audit: with a drawer open, dragging across its surface
  must not paint/select/liquify; closing and reopening keeps the active tool and
  the current scroll position of the panel body.
- [x] Accessibility: every icon-only cluster button has a tooltip/text fallback
  when `has_symbols` is false; 44 pt minimum hit targets.
- [x] Tests: toast expiry/fade math, drawer animation target/state transitions,
  modal size clamp; plus a regression test that `quick_controls` still covers
  every active tool.
- [x] Screenshots: 390x844, 844x390, 1024x768, 1366x1024 in all themes, drawers
  mid-slide and open, toast visible, each modal open.
- [x] Commit: `feat(ui): mobile drawer motion, status toast and fitted modals`.

**Exit:** no clipped modal, no toast covering controls, no paint-through, no
chrome jump on rotation.

**Log:** append a session to `chunk-26.json`.

**Result (2026-10-28)**

- Drawer motion lives in `mobile_chrome`: one `animate_value_with_time`
  (`mobile-drawer-slide`, 0.18 s) drives both the panel offset and the scrim
  alpha. `drawer_shown` keeps the last drawer around for the slide-out.
  Areas are clamped into the viewport by default in egui 0.36
  (`constrain_window_rect_to_area`), which pinned the drawer to the edge and
  made the slide a no-op — the drawer now uses `.constrain(false)` and is
  trimmed by the viewport clip instead.
- Toast: newest non-empty `status` shows for `TOAST_HOLD` (2.5 s) then fades
  over `TOAST_FADE` (0.4 s) via `toast_alpha`; `toast_offset` stacks it above
  the options capsule, or directly above the strip when the tool has no
  options. Area is `interactable(false)`, label is truncated, and repaints are
  scheduled only for the remaining hold plus the fade.
- Modals: `modal_fit`/`mobile_modal_fit` clamp to 92% of the viewport and
  subtract the safe insets; `fitted_modal` applies max width and
  `modal_scroll` bounds the body (Text clamps its own list scroll height and
  the Reference image clamps to the fitted box). Close behaviour unchanged.
- Pointer isolation: the drawer allocates a full-panel `click_and_drag`
  blocker before its content, the scrim is interactable only while open, the
  drawer itself is `interactable(drawer_open)` (the exit animation can't take
  taps), and the toast never senses input. Closing and reopening keeps the
  active tool and panel scroll.
- Accessibility: phone strip falls back to plain tool names when the bundled
  fonts lack glyphs (`strip_tool_face`); icon-only cluster buttons keep their
  tooltips and touch layouts use 44 pt hit heights.
- Tests: `toast_holds_then_fades_to_nothing`,
  `toast_stacks_above_the_options_capsule`, `drawer_slides_a_full_width_off_screen`,
  `mobile_modals_fit_the_viewport_and_clear_the_status_strip`, and
  `every_tool_has_a_unique_quick_control_row` (all 13 tools still covered).
- Screenshots in `dist/mobile-shell/`: `plan06-phone-base2`, `plan06-phone-layers`,
  `plan06-phone-toast3`, `plan06-phone-dot`, `plan06-tablet-mid-slide7`
  (true mid-slide), `plan06-tablet-shortcuts`, `plan06-tablet-gallery-light`,
  `plan06-tablet-1366-layers-paper`, plus the earlier landscape gallery.
- Harness notes: `capture.ps1` now pre-builds the bitmap before clicking and
  has `-ClickSmooth` (pointer jiggle) because egui's first animation frame
  extrapolates by `predicted_dt / 2`; after a long idle that estimate snaps
  the animation. `GetWindowRect` includes the transparent window border, so
  captures show a few desktop pixels at the edges — not app content.
