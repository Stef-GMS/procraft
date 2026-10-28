# Plan 08 — Device-verdict fixes (iterative)

**Goal:** close the gaps only a real iPhone/iPad finds.

**Inputs:** the user's device notes + screenshots from Plan 07's packs.

**Steps**

- [ ] Record each verdict item as a checkbox with the device, orientation and
  build SHA.
- [ ] Fix one item at a time with a host test when the defect has a pure seam
  (insets, anchors, widths, hit-testing); otherwise screenshot before/after.
- [ ] Keep desktop screenshots unchanged for every fix.
- [ ] Update this file's checklist and append a session per fix wave.
- [ ] Commit each wave: `fix(ui): <device verdict item>`.

**Exit:** the user signs off the mobile shell on both devices.

**Log:** append a session to `chunk-26.json`.
