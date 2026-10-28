# Plan 12 — Time-lapse on by default, Animation actions on one row

**Goal:** Time-lapse records from the first stroke (persisted, default on),
is visible without opening the Animation group, and Export GIF / Import GIF
sit on one line.

**Device verdict (e22e402 IPA, 2026-10-28):** "in animation by default make
that time-lapse on and make it more visible feature and also that export and
import gif buttons are not in same line they are up and down".

**Root cause:** `lapse_on` initialised to `false` and was session-only; its
checkbox lived inside the Animation collapsing section *and* inside the
`anim_assist` branch, so it was three taps deep. Export/Import GIF were two
plain vertical buttons in a column layout, so egui stacked them.

**Files:** `crates/engine/src/settings.rs`, `apps/procraft/src/lib.rs`.

**Steps**

- [x] `Settings::time_lapse` (default `true`) with snapshot/apply/diff wiring.
- [x] `lapse_on` starts from the setting; toggling persists immediately.
- [x] New always-visible Time-lapse card above the Animation group: record
      dot, frame count, On/Off pill, and Export/Clear when frames exist.
- [x] Delete the old in-group checkbox row (one control, one home).
- [x] Extract `gif_export_ui`/`gif_import_ui` and lay them out in one
      horizontal row.
- [x] Tests: settings default/round-trip/diff; export/import share a row via
      the extracted buttons (compile-level).
- [x] Screenshots: phone + tablet Layers drawer with the card, Animation open
      with the GIF row.
- [x] Commit: `feat(ui): time-lapse by default and a flat animation action row`.

**Exit:** a fresh install stages time-lapse frames without touching Animation,
the toggle is one tap from the Layers panel, and the two GIF actions read as
one row.
