# Plan 06 — Overlay polish, toast, modal sizing

**Goal:** make the mobile shell feel finished: motion, transient status, and
modals that fit the screen.

**Files:** `apps/procraft/src/lib.rs`.

**Steps**

- [ ] Drawer slide: animate width/offset with
  `ctx.animate_value_with_time(Id, target, 0.18)`; scrim alpha follows the same
  progress. Opening a second drawer closes the first.
- [ ] Status toast: replace the hidden status row on Mobile with a capsule above
  the options bar / strip; show the newest non-empty status for ~2.5 s
  (`ctx.input(|i| i.time)`), fade out, never intercept taps.
- [ ] Modals (Gallery, Reference, About, Shortcuts, Text, Unsaved work): on
  Mobile, clamp to 92% of the viewport in both axes and wrap the body in a
  scroll area; keep the same content and close behaviour.
- [ ] Safe areas: top inset for clusters/drawers, bottom inset for strip/options
  on iOS; Android keeps the cached top height. Rotation recomputes per frame.
- [ ] Pointer isolation audit: with a drawer open, dragging across its surface
  must not paint/select/liquify; closing and reopening keeps the active tool and
  the current scroll position of the panel body.
- [ ] Accessibility: every icon-only cluster button has a tooltip/text fallback
  when `has_symbols` is false; 44 pt minimum hit targets.
- [ ] Tests: toast expiry/fade math, drawer animation target/state transitions,
  modal size clamp; plus a regression test that `quick_controls` still covers
  every active tool.
- [ ] Screenshots: 390x844, 844x390, 1024x768, 1366x1024 in all themes, drawers
  mid-slide and open, toast visible, each modal open.
- [ ] Commit: `feat(ui): mobile drawer motion, status toast and fitted modals`.

**Exit:** no clipped modal, no toast covering controls, no paint-through, no
chrome jump on rotation.

**Log:** append a session to `chunk-26.json`.
