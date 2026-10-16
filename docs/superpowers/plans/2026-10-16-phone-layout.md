# Phone Layout Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ProCraft usable on phone-portrait Android (~360px): collapsible-via-drag sidebars, a narrow top bar, and no system-status overlap.

**Architecture:** Three independent fixes in `apps/procraft/src/lib.rs` plus one JNI bridge addition in `crates/rfd-shim/src/android_bridge.rs`. No engine changes. Standing user authorization ("clear the queue") covers implementation; test-APK dispatch needs no extra ask (device verify is part of Task 4).

**Tech Stack:** Rust, egui 0.36 `Panel::show_collapsible`, existing `menu_button` pattern, existing JNI `with_attached` bridge.

**Spec:** Father's-phone screenshot (360px portrait: Rotate° stepper starved to ~15px and wrapping char-by-char, zoom row overflowing, status bar over Gallery, sidebars eating the canvas with one-way resize). S1 (Android 8 parse error) closed as out-of-scope: APK ships only `aarch64 + x86_64` for Android 10+.

## Global Constraints

- Caller checkpoints for undo first (no doc mutation in this plan at all — all view-only).
- `cargo fmt -p procraft-engine -p procraft -- --check` clean (never whole-tree: rfd-shim drifts).
- `cargo clippy -p procraft-engine -p procraft -- -D warnings` zero.
- `cargo test --workspace` green (currently 203).
- `cargo check -p procraft --target aarch64-linux-android` + `x86_64-linux-android` green.
- Touch-sized controls throughout; no behavior change on ≥600px viewports.
- Push to main when green (authorized); debug APK via `android-apk` workflow for father's-phone verify.

## Review Focus

- `show_collapsible` drag-to-collapse must not eat the existing ◀/▶ buttons — both drive the same `show_tools`/`show_layers` bools; pin by keeping button code untouched and verifying on desktop narrow window.
- Narrow bar must not hide Gallery/Reference/File/Undo/Redo/Clear — only zoom/rotate/theme/About move; pinned by Task 2 test on the item lists.
- Status inset must be 0 on desktop/iOS (Android-only code path) and clamped — a wild JNI value must never eat half the screen; pinned by Task 3 clamp test.
- Panel ids stay `tools-compact` / `layers-wide` so persisted widths carry over (no silent width reset); naming this here so no task "cleans it up."
- The stale comment at the Theme+About block (mentions a removed right_to_left pin) gets corrected in passing by whichever task touches it — not a separate task.

---

### Task 1: Drag-to-collapse sidebars (S3)

