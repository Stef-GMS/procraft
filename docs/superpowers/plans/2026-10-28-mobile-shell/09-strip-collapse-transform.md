# Plan 09 — Collapsible strip and transform improvements

**Goal:** make the phone strip behave like Apple's floating drawing bars
(collapse to a pocket pill, move it to either side) and make the Transform
layer controls worth using (live scale/rotation, one undo per gesture,
modifier gestures, clean resampling).

**Scope:** the phone strip is mobile-only. The Transform layer panel and
on-canvas handles exist on desktop too; their improvements are shared, but no
desktop layout moves.

**Inputs:** user request 2026-10-28 (while testing build `406b8a6`): "make it
collapsable like how apple freeform/notes does and also make it like them like
how they change positions", plus "make improvements to transform layers
functions".

**Steps**

- [x] Persist the strip presentation (`collapsed`, `dock`) in `Settings`, with
  pure anchor/snap helpers and tests.
- [x] Phone strip: expanded bar plus a compact pill (grip, active tool, Undo,
  expand); the grip drags the pill and snaps to left/centre/right; the options
  capsule hides while collapsed.
- [x] Transform session: absolute scale/rotation setters, Reset, checkpoint
  once per slider gesture, live readouts.
- [x] Free handles: Shift locks corner aspect, Alt scales about the frame
  centre, Shift snaps rotation to 15°.
- [x] Bilinear resampling premultiplies alpha, so scaled/rotated/warped edges
  stop darkening against transparency.
- [x] Phone More menu: one-tap Transform / Finish Transform.
- [x] Keyboard-free verification: forced-shell captures for collapsed,
  left-docked, right-docked and the transform panel; desktop spot check.
- [x] `cargo fmt --check`, `clippy -D warnings`, both test suites, log session,
  commit.

**Exit:** the strip can be minimised and moved without losing tool access, and
the transform panel previews live without compounding blur or undo spam.

**Log:** append a session to `D:\conversationLog\chunk-26.json`.

## Design notes

- The strip has two presentations: the existing full-width scrollable bar and
  a mini pill that keeps the current tool, Undo and an expand chevron (the size
  readout is dropped so the pill stays small enough to move). Collapsing hides
  the options capsule too; the pill is the collapsed state's whole UI.
- The collapse chevron is pinned after the scroll area, so it is always
  reachable and the existing tool pills keep their positions.
- The pill is dragged by a painted grip. Release snaps it to whichever of the
  three docked positions (left / centre / right) its centre is nearest; they
  have to be computed from the pill's own width, not viewport thirds, or a wide
  pill always snaps to the centre. The dock is persisted so the bar opens where
  it was left.
- Transform sliders write absolute values into the same session as the old
  buttons. The session still re-applies from one pristine base, and the first
  change in a drag pushes the single undo checkpoint; a discrete press closes
  the gesture so the next one gets its own step.
- Modifiers are desktop affordances; touch behaviour is unchanged.

## Verification (2026-10-28)

- Strip states: `dist/mobile-shell/2026-10-28/plan09-phone-strip-expanded.png`,
  `plan09-phone-strip-pocket-{center,left,right}.png`, and
  `plan09-phone-collapse-tap.png` (tap the chevron: collapsed and
  `"strip_collapsed": true` written to settings).
- Transform panel on phone: `plan09-phone-transform-panel.png` (Size 100% /
  Angle 0 deg sliders in the Layers drawer).
- Desktop: `plan09-desktop-dark.png` is unchanged against
  `plan08-desktop-dark.png` apart from a 1px canvas-edge shift and the cursor
  ring (capture jitter).
- Found while verifying: `settings_differ` did not compare the two new fields,
  so collapsing never persisted. Fixed, with the test extended to cover both.
- Dragging the pill itself is not scriptable in the capture harness; the snap
  and clamp maths are covered by `pocket_strip_docks_to_three_thumb_positions`
  and the persisted-state shots above use the same settings fields.

