# Tablet & Mobile Top Bar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** iPad + Android-tablet top bar becomes the minimal folded bar with touch-sized buttons and iOS safe-area padding; less-used side-panel sections fold like "Selection Studio". Nothing else changes.

**Architecture:** One new `BarLayout::Tablet` variant selected by `bar_layout(width, mobile)` (mobile = `cfg!(ios|android)`), reusing the existing phone "View ▾" fold and `view_controls(ui, true)`. iOS insets come from `UIWindow.safeAreaInsets` through a new `rfd-shim` bridge fn (mirroring the existing `android_status_bar_height` pattern), read per frame. Panel headings become `ui.collapsing` (default-collapsed) exactly like the existing `ui.collapsing("Selection Studio", …)`.

**Tech Stack:** Rust, egui/eframe 0.36, objc2 + objc2-ui-kit (already in `rfd-shim`), egui `CollapsingHeader`.

**Spec:** `docs/superpowers/specs/2026-10-17-tablet-topbar-design.md`

## Global Constraints

- No new dependencies (objc2_ui_kit is already a dep of `crates/rfd-shim`).
- Desktop rendering must not change: `bar_layout(width ≥ 600, false) == Wide`, full inline row.
- Tablet touch target: `interact_size.y = 40.0`, `item_spacing = (8.0, 8.0)` (fabs stay 44×44).
- The Android inset path (`android_inset_top`, cached JNI, `clamp_status_inset` ≤ 200) stays byte-identical.
- Full suite green: 216 engine + 41 app tests, `cargo fmt`, `clippy -D warnings`, Android `cargo check` (aarch64 + x86_64).
- Version stays 0.8.5. Commit style `feat(app): …` / `fix(...)` as used in the repo.

## Review Focus

- Split-screen / Slide Over iPad (320–599pt window) must fall back to the phone Narrow bar — pinned by Task 1 test (`bar_layout(320,true)=Narrow`, `(599,true)=Narrow`).
- iPhone landscape (≥600pt, mobile) gets the Tablet bar in a short viewport: the row must wrap, not clip — pinned by Task 1 test (`bar_layout(844,true)=Tablet`) + device visual in Task 4.
- `safeAreaInsets` unavailable (no window / non-main-thread): the call errors → insets 0 → bar renders unpadded, no panic — Task 2 error path.
- Rotation changes insets (iPad portrait ≈24pt top → landscape ≈0): per-frame read keeps padding correct — Task 2.
- Bottom status label vs home indicator: `add_space(bottom)` after the label lifts it clear — Task 2 + device visual.

---

### Task 1: `bar_layout` tablet variant + bar call site

**Files:**
- Modify: `apps/procraft/src/lib.rs` — enum + `bar_layout` (~2477-2490), bar panel entry (~3594-3608), view-cluster branch (~3841-3850), test (~8148-8152)
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `enum BarLayout { Narrow, Wide, Tablet }` (keep existing derives, add nothing else) and `fn bar_layout(width: f32, mobile: bool) -> BarLayout` — `< 600` → Narrow; `≥ 600` and `mobile` → Tablet; else Wide.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing test** — replace `bar_layout_splits_at_phone_width` (~8148) with:

```rust
#[test]
fn bar_layout_splits_at_phone_width() {
    assert_eq!(bar_layout(320.0, true), BarLayout::Narrow);
    assert_eq!(bar_layout(599.0, true), BarLayout::Narrow);
    assert_eq!(bar_layout(360.0, false), BarLayout::Narrow);
    assert_eq!(bar_layout(600.0, true), BarLayout::Tablet);
    assert_eq!(bar_layout(820.0, true), BarLayout::Tablet);
    assert_eq!(bar_layout(844.0, true), BarLayout::Tablet);
    assert_eq!(bar_layout(600.0, false), BarLayout::Wide);
    assert_eq!(bar_layout(1280.0, false), BarLayout::Wide);
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p procraft bar_layout` → arity/type errors (fn takes 1 arg today).

- [ ] **Step 3: Implement** — add `Tablet` to the enum; change the fn:

