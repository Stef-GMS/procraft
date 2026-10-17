# M48 Transform Handles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drive the tested M43/M44 engine ops from on-canvas drag handles.

**Architecture:** New `XformMode` arming state on `ProCraftApp` (same arm-then-drag pattern as `sym_move`/`vp_move` in `apps/procraft/src/lib.rs`); handles drawn with `ui.painter()` through the existing `to_screen`/`to_canvas` rotation-aware closures; engine op applied once on drag release (a full-canvas resample per tick would hang like the pre-throttle filter previews).

**Tech Stack:** Rust, egui 0.36 (existing `Sense::click_and_drag`, `resp.drag_started/dragged/drag_stopped`), existing `Document::distort_layer` / `Document::warp_layer`.

**Spec:** `crates/engine/src/document.rs:1178-1257` (op contracts: canvas-space, offsets untouched, masks travel, caller checkpoints) + carried ruling (touch-handle design; iPad verdict input now in: 28px grab radius and touch-sized controls proven).

## Global Constraints

- Caller checkpoints for undo first (`self.doc.checkpoint()` before each op).
- `cargo fmt -p procraft-engine -p procraft -- --check` clean (never whole-tree: rfd-shim drifts).
- `cargo clippy -p procraft-engine -p procraft -- -D warnings` zero.
- `cargo test --workspace` green (currently 200).
- `cargo check -p procraft --target aarch64-linux-android` + `x86_64-linux-android` green (CI parity; iOS check runs in CI).
- Touch-sized: grab radius 28.0 screen px (VP precedent, `lib.rs:5127`); handle visuals ≥ 9px radius.
- No push/IPA/release without explicit user authorization.

## Review Focus

- Rotated view (`view_angle != 0`) + handle drag: mapping rides the existing `to_canvas`/`rot_pt` path shared with the VP/symmetry handles (no new math), verified by PC drag repro + iPad retest in Task 3.
- Panning mid-gesture (`view_offset` change while a handle is grabbed): handle must not jump — pinned by Task 1 test.
- Mesh grid length mismatch (`warp_layer` silently no-ops on wrong len): grid builder must always emit exactly `(cols+1)*(rows+1)` points — pinned by Task 2 test.
- Release with zero handle movement: must not checkpoint (avoids empty undo steps) — pinned by Task 1 + 2 tests.
- Cancel path leaves pixels bit-identical (apply-on-release means nothing mutated, but pin it) — pinned by Task 3 test.

---

### Task 1: Corner-pin handles (Distort)

**Files:**
- Modify: `apps/procraft/src/lib.rs` (app struct fields ~`vp_grab`, canvas input chain ~`5110-5170`, handle drawing ~`4916-4945`, Transform panel ~`4411`)
- Test: `apps/procraft/src/lib.rs` (inline `#[cfg(test)]` module, following existing app-test precedent)

**Interfaces:**
- Consumes: `Document::distort_layer(&mut self, index: usize, quad: [(f32, f32); 4])`, `Document::active: usize`, `vp_handle_pos(rect, z, x, y) -> (Pos2, bool)`, `rot_pt/unrot_pt`, existing `to_screen`/`to_canvas` closures.
- Produces: `XformMode` enum (`Off`, `Corners`, `Mesh`), `ProCraftApp.xform: XformMode`, `ProCraftApp.xform_quad: [(f32, f32); 4]`, `ProCraftApp.xform_grab: Option<usize>` for Task 2/3.

- [ ] **Step 1: Write the failing test** — pending-quad builder: identity positions map canvas corners to themselves, and a moved corner lands in the quad slot:

```rust
#[test]
fn xform_quad_tracks_dragged_corner() {
    let quad = quad_with_drag(identity_quad(8.0, 8.0), 1, (6.0, 1.0));
    assert_eq!(quad[1], (6.0, 1.0));
    assert_eq!(quad[0], (0.0, 0.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p procraft xform_quad_tracks_dragged_corner`
Expected: FAIL with "not found / function not defined".

- [ ] **Step 3: Implement `XformMode`, struct fields (`xform`, `xform_quad`, `xform_grab`), `identity_quad(w, h) -> [(f32,f32);4]` (TL/TR/BR/BL at `(0,0),(w-1,0),(w-1,h-1),(0,h-1)`), and `quad_with_drag` in `apps/procraft/src/lib.rs`.**

