# Plan 11 — Bottom bar parity: drag, dock, collapse

**Goal:** the tablet's bottom options capsule behaves like the phone's pocket
strip: drag its grip to dock left/center/right, fold it to a pill, and scroll
horizontally with no visible scrollbar.

**Device verdict (e22e402 IPA, 2026-10-28):** "it's not reposining… remove the
scroll bar from that bottom tool bar we just need scroll function not the
scrollbar itself there and make that tool bar collapsable".

**Root cause:** Plan 04 anchored the options capsule bottom-centre with a
default `ScrollArea` (visible bar) and no grip or collapse control; Plan 09's
pocket-strip mechanics (`strip_collapsed`, `strip_dock`, the grip drag) were
gated to `if phone`, so the tablet capsule had none of the three behaviours.

**Files:** `apps/procraft/src/lib.rs` (`mobile_chrome`, strip geometry helpers).

**Steps**

- [x] Share the strip geometry between both bars: `strip_left`,
      `strip_max_left`, `strip_left_clamped`, `strip_dock_for`,
      `strip_left_now`, `strip_mini_width` (renamed from the `phone_*` ones).
- [x] Extract the grip drag/release bookkeeping into `strip_drag_ui` so the
      phone pill and the tablet capsule snap identically.
- [x] Tablet capsule: leading grip, horizontally scrolling control row with
      `ScrollBarVisibility::AlwaysHidden`, trailing ChevronDown that collapses.
- [x] Collapsed capsule renders the phone's pocket pill: grip, current tool
      pill (tap expands), ChevronUp.
- [x] Capsule docks via the shared `strip_dock`/`strip_offset` state, anchored
      `LEFT_BOTTOM` at the docked left edge.
- [x] Keep the capsule visible while collapsed (it used to vanish with
      `phone && strip_collapsed`).
- [x] Tests: minimised width clamp, docked-left-with-offset math.
- [x] Screenshots: `PROCRAFT_FORCE_SHELL=mobile`, tablet docked left/center/
      right and collapsed, phone pill unchanged, desktop unchanged.
- [x] Commit: `feat(ui): dockable, collapsible bottom bar on tablets`.

**Exit:** on a tablet the options bar can be dragged to either thumb corner,
collapses to a pill, and never paints a scrollbar. Desktop is untouched.