```rust
fn bar_layout(width: f32, mobile: bool) -> BarLayout {
    if width < 600.0 {
        BarLayout::Narrow
    } else if mobile {
        BarLayout::Tablet
    } else {
        BarLayout::Wide
    }
}
```

Call site: at the TOP of the `Panel::top("menu")` body (before any widget renders, after the Android inset space), compute once:

```rust
let layout = bar_layout(
    ui.ctx().viewport_rect().width(),
    cfg!(any(target_os = "ios", target_os = "android")),
);
if layout == BarLayout::Tablet {
    ui.spacing_mut().interact_size.y = 40.0;
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
}
```

The branch at ~3844 becomes `if matches!(layout, BarLayout::Narrow | BarLayout::Tablet)` → View ▾ menu, else inline `view_controls(ui, false)` (delete the old `bar_layout(...)` recompute).

- [ ] **Step 4: Run tests** — `cargo test -p procraft bar_layout` PASS, then full `cargo test --workspace` PASS.

- [ ] **Step 5: Commit** — `git add apps/procraft/src/lib.rs` → `git commit -m "feat(app): tablet bar — folded view cluster, touch-sized buttons"`

---

### Task 2: iOS safe-area insets (top + bottom)

**Files:**
- Modify: `crates/rfd-shim/src/ios_bridge.rs` — extract `find_window`, add `safe_area_insets`
- Modify: `crates/rfd-shim/src/lib.rs` — re-export next to `android_status_bar_height` (~133)
- Modify: `apps/procraft/src/lib.rs` — iOS pad in `"menu"` panel (after the Android block ~3607) and in `Panel::bottom("status")` (~6414)

**Interfaces:**
- Produces: `pub fn ios_safe_area_insets() -> Result<(u32, u32), &'static str>` — `(top, bottom)` in points, iOS only, errors carry a reason.
- Consumes: existing `clamp_status_inset` (app, ~2496) for clamping.

- [ ] **Step 1: Implement the bridge** — in `ios_bridge.rs` split `find_root` into `find_window()` (the existing keyWindow → key window → any window walk) + `find_root()` calling it; add:

```rust
/// Window safe-area insets (top, bottom) in points for edge-to-edge padding.
pub(super) fn safe_area_insets() -> Result<(u32, u32), &'static str> {
    let window = find_window()?;
    let insets = window.safeAreaInsets();
    Ok((
        insets.top.max(0.0).round() as u32,
        insets.bottom.max(0.0).round() as u32,
    ))
}
```

(`UIWindow::safeAreaInsets()` — if the objc2-ui-kit version lacks the accessor, use `msg_send![&window, safeAreaInsets]` with `objc2_ui_kit::UIEdgeInsets`; verify against the vendored crate.)

- [ ] **Step 2: Re-export in `rfd-shim`**

```rust
/// Window safe-area insets (top, bottom) in points (call per frame; a cheap
/// property read, correct across rotation). iOS only; errors carry the
/// reason, callers fall back to 0.
#[cfg(target_os = "ios")]
pub fn ios_safe_area_insets() -> Result<(u32, u32), &'static str> {
    ios_bridge::safe_area_insets()
}
```

- [ ] **Step 3: Pad the top bar (app)** — after the `#[cfg(target_os = "android")]` inset block (~3607):

```rust
#[cfg(target_os = "ios")]
{
    let (top, _) = rfd::ios_safe_area_insets().unwrap_or((0, 0));
    let pad = clamp_status_inset(top);
    if pad > 0.0 {
        ui.add_space(pad);
    }
}
```

- [ ] **Step 4: Pad the bottom status panel (app)** — inside `Panel::bottom("status")` after the label:

```rust
#[cfg(target_os = "ios")]
{
    let (_, bottom) = rfd::ios_safe_area_insets().unwrap_or((0, 0));
    let pad = clamp_status_inset(bottom);
    if pad > 0.0 {
        ui.add_space(pad);
    }
}
```