Arming lives in the Transform panel next to `sym_move`; the drag chain goes immediately after the mirror-center block (`~5170`) and follows it exactly: `drag_started` + `!gesture` → nearest-handle hit test at 28.0px through `vp_handle_pos`+`rot_pt`; `dragged` → `to_canvas(pos)` into the grabbed slot; `drag_stopped` → if any slot differs from identity, `checkpoint()` + `distort_layer(active, quad)`, then reset grab (keep quad so repeat tweaks start where the user left off). Set `painting=false`, `last=None`, `return` like the VP/symmetry blocks.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p procraft xform_quad_tracks_dragged_corner`
Expected: PASS.

- [ ] **Step 5: Write the failing test** — zero-movement release must not checkpoint:

```rust
#[test]
fn xform_quad_identity_needs_no_apply() {
    assert!(!quad_differs_from_identity(identity_quad(8.0, 8.0), 8.0, 8.0));
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test -p procraft xform_quad_identity_needs_no_apply`
Expected: FAIL with "not found / function not defined".

- [ ] **Step 7: Implement `quad_differs_from_identity(quad, w, h) -> bool`** (exact equality against `identity_quad`; drag positions come from the same float path so no epsilon) and gate the release-apply on it.

- [ ] **Step 8: Run workspace tests**

Run: `cargo test --workspace`
Expected: 202+ pass, 0 fail (200 existing + new).

- [ ] **Step 9: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "M48 corners: on-canvas corner-pin handles with release-apply"
```

### Task 2: Warp lattice handles (Mesh)

**Files:**
- Modify: `apps/procraft/src/lib.rs` (same regions as Task 1)
- Test: `apps/procraft/src/lib.rs` (same inline module)

**Interfaces:**
- Consumes: `XformMode`, `xform_grab` from Task 1; `Document::warp_layer(&mut self, index: usize, cols: u32, rows: u32, grid: &[(f32, f32)])` with fixed `cols=2, rows=2` (the engine-tested path).
- Produces: `ProCraftApp.xform_grid: Vec<(f32, f32)>` (9 points, row-major) for Task 3.

- [ ] **Step 1: Write the failing test** — grid builder emits exactly 9 identity points for 2x2 over 8x8:

```rust
#[test]
fn xform_grid_identity_has_nine_points() {
    let g = identity_grid(8.0, 8.0, 2, 2);
    assert_eq!(g.len(), 9);
    assert_eq!(g[4], (4.0, 4.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p procraft xform_grid_identity_has_nine_points`
Expected: FAIL with "not found / function not defined".

- [ ] **Step 3: Implement `identity_grid(w, h, cols, rows) -> Vec<(f32,f32)>`** (regular canvas positions, row-major, matching the engine doc: identity is an exact no-op) plus the Mesh drag chain mirroring Task 1 (nearest-of-9 hit test at 28.0px; release applies `checkpoint()` + `warp_layer(active, 2, 2, &grid)` gated on any-point-differs, same empty-undo guard style as Task 1).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p procraft xform_grid_identity_has_nine_points`
Expected: PASS.

- [ ] **Step 5: Run workspace tests**

Run: `cargo test --workspace`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "M48 mesh: 3x3 warp lattice handles with release-apply"
```

### Task 3: Panel arming + Reset + cancel

**Files:**
- Modify: `apps/procraft/src/lib.rs` (Transform panel `~4411-4443`)
- Test: `apps/procraft/src/lib.rs` (same inline module)

**Interfaces:**
- Consumes: `XformMode`, `xform_quad`, `xform_grid` from Tasks 1–2.
- Produces: nothing (final UI surface).

- [ ] **Step 1: Write the failing test** — arming Corners seeds the identity quad, switching to Off clears the grab:

```rust
#[test]
fn xform_arm_corners_seeds_identity() {
    let mut app = test_app(8, 8);
    app.arm_xform(XformMode::Corners);
    assert_eq!(app.xform_quad, identity_quad(8.0, 8.0));
    assert_eq!(app.xform_grab, None);
}
```

(If `ProCraftApp` has no light constructor, the test builds the two fields' logic via a small `XformState` struct instead — same assertions, struct owns `mode/quad/grid/grab`. Prefer `XformState` if construction is heavy; Tasks 1–2 fields then live on it.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p procraft xform_arm_corners_seeds_identity`
Expected: FAIL with "not found / function not defined".

- [ ] **Step 3: Implement the Transform panel UI**: `Off`/`Corners`/`Mesh` radio (default Off; switching arms seeds identity positions and clears grab), `Reset` button (re-seeds identity, clears grab, status "Transform reset."), `Esc` while armed clears grab without applying (nothing mutated yet — apply happens only on release). Handle drawing: 4 red corner circles / 9 lattice dots + connecting lines through `ui.painter()`, parked-edge markers via `vp_handle_pos` exactly like the symmetry handle.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p procraft xform_arm_corners_seeds_identity`
Expected: PASS.

- [ ] **Step 5: Run full gates**

Run: `cargo test --workspace` (all green), `cargo fmt -p procraft-engine -p procraft -- --check` (clean), `cargo clippy -p procraft-engine -p procraft -- -D warnings` (zero), `cargo check -p procraft --target aarch64-linux-android`, `cargo check -p procraft --target x86_64-linux-android` (green).
Expected: all green. TDD exception (repo precedent): drag wiring itself is view-only — covered by gates + user repro on PC build + iPad IPA retest.

- [ ] **Step 6: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "M48 panel: Distort/Mesh arming with Reset and Esc cancel"
```