**Files:**
- Modify: `apps/procraft/src/lib.rs` (`Panel::left("tools-compact")` show call, `Panel::right("layers-wide")` show call)
- Test: none new (egui-internal behavior; TDD exception logged: covered by gates + desktop-narrow repro + father's-phone verify, same precedent as scrollbar fix)

**Interfaces:**
- Consumes: existing `self.show_tools: bool`, `self.show_layers: bool` (become the `is_expanded` flags).
- Produces: collapsible panels for Tasks 2–4 (no signature changes).

- [ ] **Step 1: Migrate tools panel** — `Panel::left("tools-compact").default_size(240.0).show(ui, |ui| {...})` becomes `.show_collapsible(ui, &mut self.show_tools, |ui| {...})` (borrow discipline: copy the flag dance used nowhere else — `show_collapsible` needs `&mut self.show_tools` while the closure also uses `self`; hoist `let mut show = self.show_tools;` pattern or restructure per borrowck — implementer picks the minimal compiling form, keeping panel id identical).

- [ ] **Step 2: Migrate layers panel** — same migration with `self.show_layers` on `Panel::right("layers-wide")`.

- [ ] **Step 3: Verify on desktop** — `cargo run -p procraft`, narrow window to ~400px, open both sidebars, drag edge thinner past minimum: panel must collapse (not clamp); press ▶: reopens (drag-to-open is off by design, so drag-out does NOT reopen). Note: dev must eyeball; record result in commit message trailer.

- [ ] **Step 4: Run gates** — `cargo test --workspace` (203 green), scoped fmt, clippy zero.

- [ ] **Step 5: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "Phone layout: drag-to-collapse sidebars"
```

### Task 2: Narrow top bar (S2a)

**Files:**
- Modify: `apps/procraft/src/lib.rs` (top `"menu"` panel body, zoom/rotate/theme/about section)
- Test: `apps/procraft/src/lib.rs` (inline tests module)

**Interfaces:**
- Consumes: viewport width via `ui.ctx().viewport_rect().width()` (same call as the `narrow_setup_done` block).
- Produces: `bar_layout(width: f32) -> BarLayout` (`Wide`/`Narrow`) and `PHONE_BAR_W: f32 = 600.0` for Task 3 (threshold shared).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bar_layout_splits_at_phone_width() {
    assert_eq!(bar_layout(360.0), BarLayout::Narrow);
    assert_eq!(bar_layout(599.0), BarLayout::Narrow);
    assert_eq!(bar_layout(600.0), BarLayout::Wide);
    assert_eq!(bar_layout(1280.0), BarLayout::Wide);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p procraft bar_layout_splits_at_phone_width`
Expected: FAIL with "not found" (E0425).

- [ ] **Step 3: Implement `BarLayout` + `bar_layout` + `PHONE_BAR_W`** and branch the menu body: Narrow keeps Gallery/Reference/File/Undo/Redo/Clear inline and moves zoom group + Rotate stepper + Reset + theme ComboBox + About into a `View` `menu_button` (e4e9339 precedent); Wide keeps the exact current layout pixel-for-pixel (no Wide-path changes — reviewer diffs Narrow only).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p procraft bar_layout_splits_at_phone_width`
Expected: PASS.

- [ ] **Step 5: Run workspace tests**

Run: `cargo test --workspace`
Expected: all green (204+).

- [ ] **Step 6: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "Phone layout: narrow top bar folds zoom/rotate/theme into View menu"
```

### Task 3: Android status inset (S2b)

**Files:**
- Modify: `crates/rfd-shim/src/android_bridge.rs` (new fn), `crates/rfd-shim/src/lib.rs` (re-export), `apps/procraft/src/lib.rs` (top padding in `"menu"` panel, Android-only)
- Test: `apps/procraft/src/lib.rs` (inline tests module, clamp helper)

**Interfaces:**
- Consumes: existing `with_attached` JNI helper; `BarLayout`/`PHONE_BAR_W` from Task 2 only for threshold sharing (no signature coupling).
- Produces: `rfd_shim::android_status_bar_height() -> u32` (px, 0 on error) and `clamp_status_inset(px: u32) -> f32` for Task 4.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn status_inset_clamps_wild_values() {
    assert_eq!(clamp_status_inset(0), 0.0);
    assert_eq!(clamp_status_inset(72), 72.0);
    assert_eq!(clamp_status_inset(9999), 200.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p procraft status_inset_clamps_wild_values`
Expected: FAIL with "not found" (E0425).

- [ ] **Step 3: Implement the bridge** — `android_status_bar_height()` via activity `getWindow().getDecorView().getWindowVisibleDisplayFrame(rect)` returning `rect.top` (px), errors → 0 (same `with_attached` + error-to-status-line style as `external_files_dir`); re-export in `rfd_shim::lib.rs` beside `android_external_files`; app side: `#[cfg(target_os = "android")]` block at the top of the `"menu"` panel adding `ui.add_space(clamp_status_inset(px))`. Desktop/iOS paths untouched (0 by construction — no `cfg` needed on the helper itself, but the call site is Android-gated).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p procraft status_inset_clamps_wild_values`
Expected: PASS.

- [ ] **Step 5: Run gates incl. Android targets**

Run: `cargo test --workspace`, scoped fmt, clippy zero, both Android `cargo check` targets.
Expected: all green (JNI compiles under the Android target; host build uses the declining shim path unchanged).

- [ ] **Step 6: Commit**

```bash
git add crates/rfd-shim/src/android_bridge.rs crates/rfd-shim/src/lib.rs apps/procraft/src/lib.rs
git commit -m "Phone layout: Android status-bar inset padding"
```

### Task 4: Push + test APK + verify

**Files:** none (process task).

- [ ] **Step 1: Push** — `git push origin main` (authorized).

- [ ] **Step 2: Dispatch debug APK** — `gh workflow run android-apk.yml --ref main` (default `debug` profile = test build).

- [ ] **Step 3: Wait + download** — `gh run watch`, download artifact, copy to Desktop as `ProCraft-android-phonefix.apk`, report SHA to user.

- [ ] **Step 4: Hand to user** — father's-phone checklist: (a) top bar fits without char-wrap, View menu holds zoom/rotate/theme; (b) Gallery row clear of status bar; (c) sidebar drag-thin collapses, drag-out/▶ reopens. Await verdicts before any release tagging.