- [ ] **Step 5: Verify no desktop regression** — `cargo test --workspace` PASS, `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check -p procraft --target aarch64-linux-android` + `x86_64-linux-android` PASS. (iOS code compiles only in CI — Task 4.)

- [ ] **Step 6: Commit** — `git commit -m "feat(app): iOS safe-area padding for top bar + status line"`

---

### Task 3: Side-panel folds (collapsed like "Selection Studio")

**Files:**
- Modify: `apps/procraft/src/lib.rs` — wrap section bodies in `ui.collapsing`:
  - left panel: `ui.heading("My brushes")` (~4675), `ui.heading("Page")` (~5122), `ui.heading("Guides")` (~5135)
  - right panel: `ui.heading("Animation")` (~5619), `ui.heading("Transform layer")` (~5873), `ui.heading("Symmetry")` (~5980), `ui.heading("Filters")` (~6018)

**Interfaces:**
- Consumes: nothing (pure UI tree change).
- Produces: seven default-collapsed sections (egui `CollapsingHeader` default), same behavior as `ui.collapsing("Selection Studio", …)` at ~4495.

- [ ] **Step 1: Fold the left panel** — replace each heading with `ui.collapsing("Name", |ui| { …existing section body unchanged… });`. If a section body mutates an accumulator consumed after it, declare that accumulator before the closure and mutate inside (same style the Layers block already uses for `remove`/`select`/`grp_*`).

- [ ] **Step 2: Fold the right panel** — same conversion for Animation, Transform layer, Symmetry, Filters. Do NOT touch the `ui.heading("Layers")` block or its content.

- [ ] **Step 3: Compile + suite** — `cargo test --workspace` PASS (no new unit tests: pure UI tree; behavior gate is compile + suite + Task 4 device check), `cargo fmt --all`, clippy clean.

- [ ] **Step 4: Commit** — `git commit -m "feat(app): fold less-used side-panel sections (Selection Studio pattern)"`

---

### Task 4: Gates, desktop smoke, push, iOS build, log

**Files:**
- Modify: `conversationLog/chunk-22.json` (append session entry; parse → append → compact JSON, UTF-8 no BOM)

**Interfaces:**
- Consumes: all previous tasks.
- Produces: pushed `main`, CI `ios-ipa.yml` run + artifact `ProCraft-ios-v1.ipa` for device testing.

- [ ] **Step 1: Full gates** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; Android checks. All green.
- [ ] **Step 2: Desktop smoke** — run the debug exe; confirm the desktop bar is byte-identical (full inline row, no View ▾ fold, no extra padding).
- [ ] **Step 3: Commit + push** — `git add -A` (exclude `dist/`), commit stragglers, `git push origin main`.
- [ ] **Step 4: iOS build** — `gh workflow run ios-ipa.yml --repo surfrpt1/procraft`, watch to success (this is also the iOS compile gate for Task 2), download artifact link for the user.
- [ ] **Step 5: Hand to user** — iPad test list: bar = one row below the status bar, nothing cut; View ▾ has zoom/Fit/Rotate/Reset/Theme/About/Shortcuts; bottom status text clears the home indicator; the seven panel sections start collapsed and expand on tap; rotate the iPad (portrait↔landscape) and re-check the bar.
- [ ] **Step 6: After user OK** — append the session entry to `conversationLog/chunk-22.json`; offer an Android APK build + releases as the next step.

---

## Self-Review

- **Spec coverage:** tablet bar → Task 1; iOS top+bottom insets → Task 2; panel folds → Task 3; desktop-unchanged → Task 1 tests + Task 4 smoke; device verification → Task 4. No gaps.
- **Step scan:** every step names files, exact signatures, or exact commands; no TBDs.
- **Type consistency:** `BarLayout::Tablet`, `bar_layout(width, mobile)`, `ios_safe_area_insets() -> Result<(u32,u32),…>` used identically across tasks.
- **Review Focus:** each line has an owning test or an explicit device-visual step (Task 4).
- **Proportion:** plan ≈ spec size; no code transcripts beyond signatures and pinned lines.
